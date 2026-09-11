#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn child(home: &Path, base: &str, args: &[&str]) -> Child {
    let config = home.join(".config/sunox");
    std::fs::create_dir_all(&config).expect("config");
    std::fs::write(config.join("auth.json"), json!({
        "jwt": "e30.eyJzdWIiOiJ1c2VyLXRlc3QiLCJleHAiOjQxMDI0NDQ4MDB9.c2ln",
        "device_id": "fake-device",
        "browser_environment": {
            "browser_source": "cli-http-test", "user_agent": "Mozilla/5.0 Chrome/136.0.0.0",
            "accept_language": "en-US",
            "client_hints": {"sec_ch_ua":"\"Chromium\";v=\"136\"", "sec_ch_ua_mobile":"?0", "sec_ch_ua_platform":"\"macOS\""}
        }
    }).to_string()).expect("fake auth");
    let mut command = Command::new(assert_cmd::cargo::cargo_bin("sunox"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("SUNOX_")) {
        command.env_remove(key);
    }
    command
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("SUNOX_TEST_API_BASE_URL", base)
        .env("NO_PROXY", "127.0.0.1,localhost")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI process")
}

fn read_request(stream: &mut TcpStream) -> (String, Value) {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read deadline");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let boundary = loop {
        let count = stream.read(&mut chunk).expect("read request");
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..boundary]);
    let path = headers
        .lines()
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("path")
        .to_string();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("length"))
        })
        .unwrap_or(0);
    while bytes.len() < boundary + length {
        let count = stream.read(&mut chunk).expect("request body");
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    let value = serde_json::from_slice(&bytes[boundary..boundary + length]).unwrap_or(Value::Null);
    (path, value)
}

fn reply(stream: &mut TcpStream, body: Value) {
    let body = body.to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).expect("response");
}

fn billing() -> Value {
    json!({"credits":100,"total_credits_left":100,"monthly_usage":0,"monthly_limit":2500,
        "is_active":true,"plan":{"id":"tier-pro","name":"Pro Plan","plan_key":"pro","usage_plan_features":[]},
        "models":[{"name":"v6","external_key":"chirp-hawk","can_use":true,"is_default_model":true,"description":"test","capabilities":["all"],"features":["create_control_sliders"],"badges":["pro"],"max_lengths":{}}],
        "period":"month","renews_on":null,"remaster_model_types":[]})
}

fn interrupt_and_wait(mut process: Child) -> Output {
    // The server has observed the exact target stage before we deliver Ctrl+C.
    assert_eq!(unsafe { libc::kill(process.id() as i32, libc::SIGINT) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while process.try_wait().expect("process status").is_none() {
        if Instant::now() >= deadline {
            process.kill().expect("stop hung child");
            panic!("interrupted CLI did not exit within five seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    process.wait_with_output().expect("CLI output")
}

fn error(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(130));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let start = stderr.find('{').expect("JSON error");
    let error: Value = serde_json::from_str(&stderr[start..]).expect("error envelope");
    assert_eq!(error["error"]["code"], "interrupted");
    assert!(!stderr.contains("operation was cancelled"));
    error
}

#[test]
fn interrupt_after_generation_send_keeps_the_transaction_before_any_response() {
    let home = tempfile::tempdir().expect("test home");
    let listener = TcpListener::bind("127.0.0.1:0").expect("server");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let (sender, receiver) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let server = std::thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().expect("request");
            let (path, body) = read_request(&mut stream);
            match path.as_str() {
                "/api/billing/info/" => reply(&mut stream, billing()),
                "/api/session/" => reply(
                    &mut stream,
                    json!({"flags":{"aug-creativity":true},"roles":{}}),
                ),
                "/api/c/check" => reply(&mut stream, json!({"required":false,"captcha_version":2})),
                "/api/generate/v2-web/" => {
                    sender
                        .send(
                            body["transaction_uuid"]
                                .as_str()
                                .expect("transaction")
                                .to_string(),
                        )
                        .expect("observed request");
                    finish
                        .recv_timeout(Duration::from_secs(10))
                        .expect("release server");
                    break;
                }
                _ => panic!("unexpected request: {path}"),
            }
        }
    });
    let process = child(
        home.path(),
        &base,
        &[
            "create",
            "--instrumental",
            "--tags",
            "PRIVATE PROMPT",
            "--no-captcha",
            "--json",
        ],
    );
    let transaction = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("generation was sent");
    // The identity must already be durable, not first written by the interrupt handler.
    let directory = home.path().join(".config/sunox/operations");
    let checkpoint = std::fs::read_dir(&directory)
        .expect("recovery directory")
        .next()
        .expect("checkpoint")
        .expect("entry")
        .path();
    let before = std::fs::read_to_string(&checkpoint).expect("checkpoint before interrupt");
    assert!(before.contains(&transaction));
    assert!(!before.contains("PRIVATE PROMPT"));
    let result = error(&interrupt_and_wait(process));
    let recovery = &result["error"]["details"]["operation_recovery"];
    assert_eq!(recovery["remote_effects_possible"], true);
    assert_eq!(recovery["resumable"], false);
    assert!(recovery.to_string().contains(&transaction));
    let stored: Value =
        serde_json::from_slice(&std::fs::read(checkpoint).expect("durable checkpoint"))
            .expect("stored JSON");
    assert!(matches!(
        stored["state"].as_str(),
        Some("running" | "interrupted")
    ));
    release.send(()).expect("finish");
    server.join().expect("server thread");
}

#[test]
fn interrupt_after_upload_id_preserves_the_resource_and_inspection_command() {
    let home = tempfile::tempdir().expect("test home");
    let audio = home.path().join("recording.mp3");
    std::fs::write(&audio, b"ID3fake-upload-recording").expect("fixture");
    let listener = TcpListener::bind("127.0.0.1:0").expect("server");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let upload_url = format!("{base}/presigned");
    let (sender, receiver) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let server = std::thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().expect("request");
            let (path, _) = read_request(&mut stream);
            match path.as_str() {
                "/api/billing/info/" => reply(&mut stream, billing()),
                "/api/uploads/audio/" => reply(
                    &mut stream,
                    json!({"id":"upload-known","url":upload_url,"fields":{}}),
                ),
                "/presigned" => reply(&mut stream, json!({})),
                "/api/uploads/audio/upload-known/upload-finish/" => {
                    sender.send(()).expect("upload finish received");
                    finish
                        .recv_timeout(Duration::from_secs(10))
                        .expect("release server");
                    break;
                }
                _ => panic!("unexpected request: {path}"),
            }
        }
    });
    let process = child(
        home.path(),
        &base,
        &[
            "clip",
            "upload",
            audio.to_str().expect("audio path"),
            "--json",
        ],
    );
    receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("upload reached finish");
    let result = error(&interrupt_and_wait(process));
    let recovery = &result["error"]["details"]["operation_recovery"];
    assert!(
        recovery
            .to_string()
            .contains("sunox clip upload-status upload-known --json")
    );
    assert_eq!(
        recovery["writes"][0]["identifiers"]["upload_ids"],
        json!(["upload-known"])
    );
    release.send(()).expect("finish");
    server.join().expect("server thread");
}

#[test]
fn interrupt_before_any_write_does_not_claim_or_persist_a_mutation() {
    let home = tempfile::tempdir().expect("test home");
    let listener = TcpListener::bind("127.0.0.1:0").expect("server");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let (sender, receiver) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request");
        assert_eq!(read_request(&mut stream).0, "/api/billing/info/");
        sender.send(()).expect("preflight received");
        finish
            .recv_timeout(Duration::from_secs(10))
            .expect("release server");
    });
    let process = child(
        home.path(),
        &base,
        &[
            "create",
            "--instrumental",
            "--tags",
            "ambient",
            "--no-captcha",
            "--json",
        ],
    );
    receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("preflight");
    let result = error(&interrupt_and_wait(process));
    assert!(result["error"]["details"]["operation_recovery"].is_null());
    assert!(!home.path().join(".config/sunox/operations").exists());
    release.send(()).expect("finish");
    server.join().expect("server thread");
}

fn checkpoint(home: &Path) -> Value {
    let paths = std::fs::read_dir(home.join(".config/sunox/operations"))
        .expect("operation directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 1);
    serde_json::from_slice(&std::fs::read(&paths[0]).expect("checkpoint file"))
        .expect("checkpoint JSON")
}

#[test]
fn interrupt_after_nested_playlist_create_preserves_the_id_before_the_next_write() {
    let playlist = json!({"id":"playlist-known","name":"PRIVATE PLAYLIST TITLE"});
    let deferred = json!({"metadata":playlist});
    for response in [
        playlist.clone(),
        deferred.clone(),
        json!({"playlist":playlist}),
        json!({"data":playlist}),
        json!({"playlist":deferred}),
        json!({"data":deferred}),
    ] {
        let home = tempfile::tempdir().expect("test home");
        let listener = TcpListener::bind("127.0.0.1:0").expect("server");
        let base = format!("http://{}", listener.local_addr().expect("address"));
        let (sender, receiver) = mpsc::channel();
        let (release, finish) = mpsc::channel();
        let server = std::thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().expect("request");
                let (path, body) = read_request(&mut stream);
                match path.as_str() {
                    "/api/billing/info/" => reply(&mut stream, billing()),
                    "/api/playlist/create/" => reply(&mut stream, response.clone()),
                    "/api/playlist/set_metadata" => {
                        assert_eq!(body["playlist_id"], "playlist-known");
                        sender.send(()).expect("metadata received");
                        finish
                            .recv_timeout(Duration::from_secs(10))
                            .expect("release");
                        break;
                    }
                    _ => panic!("unexpected request: {path}"),
                }
            }
        });
        let process = child(
            home.path(),
            &base,
            &[
                "playlist",
                "create",
                "--name",
                "PRIVATE PLAYLIST TITLE",
                "--image-url",
                "https://example.invalid/PRIVATE_IMAGE_URL",
                "--json",
            ],
        );
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("metadata write");
        let before = checkpoint(home.path());
        // Both the actual create response and the next request carry the typed resource ID.
        for write in before["writes"].as_array().expect("writes") {
            assert_eq!(
                write["identifiers"]["playlist_ids"],
                json!(["playlist-known"])
            );
        }
        assert!(!before.to_string().contains("PRIVATE"));
        let result = error(&interrupt_and_wait(process));
        let recovery = &result["error"]["details"]["operation_recovery"];
        assert!(
            recovery["inspection_commands"]
                .as_array()
                .expect("commands")
                .contains(&json!("sunox playlist info playlist-known --json"))
        );
        assert_eq!(checkpoint(home.path())["writes"], before["writes"]);
        release.send(()).expect("finish");
        server.join().expect("server");
    }
}

#[test]
fn interrupt_during_empty_trash_keeps_discovered_targets_before_any_delete_response() {
    let home = tempfile::tempdir().expect("test home");
    let listener = TcpListener::bind("127.0.0.1:0").expect("server");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let (sender, receiver) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    let server = std::thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().expect("request");
            let (path, body) = read_request(&mut stream);
            match path.as_str() {
                "/api/billing/info/" => reply(&mut stream, billing()),
                "/api/feed/v3" => reply(
                    &mut stream,
                    json!({
                        "clips":[
                            {"id":"trash-a","title":"A","status":"complete","model_name":"chirp-fenix","created_at":"2026-09-08T00:00:00Z","is_trashed":true},
                            {"id":"trash-b","title":"B","status":"complete","model_name":"chirp-fenix","created_at":"2026-09-08T00:00:00Z","is_trashed":true}
                        ], "has_more":false,"next_cursor":null
                    }),
                ),
                "/api/clips/delete/" => {
                    assert_eq!(body, json!({"ids":["trash-a","trash-b"]}));
                    sender.send(()).expect("delete received");
                    finish
                        .recv_timeout(Duration::from_secs(10))
                        .expect("release");
                    break;
                }
                _ => panic!("unexpected request: {path}"),
            }
        }
    });
    let process = child(
        home.path(),
        &base,
        &["clip", "empty-trash", "--yes", "--json"],
    );
    receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("delete write");
    let before = checkpoint(home.path());
    assert_eq!(
        before["writes"][0]["identifiers"]["clip_ids"],
        json!(["trash-a", "trash-b"])
    );
    let result = error(&interrupt_and_wait(process));
    let recovery = &result["error"]["details"]["operation_recovery"];
    assert_eq!(recovery["writes"], before["writes"]);
    assert_eq!(checkpoint(home.path())["writes"], before["writes"]);
    for target in ["trash-a", "trash-b"] {
        assert!(
            recovery["inspection_commands"]
                .as_array()
                .expect("commands")
                .contains(&json!(format!("sunox clip info {target} --json")))
        );
    }
    release.send(()).expect("finish");
    server.join().expect("server");
}

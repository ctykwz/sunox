//! The CLI must not replace an existing file with a truncated HTTP 200 body.
use assert_cmd::Command;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn request_path(stream: &mut TcpStream, alignment: bool) -> String {
    stream
        .set_nonblocking(false)
        .expect("blocking accepted socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut chunk = [0; 4096];
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).expect("request headers");
        assert!(count > 0, "connection closed before request headers");
        request.extend_from_slice(&chunk[..count]);
    }
    let header_end = request
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = String::from_utf8(request[..header_end].to_vec()).unwrap();
    let content_length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let count = stream.read(&mut chunk).expect("request body");
        assert!(count > 0, "truncated request body");
        request.extend_from_slice(&chunk[..count]);
    }
    let mut line = headers.lines().next().unwrap().split_whitespace();
    let method = line.next().unwrap();
    let path = line.next().unwrap();
    assert!(
        method == "GET"
            || alignment
                && method == "POST"
                && (path.ends_with("/aligned_lyrics/v3") || path == "/api/feed/v3")
    );
    path.to_string()
}

fn check_download(case: &str, format: &str, body: Vec<u8>, valid: bool) {
    check_download_with_alignment(case, format, body, valid, None);
}

fn check_download_with_alignment(
    case: &str,
    format: &str,
    body: Vec<u8>,
    valid: bool,
    alignment_status: Option<u16>,
) {
    let directory = tempfile::tempdir().expect("isolated CLI fixture");
    let config = directory.path().join("config/sunox");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("auth.json"),
        json!({
            "jwt":"e30.eyJzdWIiOiJ1c2VyLXRlc3QiLCJleHAiOjQxMDI0NDQ4MDB9.c2ln",
            "browser_environment": {
                "browser_source":"cli-http-test",
                "user_agent":"Mozilla/5.0 Chrome/136.0.0.0",
                "accept_language":"en-US",
                "client_hints": {
                    "sec_ch_ua":"\"Chromium\";v=\"136\"",
                    "sec_ch_ua_mobile":"?0",
                    "sec_ch_ua_platform":"\"macOS\""
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let output = directory.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let destination = output.join(format!("track-clip-rev.{format}"));
    let previous = b"previous download must survive";
    std::fs::write(&destination, previous).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let served = body.clone();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut paths = Vec::new();
        loop {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "CLI never reached media response"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("accept: {error}"),
            };
            let path = request_path(&mut stream, alignment_status.is_some());
            let aligned = path.ends_with("/aligned_lyrics/v3");
            let done = if alignment_status.is_some() {
                aligned
            } else {
                path == "/media"
            };
            let status = if aligned {
                alignment_status.unwrap()
            } else {
                200
            };
            let bytes = match path.as_str() {
                "/api/clip/clip-review" => json!({
                    "id":"clip-review","title":"Track","status":"complete",
                    "model_name":"chirp-fenix","created_at":"2026-08-30T00:00:00Z",
                    "is_download_unlocked":true,"metadata":{"prompt":"fixture lyrics"}
                })
                .to_string()
                .into_bytes(),
                "/api/download/clip/clip-review?format=wav"
                | "/api/download/clip/clip-review?format=mp3" => json!({
                    "download_url":format!("http://{address}/media"),"status":"complete"
                })
                .to_string()
                .into_bytes(),
                "/api/gen/clip-review/opus_file/" => json!({
                    "opus_file_url":format!("http://{address}/media")
                })
                .to_string()
                .into_bytes(),
                "/api/billing/info/" => json!({
                    "credits":100,"total_credits_left":100,"monthly_usage":0,"monthly_limit":2500,
                    "is_active":true,"plan":{"name":"Pro","plan_key":"pro"},
                    "models":[],"period":"month","renews_on":null
                })
                .to_string()
                .into_bytes(),
                "/api/gen/clip-review/aligned_lyrics/v3" => {
                    if status == 200 {
                        b"{\"alignment\":[]}".to_vec()
                    } else if status == 202 {
                        b"not json".to_vec()
                    } else {
                        b"{\"detail\":\"fixture failure\"}".to_vec()
                    }
                }
                "/media" => served.clone(),
                _ => panic!("unexpected route: {path}"),
            };
            paths.push(path);
            write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
            stream.write_all(&bytes).unwrap();
            if done {
                return paths;
            }
        }
    });
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("sunox"));
    command
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .env("SUNOX_TEST_API_BASE_URL", format!("http://{address}"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .args([
            "-c",
            "challenge_browser=existing",
            "download",
            "clip-review",
            "--format",
            format,
            "--force",
            "--output",
        ])
        .arg(&output)
        .arg("--json")
        .timeout(Duration::from_secs(10));
    if alignment_status.is_none() {
        command.arg("--read-only");
    }
    command.arg("--quiet");
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "SUNOX_DEFAULT_MODEL",
        "SUNOX_POLL_INTERVAL_SECS",
        "SUNOX_POLL_TIMEOUT_SECS",
        "SUNOX_SERIAL_MUTATIONS",
    ] {
        command.env_remove(name);
    }
    let assertion = command.assert();
    let result = assertion.get_output();
    let paths = server.join().expect("HTTP fixture");
    assert_eq!(
        paths.len(),
        if alignment_status.is_some() { 5 } else { 3 },
        "{case}: {paths:?}"
    );
    assert_eq!(
        result.status.success(),
        valid,
        "{case}: stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let saved = std::fs::read(&destination).unwrap();
    if let Some(status) = alignment_status {
        assert_ne!(saved, previous);
        let result: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let checkpoints = config.join("operations");
        let remaining = std::fs::read_dir(checkpoints)
            .map(|entries| entries.count())
            .unwrap_or(0);
        let ambiguous = status >= 500 || status == 202;
        assert_eq!(remaining, usize::from(ambiguous), "{case}: {result}");
        if ambiguous {
            let warning = &result["warnings"][0];
            assert_eq!(warning["code"], "ambiguous_mutation");
            assert!(warning["details"]["operation_id"].is_string());
            let recovery = &warning["details"]["operation_recovery"];
            let checkpoint = recovery["checkpoint_path"]
                .as_str()
                .expect("checkpoint path");
            let persisted: serde_json::Value =
                serde_json::from_slice(&std::fs::read(checkpoint).unwrap()).unwrap();
            assert_eq!(persisted["state"], "completed_with_warnings");
            assert_eq!(
                persisted["writes"][0]["state"],
                if status == 202 {
                    "response_received"
                } else {
                    "possibly_sent"
                }
            );
            assert!(!persisted.to_string().contains("fixture lyrics"));
            assert!(
                recovery["inspection_commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_str() == Some("sunox clip info clip-review --json"))
            );
        }
    } else if valid {
        assert_eq!(saved, body, "{case}");
    } else {
        assert_eq!(
            saved, previous,
            "{case}: --force replaced the previous file"
        );
        assert!(
            result.stdout.is_empty(),
            "{case}: invalid file was reported as success"
        );
        let error: serde_json::Value =
            serde_json::from_slice(&result.stderr).expect("structured download error");
        assert_eq!(error["error"]["code"], "download_error", "{case}: {error}");
    }
    assert_eq!(
        std::fs::read_dir(&output).unwrap().count(),
        1,
        "{case}: leaked staging file"
    );
}

#[test]
fn batch_download_stops_on_direct_and_wrapped_account_errors() {
    for (status, poll, switch_account) in [
        (401, false, false),
        (429, false, false),
        (401, true, false),
        (429, true, false),
        (401, true, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config/sunox");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("auth.json"), json!({
            "jwt":"e30.eyJzdWIiOiJ1c2VyLXRlc3QiLCJleHAiOjQxMDI0NDQ4MDB9.c2ln",
            "device_id":"test-device",
            "browser_environment":{"browser_source":"test","user_agent":"test","accept_language":"en"}
        }).to_string()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_stopped = stopped.clone();
        let auth_path = config.join("auth.json");
        if switch_account {
            let mut auth: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&auth_path).unwrap()).unwrap();
            auth["clerk_client_cookie"] = json!("fake-cookie");
            std::fs::write(&auth_path, auth.to_string()).unwrap();
        }
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut paths = Vec::new();
            let mut align_reads = 0;
            while !server_stopped.load(std::sync::atomic::Ordering::SeqCst)
                && Instant::now() < deadline
            {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                let path = request_path(&mut stream, true);
                let mut response_status = 200;
                let body = match path.as_str() {
                    "/api/feed/v3" => json!({"clips": (["clip-first","clip-second"].map(|id| json!({"id":id,"title":id,"status":"complete","model_name":"chirp-hawk","created_at":"2026-09-13T00:00:00Z","is_download_unlocked":true,"metadata":{"prompt":"test lyrics"}})))}).to_string().into_bytes(),
                    "/api/billing/info/" => json!({"credits":100,"total_credits_left":100,"monthly_usage":0,"monthly_limit":100,"is_active":true,"plan":{"name":"Pro","plan_key":"pro"},"models":[],"period":"month"}).to_string().into_bytes(),
                    path if path.starts_with("/api/download/clip/") => json!({"download_url":format!("http://{address}/media"),"status":"complete"}).to_string().into_bytes(),
                    "/media" => include_bytes!("fixtures/download/silence.mp3").to_vec(),
                    "/api/gen/clip-first/aligned_lyrics/v3" => {
                        align_reads += 1;
                        if poll && align_reads == 1 { b"{\"state\":\"running\"}".to_vec() }
                        else {
                            if switch_account {
                                use base64::Engine;
                                let mut auth: serde_json::Value = serde_json::from_slice(&std::fs::read(&auth_path).unwrap()).unwrap();
                                let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!({"sub":"new-account","exp":4102444800_u64}).to_string());
                                auth["jwt"] = json!(format!("e30.{claims}.c2ln"));
                                std::fs::write(&auth_path, auth.to_string()).unwrap();
                            }
                            response_status = status; b"{}".to_vec()
                        }
                    }
                    "/api/gen/clip-second/aligned_lyrics/v3" => b"{\"alignment\":[]}".to_vec(),
                    _ => panic!("unexpected {path}"),
                };
                paths.push(path);
                write!(stream,"HTTP/1.1 {response_status} Response\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
            paths
        });
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("sunox"));
        for (key, _) in std::env::vars()
            .filter(|(key, _)| key.starts_with("SUNOX_") || key.to_lowercase().ends_with("_proxy"))
        {
            command.env_remove(key);
        }
        let output_dir = directory.path().join("output");
        let output = command
            .env("XDG_CONFIG_HOME", directory.path().join("config"))
            .env("SUNOX_TEST_API_BASE_URL", format!("http://{address}"))
            .env("NO_PROXY", "127.0.0.1,localhost")
            .args([
                "-c",
                "challenge_browser=existing",
                "-c",
                "poll_interval_secs=1",
                "download",
                "clip-first",
                "clip-second",
                "--quiet",
                "--json",
                "--output",
            ])
            .arg(&output_dir)
            .timeout(Duration::from_secs(6))
            .assert()
            .get_output()
            .clone();
        stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        let paths = server.join().unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "status={status} poll={poll}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "partial_download");
        let failed = &error["error"]["details"]["failed"];
        let cause = if switch_account {
            "auth_changed"
        } else if status == 401 {
            "auth_expired"
        } else {
            "rate_limited"
        };
        if poll {
            assert_eq!(failed["code"], "ambiguous_mutation");
            assert_eq!(failed["details"]["cause"]["code"], cause);
            let checkpoint = error["error"]["details"]["operation_recovery"]["checkpoint_path"]
                .as_str()
                .unwrap();
            assert!(std::path::Path::new(checkpoint).is_file());
        } else {
            assert_eq!(failed["code"], cause);
        }
        assert_eq!(
            error["error"]["details"]["succeeded"][0]["clip_id"],
            "clip-first"
        );
        assert_eq!(
            error["error"]["details"]["not_attempted_clip_ids"],
            json!(["clip-second"])
        );
        assert_eq!(std::fs::read_dir(output_dir).unwrap().count(), 1);
        assert!(
            !paths.iter().any(|p| p.contains("clip-second")),
            "{paths:?}"
        );
    }
}

#[test]
fn invalid_image_inputs_fail_before_authentication_or_account_locking() {
    let directory = tempfile::tempdir().unwrap();
    let empty = directory.path().join("empty.png");
    std::fs::write(&empty, []).unwrap();
    let folder = directory.path().join("folder.png");
    std::fs::create_dir(&folder).unwrap();
    let oversized = directory.path().join("oversized.png");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let paths = vec![empty, folder, oversized];
    #[cfg(unix)]
    let paths = {
        let mut paths = paths;
        let fifo = directory.path().join("pipe.png");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        paths.push(fifo);
        paths
    };
    for path in paths {
        for args in [
            vec!["clip", "set", "clip-test"],
            vec!["playlist", "create", "Test"],
            vec!["playlist", "set", "playlist-test"],
        ] {
            let mut command = Command::new(assert_cmd::cargo::cargo_bin!("sunox"));
            let output = command
                .env("XDG_CONFIG_HOME", directory.path().join("config"))
                .args(["-c", "challenge_browser=existing"])
                .args(args)
                .arg("--image-file")
                .arg(&path)
                .arg("--json")
                .timeout(Duration::from_secs(3))
                .assert()
                .get_output()
                .clone();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!directory.path().join("config/sunox/locks").exists());
        }
    }
}

#[test]
fn regular_image_preparation_reaches_authentication_without_creating_a_lock() {
    let directory = tempfile::tempdir().unwrap();
    let image = directory.path().join("cover.png");
    std::fs::write(
        &image,
        include_bytes!("../assets/browser-extension/icons/icon-16.png"),
    )
    .unwrap();
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("sunox"));
    let output = command
        .env("XDG_CONFIG_HOME", directory.path().join("config"))
        .args([
            "-c",
            "challenge_browser=existing",
            "clip",
            "set",
            "clip-test",
            "--image-file",
        ])
        .arg(image)
        .arg("--json")
        .timeout(Duration::from_secs(3))
        .assert()
        .get_output()
        .clone();
    assert_eq!(output.status.code(), Some(3));
    assert!(!directory.path().join("config/sunox/locks").exists());
}

#[test]
fn optional_alignment_preserves_only_unresolved_mutation_checkpoints() {
    for status in [500, 202, 403, 200] {
        check_download_with_alignment(
            "optional alignment",
            "mp3",
            include_bytes!("fixtures/download/silence.mp3").to_vec(),
            true,
            Some(status),
        );
    }
}

#[test]
fn truncated_opus_and_rf64_never_replace_existing_files() {
    let opus = include_bytes!("fixtures/download/silence.opus");
    let large = include_bytes!("fixtures/download/silence-large-tags.opus");
    let rf64 = include_bytes!("fixtures/download/silence-rf64.wav");
    // Exact byte cuts from the real-file CLI reproductions: Opus headers
    // end at 136; the large comment's first page ends at 65354; RF64 data
    // starts at 114. None of these cuts contains a complete audio packet.
    for (case, format, body, valid) in [
        ("Opus headers only", "opus", opus[..136].to_vec(), false),
        (
            "Opus incomplete audio page header",
            "opus",
            opus[..140].to_vec(),
            false,
        ),
        (
            "Opus incomplete comment packet",
            "opus",
            large[..65354].to_vec(),
            false,
        ),
        ("RF64 one data byte", "wav", rf64[..115].to_vec(), false),
        ("complete Opus", "opus", opus.to_vec(), true),
        ("complete RF64", "wav", rf64.to_vec(), true),
    ] {
        check_download(case, format, body, valid);
    }
}

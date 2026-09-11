use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fs2::FileExt;
use sha2::{Digest, Sha256};

struct RunningCli(Option<Child>);

impl Drop for RunningCli {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    body: String,
    account_lock_held: bool,
}

#[test]
fn enhanced_generation_keeps_every_write_under_one_account_lock() {
    let cases: &[&[&str]] = &[
        &["create", "--instrumental", "--tags", "ambient piano"],
        &["create", "gentle rain", "--tags", "ambient piano"],
        &[
            "clip",
            "inspire",
            "source-1",
            "--title",
            "New song",
            "--tags",
            "ambient piano",
            "--lyrics",
            "Soft rain falls",
        ],
    ];
    for args in cases {
        assert_enhancement_is_serialized(args);
    }
}

fn assert_enhancement_is_serialized(args: &[&str]) {
    let directory = tempfile::tempdir().expect("isolated config directory");
    let config = directory.path().join("sunox");
    std::fs::create_dir_all(config.join("locks")).expect("lock directory");
    std::fs::write(
        config.join("auth.json"),
        serde_json::to_vec(&serde_json::json!({
            "jwt": "e30.eyJzdWIiOiJ1c2VyLXRlc3QiLCJleHAiOjQxMDI0NDQ4MDB9.c2ln",
            "browser_environment": {
                "browser_source": "generation-lock-test",
                "user_agent": "Mozilla/5.0 Chrome/136.0.0.0",
                "accept_language": "en-US",
                "client_hints": {
                    "sec_ch_ua": "\"Chromium\";v=\"136\"",
                    "sec_ch_ua_mobile": "?0",
                    "sec_ch_ua_platform": "\"macOS\""
                }
            }
        }))
        .expect("auth JSON"),
    )
    .expect("write fake auth");
    let key = Sha256::digest(b"jwt-sub:user-test")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let lock_path = config
        .join("locks")
        .join(format!("mutation-account-{key}.lock"));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("account lock");
    lock.lock_exclusive().expect("hold another command's lock");
    let (base_url, requests) = mock_generation(&lock_path);
    let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin!("sunox"));
    command
        .args(args)
        .args(["--enhance-tags", "--no-captcha", "--json"])
        .env("XDG_CONFIG_HOME", directory.path())
        .env("SUNOX_TEST_API_BASE_URL", base_url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "SUNOX_DEFAULT_MODEL",
        "SUNOX_POLL_INTERVAL_SECS",
        "SUNOX_POLL_TIMEOUT_SECS",
        "SUNOX_OUTPUT_DIR",
        "SUNOX_SERIAL_MUTATIONS",
        "SUNOX_CHALLENGE_BROWSER",
    ] {
        command.env_remove(name);
    }
    let mut child = RunningCli(Some(command.spawn().expect("start CLI")));
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut observed = Vec::new();
    while let Ok(request) =
        requests.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        assert_ne!(
            request.path, "/api/prompts/upsample",
            "enhancement was submitted while another command held the account lock: {args:?}"
        );
        observed.push(request);
    }
    FileExt::unlock(&lock).expect("allow the CLI to continue");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child
            .0
            .as_mut()
            .expect("running child")
            .try_wait()
            .expect("child status")
            .is_some()
        {
            break;
        }
        assert!(Instant::now() < deadline, "generation deadlocked: {args:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child
        .0
        .take()
        .expect("completed child")
        .wait_with_output()
        .expect("CLI output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    observed.extend(requests.try_iter());
    for path in [
        "/api/prompts/upsample",
        "/api/c/check",
        "/api/generate/v2-web/",
    ] {
        let matches = observed
            .iter()
            .filter(|request| request.path == path)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "expected one {path}: {observed:?}");
        assert!(
            matches[0].account_lock_held,
            "account guard was released before {path}"
        );
    }
    let generation = observed
        .iter()
        .find(|request| request.path == "/api/generate/v2-web/")
        .expect("generation");
    let body: serde_json::Value = serde_json::from_str(&generation.body).expect("generation JSON");
    assert_eq!(body["tags"], "enhanced ambient piano");
    assert_eq!(
        body["metadata"]["last_tags_generation"]["request_id"],
        "enhancement-1"
    );
}

fn mock_generation(lock_path: &Path) -> (String, Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let base_url = format!("http://{}", listener.local_addr().expect("server address"));
    let lock_path = lock_path.to_path_buf();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("accept: {error}"),
            };
            // macOS inherits the listener's nonblocking mode on accept.
            stream
                .set_nonblocking(false)
                .expect("blocking accepted stream");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let (path, body) = read_request(&mut stream);
            let probe = File::options()
                .read(true)
                .write(true)
                .open(&lock_path)
                .expect("lock probe");
            let account_lock_held = match probe.try_lock_exclusive() {
                Ok(()) => {
                    FileExt::unlock(&probe).expect("unlock probe");
                    false
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || cfg!(windows) && error.raw_os_error() == Some(33) =>
                {
                    // LockFileEx reports ERROR_LOCK_VIOLATION for a conflicting
                    // byte-range lock instead of consistently mapping it to WouldBlock.
                    true
                }
                Err(error) => panic!("probe account lock: {error}"),
            };
            let response = match path.as_str() {
                "/api/billing/info/" => serde_json::json!({
                    "credits":1000,"total_credits_left":1000,"monthly_usage":0,"monthly_limit":2500,"is_active":true,
                    "plan":{"id":"tier-pro","name":"Pro","plan_key":"pro","usage_plan_features":[]},
                    "models":[{"name":"v6","external_key":"chirp-hawk","can_use":true,"is_default_model":true,"description":"test model","capabilities":["all"],"features":["create_control_sliders","tag_upsample"],"badges":["pro"],"max_lengths":{}}],
                    "period":"month","renews_on":null
                }),
                "/api/session/" => serde_json::json!({
                    "flags":{"aug-creativity":true},"roles":{}
                }),
                "/api/personalization/settings" => serde_json::json!({"styles_augmentation":true}),
                "/api/prompts/upsample" => {
                    serde_json::json!({"upsampled":"enhanced ambient piano","request_id":"enhancement-1"})
                }
                "/api/c/check" => serde_json::json!({"required":false}),
                "/api/generate/v2-web/" => {
                    serde_json::json!({"clips":[{"id":"clip-1","title":"New song","status":"submitted","model_name":"chirp-hawk","created_at":"2026-09-08T00:00:00Z"}]})
                }
                unexpected => panic!("unexpected route {unexpected}"),
            };
            let done = path == "/api/generate/v2-web/";
            if sender
                .send(CapturedRequest {
                    path,
                    body,
                    account_lock_held,
                })
                .is_err()
            {
                break;
            }
            let response = response.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", response.len(), response).expect("response");
            if done {
                break;
            }
        }
    });
    (base_url, receiver)
}

fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).expect("read headers");
        assert!(count > 0, "request closed before headers");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let path = headers
        .lines()
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("request path")
        .to_string();
    let length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let count = stream.read(&mut chunk).expect("read body");
        assert!(count > 0, "request closed before body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    (
        path,
        String::from_utf8(bytes[header_end..header_end + length].to_vec()).expect("UTF-8 body"),
    )
}

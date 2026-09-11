use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Output;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use base64::Engine;
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SUBJECT: &str = "mutation-auth-user";

#[derive(Clone, Copy)]
enum Scenario {
    RejectedInitialAuth,
    RejectedWrite,
    LongGeneration,
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    authorization: String,
    body: Value,
    elapsed: Duration,
    account_lock_held: bool,
}

fn token(marker: &str) -> String {
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(json!({"sub":SUBJECT,"exp":4102444800_u64,"marker":marker}).to_string());
    format!("e30.{claims}.c2ln")
}

fn write_auth(path: &Path, marker: &str) {
    std::fs::write(
        path,
        json!({
            "jwt": token(marker),
            "session_id": "fake-session",
            "clerk_client_cookie": "fake-cookie",
            "device_id": "fake-device",
            "browser_environment": {
                "browser_source": "cli-mutation-auth-test",
                "user_agent": "Mozilla/5.0 Chrome/136.0.0.0",
                "accept_language": "en-US",
                "client_hints": {
                    "sec_ch_ua":"\"Chromium\";v=\"136\"",
                    "sec_ch_ua_mobile":"?0",
                    "sec_ch_ua_platform":"\"macOS\""
                }
            }
        })
        .to_string(),
    )
    .expect("isolated fake auth");
}

fn account_lock_held(path: &Path) -> bool {
    let Ok(probe) = File::options().read(true).write(true).open(path) else {
        return false;
    };
    match probe.try_lock_exclusive() {
        Ok(()) => {
            FileExt::unlock(&probe).expect("unlock probe");
            false
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || cfg!(windows) && error.raw_os_error() == Some(33) =>
        {
            // LockFileEx reports ERROR_LOCK_VIOLATION for a conflicting byte-range
            // lock, which Rust does not consistently map to WouldBlock.
            true
        }
        Err(error) => panic!("probe account lock: {error}"),
    }
}

fn read_request(stream: &mut TcpStream, start: Instant, lock_path: &Path) -> Request {
    stream.set_nonblocking(false).expect("blocking socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read deadline");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let boundary = loop {
        let count = stream.read(&mut chunk).expect("request bytes");
        assert!(count > 0, "request closed before headers");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..boundary]).to_string();
    let field = |name: &str| {
        headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let length = field("content-length")
        .map(|value| value.parse::<usize>().expect("content length"))
        .unwrap_or(0);
    while bytes.len() < boundary + length {
        let count = stream.read(&mut chunk).expect("request body");
        assert!(count > 0, "request closed before body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    let mut line = headers
        .lines()
        .next()
        .expect("request line")
        .split_whitespace();
    Request {
        method: line.next().expect("method").to_string(),
        path: line.next().expect("path").to_string(),
        authorization: field("authorization").expect("authorization"),
        body: serde_json::from_slice(&bytes[boundary..boundary + length]).unwrap_or(Value::Null),
        elapsed: start.elapsed(),
        account_lock_held: account_lock_held(lock_path),
    }
}

fn response_for(path: &str) -> Value {
    let clip = json!({
        "id":"edited-clip","title":"Edited song","status":"complete",
        "model_name":"chirp-hawk","created_at":"2026-09-08T00:00:00Z"
    });
    match path {
        "/api/billing/info/" => json!({
            "credits":1000,"total_credits_left":1000,"monthly_usage":0,"monthly_limit":2500,
            "is_active":true,"plan":{"id":"tier-pro","name":"Pro","plan_key":"pro","usage_plan_features":[]},
            "models":[{"name":"v6","external_key":"chirp-hawk","can_use":true,
                "is_default_model":true,"description":"test model","capabilities":["all"],
                "features":["create_control_sliders","tag_upsample"],"badges":["pro"],"max_lengths":{}}],
            "period":"month","renews_on":null
        }),
        "/api/session/" => json!({"flags":{"aug-creativity":true},"roles":{}}),
        "/api/personalization/settings" => json!({"styles_augmentation":true}),
        "/api/prompts/upsample" => {
            json!({"upsampled":"enhanced ambient piano","request_id":"enhancement-auth"})
        }
        "/api/c/check" => json!({"required":false}),
        "/api/generate/v2-web/" => json!({"clips":[clip]}),
        "/api/clips/reverse-clip/" | "/api/clips/adjust-speed/" | "/api/clip/edited-clip" => clip,
        "/api/edit/crop/source-clip/" | "/api/edit/fade/source-clip/" => {
            json!({"action_clip_id":"edited-clip"})
        }
        "/api/edit/action/edited-clip/" => json!({"status":"complete"}),
        _ => panic!("unexpected route {path}"),
    }
}

fn run_case(args: &[&str], mutation_path: &str, scenario: Scenario) -> (Output, Vec<Request>) {
    let home = tempfile::tempdir().expect("isolated home");
    let config = home.path().join(".config/sunox");
    std::fs::create_dir_all(&config).expect("config directory");
    let auth_path = config.join("auth.json");
    write_auth(&auth_path, "initial");
    let account_hash = Sha256::digest(format!("jwt-sub:{SUBJECT}").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let lock_path = config
        .join("locks")
        .join(format!("mutation-account-{account_hash}.lock"));
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server");
    listener.set_nonblocking(true).expect("nonblocking server");
    let address = listener.local_addr().expect("server address");
    let stopped = Arc::new(AtomicBool::new(false));
    let server_stopped = Arc::clone(&stopped);
    let mutation_path = mutation_path.to_string();
    let server = std::thread::spawn(move || {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(50);
        let mut requests = Vec::new();
        let mut rotated = !matches!(scenario, Scenario::LongGeneration);
        while !server_stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("mock accept: {error}"),
            };
            let request = read_request(&mut stream, start, &lock_path);
            let initial_auth = request.authorization == format!("Bearer {}", token("initial"));
            let rejected_write =
                matches!(scenario, Scenario::RejectedWrite) && request.path == mutation_path;
            let (status, body) = if rotated && initial_auth {
                // A concurrent process refreshed this same account. Only the read may retry.
                write_auth(&auth_path, "fresh");
                (
                    "401 Unauthorized",
                    json!({"detail":"Token validation failed."}),
                )
            } else if rejected_write {
                // Keep even an erroneous replay inside the loopback fixture: any attempted
                // refresh can adopt a newer fake JWT instead of contacting Clerk.
                write_auth(&auth_path, &format!("rejected-write-{}", requests.len()));
                (
                    "401 Unauthorized",
                    json!({"detail":"Token validation failed."}),
                )
            } else {
                if matches!(scenario, Scenario::LongGeneration)
                    && matches!(
                        request.path.as_str(),
                        "/api/personalization/settings" | "/api/c/check"
                    )
                {
                    // Each API response stays below its 30-second timeout, but preparation
                    // as a whole exceeds the production 30-second auth freshness budget.
                    std::thread::sleep(Duration::from_secs(16));
                    if request.path == "/api/c/check" {
                        write_auth(&auth_path, "fresh");
                        rotated = true;
                    }
                }
                ("200 OK", response_for(&request.path))
            };
            requests.push(request);
            let body = body.to_string();
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
        }
        requests
    });
    let mut command = Command::cargo_bin("sunox").expect("binary");
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("SUNOX_")) {
        command.env_remove(key);
    }
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("SUNOX_TEST_API_BASE_URL", format!("http://{address}"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .args(["-c", "challenge_browser=existing"])
        .args(args)
        .arg("--json")
        .timeout(Duration::from_secs(45));
    let output = command.assert().get_output().clone();
    stopped.store(true, Ordering::SeqCst);
    (output, server.join().expect("mock server"))
}

#[test]
fn direct_clip_edits_refresh_rejected_auth_before_the_first_write_under_the_account_lock() {
    for (args, path) in [
        (
            vec!["clip", "reverse", "source-clip", "--title", "Edited song"],
            "/api/clips/reverse-clip/",
        ),
        (
            vec![
                "clip",
                "speed",
                "source-clip",
                "--multiplier",
                "1.2",
                "--title",
                "Edited song",
            ],
            "/api/clips/adjust-speed/",
        ),
        (
            vec![
                "clip",
                "crop",
                "source-clip",
                "--start",
                "1",
                "--end",
                "2",
                "--title",
                "Edited song",
            ],
            "/api/edit/crop/source-clip/",
        ),
        (
            vec![
                "clip",
                "fade",
                "source-clip",
                "--in",
                "1",
                "--title",
                "Edited song",
            ],
            "/api/edit/fade/source-clip/",
        ),
    ] {
        let (output, requests) = run_case(&args, path, Scenario::RejectedInitialAuth);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(requests[0].path, "/api/billing/info/");
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[1].path, "/api/billing/info/");
        assert_eq!(
            requests[1].authorization,
            format!("Bearer {}", token("fresh"))
        );
        let writes: Vec<_> = requests
            .iter()
            .filter(|request| request.path == path)
            .collect();
        assert_eq!(writes.len(), 1, "{requests:?}");
        assert_eq!(writes[0].method, "POST");
        assert_eq!(
            writes[0].authorization,
            format!("Bearer {}", token("fresh"))
        );
        assert_eq!(writes[0].body["title"], "Edited song");
        assert!(
            requests.iter().all(|request| request.account_lock_held),
            "preflight/write escaped account guard: {requests:?}"
        );
    }
}

#[test]
fn an_auth_rejection_of_the_write_is_never_replayed_after_preflight() {
    let path = "/api/clips/reverse-clip/";
    let (output, requests) = run_case(
        &["clip", "reverse", "source-clip", "--title", "Edited song"],
        path,
        Scenario::RejectedWrite,
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == path)
            .count(),
        1,
        "{requests:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("auth_expired"));
    let error: serde_json::Value =
        serde_json::from_slice(&output.stderr).expect("structured auth error");
    assert!(
        error["error"]["details"]["operation_recovery"].is_null(),
        "a definite auth rejection must not claim that the write was possibly accepted"
    );
}

#[test]
fn generation_revalidates_auth_after_preparation_exceeds_thirty_seconds() {
    let (output, requests) = run_case(
        &[
            "create",
            "--instrumental",
            "--tags",
            "ambient piano",
            "--enhance-tags",
            "--no-captcha",
        ],
        "/api/generate/v2-web/",
        Scenario::LongGeneration,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let later_billing: Vec<_> = requests
        .iter()
        .filter(|request| {
            request.path == "/api/billing/info/" && request.elapsed >= Duration::from_secs(30)
        })
        .collect();
    assert_eq!(later_billing.len(), 2, "{requests:?}");
    assert_eq!(
        later_billing[0].authorization,
        format!("Bearer {}", token("initial"))
    );
    assert_eq!(
        later_billing[1].authorization,
        format!("Bearer {}", token("fresh"))
    );
    let submissions: Vec<_> = requests
        .iter()
        .filter(|request| request.path == "/api/generate/v2-web/")
        .collect();
    assert_eq!(submissions.len(), 1, "{requests:?}");
    assert_eq!(
        submissions[0].authorization,
        format!("Bearer {}", token("fresh"))
    );
    assert_eq!(submissions[0].body["tags"], "enhanced ambient piano");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == "/api/prompts/upsample")
            .count(),
        1
    );
    assert!(
        requests.iter().all(|request| request.account_lock_held),
        "preparation escaped account guard: {requests:?}"
    );
}

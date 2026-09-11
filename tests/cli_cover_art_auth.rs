use assert_cmd::Command;
use base64::Engine;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    authorization: String,
    body: String,
}

fn write_auth(path: &Path, marker: &str) {
    let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(json!({"sub":"user-test","exp":4102444800_u64,"marker":marker}).to_string());
    std::fs::write(
        path,
        json!({
            "jwt": format!("e30.{claims}.c2ln"),
            "session_id": "fake-session",
            "clerk_client_cookie": "fake-cookie",
            "device_id": "fake-device",
            "browser_environment": {
                "browser_source": "cli-http-test",
                "user_agent": "Mozilla/5.0 Chrome/136.0.0.0",
                "accept_language": "en-US",
                "client_hints": {
                    "sec_ch_ua": "\"Chromium\";v=\"136\"",
                    "sec_ch_ua_mobile": "?0",
                    "sec_ch_ua_platform": "\"macOS\""
                }
            }
        })
        .to_string(),
    )
    .expect("write isolated fake auth");
}

fn read_request(stream: &mut TcpStream) -> Request {
    // On macOS an accepted socket inherits the listener's nonblocking mode.
    // The polling accept loop must not turn a valid slow arrival into WouldBlock.
    stream
        .set_nonblocking(false)
        .expect("blocking accepted stream");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).expect("request bytes");
        assert!(count > 0, "connection closed before headers");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let field = |name: &str| {
        headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let length = field("content-length")
        .map(|n| n.parse::<usize>().expect("body size"))
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let count = stream.read(&mut chunk).expect("request body");
        assert!(count > 0, "connection closed before body");
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
        body: String::from_utf8(bytes[header_end..header_end + length].to_vec())
            .expect("JSON body"),
    }
}

fn response_for(path: &str) -> Value {
    match path {
        "/api/billing/info/" => json!({
            "credits":100,"total_credits_left":100,"monthly_usage":0,"monthly_limit":2500,
            "is_active":true,"plan":{"name":"Pro","plan_key":"pro","id":"tier-pro"},
            "models":[],"period":"month","renews_on":null,
            "accessible_features":["generate_song_image","generate_song_video"]
        }),
        "/api/clip/clip-test" => json!({
            "id":"clip-test","title":"Test","status":"complete","model_name":"chirp-fenix",
            "created_at":"2026-08-30T00:00:00Z","user_id":"user-test","is_trashed":false,
            "action_config":{"actions":[{"action_type":"generate_cover_art","visible":true,"disabled":false}]}
        }),
        "/api/video_gen/model-configs" => json!({
            "image_model_categories":[{"category":"image-v1"}],
            "video_model_categories":[{"category":"video-v1","allowed_durations":[5]}]
        }),
        "/api/video_gen/cost/image" | "/api/video_gen/cost/video" => {
            json!({"cost":8,"remaining_gens":4})
        }
        "/api/video_gen/image/generate" => {
            json!({"batch_id":"batch-test","image_ids":["image-1","image-2"]})
        }
        "/api/video_gen/video/generate" => {
            json!({"batch_id":"batch-test","video_ids":["video-1","video-2"]})
        }
        "/api/video_gen/pending_batches" => json!({"batch_ids":[]}),
        "/api/video_gen/history" => json!({"history":[]}),
        "/api/video_gen/poll_batches" => json!({"batches":{"batch-test":[
            {"id":"image-1","type":"image","status":"complete","url":"https://example.invalid/image-1.jpg"},
            {"id":"image-2","type":"image","status":"complete","url":"https://example.invalid/image-2.jpg"}
        ]}}),
        _ => panic!("unexpected test route {path}"),
    }
}

fn run_case(args: &[&str], rejected_paths: &[&str]) -> (std::process::Output, Vec<Request>) {
    let home = tempfile::tempdir().expect("isolated home");
    let config = home.path().join(".config/sunox");
    std::fs::create_dir_all(&config).expect("config directory");
    let auth_path = config.join("auth.json");
    write_auth(&auth_path, "initial");
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server");
    listener.set_nonblocking(true).expect("nonblocking server");
    let address = listener.local_addr().expect("server address");
    let stopped = Arc::new(AtomicBool::new(false));
    let server_stopped = Arc::clone(&stopped);
    let rejected_paths: Vec<String> = rejected_paths.iter().map(|s| s.to_string()).collect();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let mut counts = HashMap::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !server_stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("mock accept: {error}"),
            };
            let request = read_request(&mut stream);
            let count = counts.entry(request.path.clone()).or_insert(0);
            *count += 1;
            let reject = *count == 1 && rejected_paths.contains(&request.path);
            let (status, body) = if reject {
                // Another process refreshed this same account. Auth retry must reuse it
                // without contacting Clerk or replaying a paid generation mutation.
                write_auth(&auth_path, &request.path);
                (
                    "401 Unauthorized",
                    json!({"detail":"Token validation failed."}),
                )
            } else {
                ("200 OK", response_for(&request.path))
            };
            requests.push(request);
            let body = body.to_string();
            let wire = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(wire.as_bytes()).expect("mock response");
        }
        requests
    });
    let mut command = Command::cargo_bin("sunox").expect("binary");
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("SUNOX_TEST_API_BASE_URL", format!("http://{address}"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env_remove("SUNOX_DEFAULT_MODEL")
        .env_remove("SUNOX_POLL_INTERVAL_SECS")
        .env_remove("SUNOX_POLL_TIMEOUT_SECS")
        .env_remove("SUNOX_CHALLENGE_BROWSER")
        .args(args)
        .arg("--json")
        .timeout(Duration::from_secs(10));
    let assertion = command.assert();
    let output = assertion.get_output().clone();
    stopped.store(true, Ordering::SeqCst);
    (output, server.join().expect("mock server"))
}

#[test]
fn cover_art_read_posts_retry_once_with_saved_auth_and_the_same_body() {
    for (args, path) in [
        (
            vec![
                "clip",
                "cover-art",
                "status",
                "batch-test",
                "--media",
                "image",
            ],
            "/api/video_gen/poll_batches",
        ),
        (
            vec![
                "clip",
                "cover-art",
                "status",
                "batch-test",
                "--media",
                "image",
                "--wait",
                "--timeout",
                "3",
            ],
            "/api/video_gen/poll_batches",
        ),
        (
            vec!["clip", "cover-art", "pending"],
            "/api/video_gen/pending_batches",
        ),
        (
            vec!["clip", "cover-art", "history", "--clip-id", "clip-test"],
            "/api/video_gen/history",
        ),
    ] {
        let (output, requests) = run_case(&args, &[path]);
        assert!(
            output.status.success(),
            "args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, path);
        assert_eq!(requests[1].path, path);
        assert_eq!(requests[0].body, requests[1].body);
        assert_ne!(requests[0].authorization, requests[1].authorization);
    }
}

#[test]
fn cover_art_costs_refresh_auth_but_paid_submissions_are_never_replayed() {
    for media in ["image", "video"] {
        let cost = format!("/api/video_gen/cost/{media}");
        let mutation = format!("/api/video_gen/{media}/generate");
        let (output, requests) = run_case(
            &[
                "clip",
                "cover-art",
                media,
                "clip-test",
                "--prompt",
                "blue sky",
                "--no-wait",
            ],
            &[&cost, &mutation],
        );
        assert_eq!(
            output.status.code(),
            Some(3),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let costs: Vec<_> = requests
            .iter()
            .filter(|request| request.path == cost)
            .collect();
        assert_eq!(costs.len(), 2, "{requests:?}");
        assert_eq!(costs[0].body, costs[1].body);
        assert_ne!(costs[0].authorization, costs[1].authorization);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == mutation)
                .count(),
            1
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("auth_expired"));
    }
}

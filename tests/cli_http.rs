use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

fn isolated_test_home(prefix: &str) -> PathBuf {
    let test_home = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&test_home).expect("test home");
    test_home
}

fn write_auth(test_home: &Path) {
    let config_dir = test_home.join(".config").join("sunox");
    std::fs::create_dir_all(&config_dir).expect("auth directory");
    std::fs::write(
        config_dir.join("auth.json"),
        serde_json::to_vec(&serde_json::json!({
            "jwt": "e30.eyJzdWIiOiJ1c2VyLXRlc3QiLCJleHAiOjQxMDI0NDQ4MDB9.c2ln",
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
        }))
        .expect("auth json"),
    )
    .expect("write auth");
}

fn isolated_command(test_home: &Path, base_url: &str) -> Command {
    let mut command = Command::cargo_bin("sunox").expect("binary");
    command
        .env("HOME", test_home)
        .env("USERPROFILE", test_home)
        .env("APPDATA", test_home.join("AppData").join("Roaming"))
        .env("LOCALAPPDATA", test_home.join("AppData").join("Local"))
        .env("XDG_CONFIG_HOME", test_home.join(".config"))
        .env("XDG_DATA_HOME", test_home.join(".local").join("share"))
        .env("SUNOX_TEST_API_BASE_URL", base_url)
        .env_remove("SUNOX_DEFAULT_MODEL")
        .env_remove("SUNOX_POLL_INTERVAL_SECS")
        .env_remove("SUNOX_POLL_TIMEOUT_SECS")
        .env_remove("SUNOX_OUTPUT_DIR")
        .env_remove("SUNOX_SERIAL_MUTATIONS")
        .env_remove("SUNOX_CHALLENGE_BROWSER");
    command
}

fn serve_json_sequence(responses: Vec<String>) -> (String, Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let address = listener.local_addr().expect("mock address");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "mock request deadline");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept mock request: {error}"),
                }
            };
            // Darwin can inherit O_NONBLOCK from the listener; receive deadlines need blocking IO.
            stream
                .set_nonblocking(false)
                .expect("blocking accepted connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("request timeout");
            let request = read_request(&mut stream);
            sender.send(request).expect("capture request");
            let wire = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            );
            stream.write_all(wire.as_bytes()).expect("mock response");
        }
    });
    (format!("http://{address}"), receiver)
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk).expect("read request");
        assert!(count > 0, "request closed before headers");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk).expect("read request body");
        assert!(count > 0, "request closed before body");
        bytes.extend_from_slice(&chunk[..count]);
    }
    let request_line = headers.lines().next().expect("request line");
    let mut parts = request_line.split_whitespace();
    CapturedRequest {
        method: parts.next().expect("request method").to_string(),
        path: parts.next().expect("request path").to_string(),
        body: String::from_utf8(bytes[header_end..header_end + content_length].to_vec())
            .expect("request body utf8"),
    }
}

fn billing_fixture() -> String {
    serde_json::json!({
        "credits": 100,
        "total_credits_left": 100,
        "monthly_usage": 0,
        "monthly_limit": 2500,
        "is_active": true,
        "plan": {
            "id": "tier-pro",
            "name": "Pro Plan",
            "plan_key": "pro",
            "usage_plan_features": []
        },
        "models": [],
        "period": "month",
        "renews_on": null,
        "remaster_model_types": []
    })
    .to_string()
}

#[test]
fn clip_list_dispatches_through_the_public_cli_to_feed_v3() {
    let home = isolated_test_home("sunox-cli-http-read");
    write_auth(&home);
    let feed = serde_json::json!({
        "clips": [{
            "id": "clip-read-1",
            "title": "CLI contract",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-08-30T00:00:00Z"
        }],
        "next_cursor": null,
        "has_more": false
    })
    .to_string();
    let (base_url, requests) = serve_json_sequence(vec![feed]);

    let assertion = isolated_command(&home, &base_url)
        .args(["clip", "list", "--limit", "1", "--json"])
        .assert()
        .success();
    let output: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("CLI json");
    assert_eq!(output["data"]["clips"][0]["id"], "clip-read-1");

    let request = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("feed request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/feed/v3");
    let body: serde_json::Value = serde_json::from_str(&request.body).expect("feed request json");
    assert_eq!(body["limit"], 1);
    assert_eq!(body["filters"]["workspace"]["presence"], "True");
}

#[test]
fn clip_visibility_dispatches_auth_preflight_and_one_write_from_public_cli() {
    let home = isolated_test_home("sunox-cli-http-write");
    write_auth(&home);
    let (base_url, requests) = serve_json_sequence(vec![billing_fixture(), "{}".to_string()]);

    let assertion = isolated_command(&home, &base_url)
        .args(["clip", "publish", "clip-write-1", "--private", "--json"])
        .assert()
        .success();
    let output: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("CLI json");
    assert_eq!(
        output["data"]["clip_ids"],
        serde_json::json!(["clip-write-1"])
    );
    assert_eq!(output["data"]["is_public"], false);

    let preflight = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("billing preflight");
    assert_eq!(preflight.method, "GET");
    assert_eq!(preflight.path, "/api/billing/info/");
    let mutation = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("visibility write");
    assert_eq!(mutation.method, "POST");
    assert_eq!(mutation.path, "/api/gen/clip-write-1/set_visibility/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&mutation.body).expect("mutation json"),
        serde_json::json!({"is_public": false, "submit_to_contest": false})
    );
    assert!(requests.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn playlist_add_partial_failure_is_nonzero_and_preserves_recovery_journal() {
    let test_home = isolated_test_home("sunox-playlist-add-partial");
    write_auth(&test_home);
    let (base_url, requests) = serve_json_sequence(vec![billing_fixture(),
        serde_json::json!({"successes":["clip-a"],"failures":[{"id":"clip-b","code":"not_allowed"}]}).to_string()]);
    let assertion = isolated_command(&test_home, &base_url)
        .args([
            "--json",
            "playlist",
            "add",
            "playlist-a",
            "clip-a",
            "clip-b",
        ])
        .assert()
        .failure();
    let error: serde_json::Value = serde_json::from_slice(&assertion.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "partial_mutation");
    assert_eq!(
        error["error"]["details"]["succeeded_clip_ids"],
        serde_json::json!(["clip-a"])
    );
    assert_eq!(error["error"]["details"]["failed"][0]["clip_id"], "clip-b");
    let operations = test_home.join(".config/sunox/operations");
    assert_eq!(std::fs::read_dir(operations).unwrap().count(), 1);
    let captured: Vec<_> = requests.iter().collect();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[1].path, "/api/playlist/v2/playlist-a/tracks/add");
}

#[test]
fn reuse_mumble_checks_the_session_gate_before_any_generation() {
    let test_home = isolated_test_home("sunox-reuse-mumble-gate");
    write_auth(&test_home);
    let mut billing: serde_json::Value = serde_json::from_str(&billing_fixture()).unwrap();
    billing["models"] = serde_json::json!([{
        "name":"v6","external_key":"chirp-hawk","can_use":true,"is_default_model":true,
        "description":"fixture",
        "capabilities":["all"],"features":["reuse_styles_lyrics","mumble_mode","create_control_sliders"],
        "max_lengths":{},"badges":["pro"]
    }]);
    let source = serde_json::json!({"id":"mumble-source","title":"Mumble","status":"complete",
        "model_name":"chirp-hawk","created_at":"2026-09-30T00:00:00Z",
        "metadata":{"prompt":"","tags":"ambient","is_mumble":true,"make_instrumental":false}});
    let (base_url, requests) = serve_json_sequence(vec![
        billing.to_string(),
        source.to_string(),
        billing.to_string(),
        serde_json::json!({"flags":{"aug-creativity":true}}).to_string(),
    ]);
    let assertion = isolated_command(&test_home, &base_url)
        .args([
            "--json",
            "--quiet",
            "clip",
            "reuse",
            "mumble-source",
            "--no-captcha",
        ])
        .assert()
        .failure();
    let error: serde_json::Value = serde_json::from_slice(&assertion.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "config_error", "{error}");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("mumble-mode")
    );
    let captured: Vec<_> = requests.iter().collect();
    assert_eq!(captured.len(), 4);
    assert!(captured.iter().all(|r| r.method == "GET"));
}

#[test]
fn retired_opus_download_rejects_before_account_or_authorization_requests() {
    let test_home = isolated_test_home("sunox-retired-opus");
    write_auth(&test_home);
    let mut command = isolated_command(&test_home, "http://127.0.0.1:1");
    let output = command
        .args(["--json", "clip", "download", "clip-a", "--format", "opus"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported_download_format"));
    std::fs::remove_dir_all(test_home).unwrap();
}

#[test]
fn external_hcaptcha_token_survives_turnstile_preflight_in_real_cli_submission() {
    let test_home = isolated_test_home("sunox-external-provider");
    write_auth(&test_home);
    let mut billing: serde_json::Value = serde_json::from_str(&billing_fixture()).unwrap();
    billing["models"] = serde_json::json!([{
        "name":"v6","external_key":"chirp-hawk","can_use":true,"is_default_model":true,
        "description":"fixture","capabilities":["all"],"features":[],
        "max_lengths":{},"badges":["pro"]
    }]);
    let generated = serde_json::json!({"clips":[{
        "id":"created-fixture","status":"submitted","title":"test",
        "model_name":"chirp-hawk","created_at":"2026-10-02T00:00:00Z"
    }]});
    let (base_url, requests) = serve_json_sequence(vec![
        billing.to_string(),
        billing.to_string(),
        serde_json::json!({"required":true,"captcha_version":2}).to_string(),
        generated.to_string(),
    ]);
    isolated_command(&test_home, &base_url)
        .args([
            "--json",
            "--quiet",
            "create",
            "short piano piece",
            "--token",
            "external-hcaptcha-fixture",
            "--token-provider",
            "hcaptcha",
        ])
        .assert()
        .success();
    let captured: Vec<_> = requests.iter().collect();
    assert_eq!(captured.len(), 4);
    assert_eq!(captured[2].path, "/api/c/check");
    assert_eq!(captured[3].method, "POST");
    assert_eq!(captured[3].path, "/api/generate/v2-web/");
    let body: serde_json::Value = serde_json::from_str(&captured[3].body).unwrap();
    assert_eq!(body["token_provider"], 1);
    assert_eq!(body["token"], "external-hcaptcha-fixture");
}

#[test]
fn ambiguous_external_token_never_reaches_generation_post() {
    let test_home = isolated_test_home("sunox-ambiguous-provider");
    write_auth(&test_home);
    let mut billing: serde_json::Value = serde_json::from_str(&billing_fixture()).unwrap();
    billing["models"] = serde_json::json!([{
        "name":"v6","external_key":"chirp-hawk","can_use":true,"is_default_model":true,
        "description":"fixture","capabilities":["all"],"features":[],"max_lengths":{},"badges":["pro"]
    }]);
    let (base_url, requests) = serve_json_sequence(vec![
        billing.to_string(),
        billing.to_string(),
        serde_json::json!({"required":true,"captcha_version":2}).to_string(),
    ]);
    let assertion = isolated_command(&test_home, &base_url)
        .args([
            "--json",
            "--quiet",
            "create",
            "short piano piece",
            "--token",
            "ambiguous-fixture",
        ])
        .assert()
        .failure();
    let error: serde_json::Value = serde_json::from_slice(&assertion.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "config_error");
    let captured: Vec<_> = requests.iter().collect();
    assert_eq!(captured.len(), 3);
    assert!(captured.iter().all(|r| r.path != "/api/generate/v2-web/"));
}

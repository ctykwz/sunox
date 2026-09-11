//! The CLI must not replace an existing file with a truncated HTTP 200 body.
use assert_cmd::Command;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn request_path(stream: &mut TcpStream) -> String {
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
    let request = String::from_utf8(request).unwrap();
    let mut line = request.lines().next().unwrap().split_whitespace();
    assert_eq!(line.next(), Some("GET"), "download must remain read-only");
    line.next().unwrap().to_string()
}

fn check_download(case: &str, format: &str, body: Vec<u8>, valid: bool) {
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
            let path = request_path(&mut stream);
            let done = path == "/media";
            let bytes = match path.as_str() {
                "/api/clip/clip-review" => json!({
                    "id":"clip-review","title":"Track","status":"complete",
                    "model_name":"chirp-fenix","created_at":"2026-08-30T00:00:00Z",
                    "is_download_unlocked":true
                })
                .to_string()
                .into_bytes(),
                "/api/download/clip/clip-review?format=wav" => json!({
                    "download_url":format!("http://{address}/media"),"status":"complete"
                })
                .to_string()
                .into_bytes(),
                "/api/gen/clip-review/opus_file/" => json!({
                    "opus_file_url":format!("http://{address}/media")
                })
                .to_string()
                .into_bytes(),
                "/media" => served.clone(),
                _ => panic!("unexpected route: {path}"),
            };
            paths.push(path);
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
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
            "--read-only",
            "--format",
            format,
            "--force",
            "--output",
        ])
        .arg(&output)
        .arg("--json")
        .timeout(Duration::from_secs(10));
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
    assert_eq!(paths.len(), 3, "{case}: {paths:?}");
    assert_eq!(
        result.status.success(),
        valid,
        "{case}: stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let saved = std::fs::read(&destination).unwrap();
    if valid {
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

//! Browser launch policy through the compiled CLI, with no real browser or account.
#![cfg(unix)]

use assert_cmd::Command;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

#[test]
fn authentication_metadata_recovery_respects_browser_launch_policy() {
    for (mode, recorded_bridge, expect_probe) in [
        ("existing", false, false),
        ("auto", true, false),
        ("auto", false, true),
        ("isolated", true, true),
    ] {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let config_dir = temp.path().join("config").join("sunox");
        std::fs::create_dir_all(&config_dir).expect("config directory");
        std::fs::write(
            config_dir.join("auth.json"),
            serde_json::to_vec(&serde_json::json!({
                "jwt": "e30.eyJzdWIiOiJ1c2VyLXRlc3QiLCJleHAiOjQxMDI0NDQ4MDB9.c2ln",
                "device_id": "fake-device",
                "browser_environment": {"browser_source": "chrome"}
            }))
            .unwrap(),
        )
        .expect("fake auth");
        if recorded_bridge {
            // Even a previously installed Bridge awaiting pairing repair may
            // not silently authorize a separate browser in auto mode.
            std::fs::write(
                config_dir.join("browser-extension-installed"),
                "sunox-browser-bridge-installed\nschema=1\n",
            )
            .expect("installation evidence");
        }
        let browser = temp.path().join("fake-chrome");
        std::fs::write(
            &browser,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$SUNOX_BROWSER_PROBE_LOG\"\nexit 1\n",
        )
        .expect("fake browser");
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o700))
            .expect("executable fixture");
        let invocation = temp.path().join("browser-argv.txt");
        let listener = TcpListener::bind("127.0.0.1:0").expect("local mock");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "CLI did not reach billing mock");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("mock accept: {error}"),
                }
            };
            // macOS inherits the listener's nonblocking mode on accept.
            stream
                .set_nonblocking(false)
                .expect("blocking accepted stream");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 4096];
            while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            assert!(String::from_utf8_lossy(&bytes).starts_with("GET /api/billing/info/ HTTP/1.1"));
            let body = r#"{"detail":"browser-policy-mock-stop"}"#;
            write!(stream, "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let mut command = Command::cargo_bin("sunox").expect("CLI binary");
        command
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .env("SUNOX_BROWSER_PATH", &browser)
            .env("SUNOX_BROWSER_PROBE_LOG", &invocation)
            .env("SUNOX_TEST_API_BASE_URL", base_url)
            .args([
                "-c",
                &format!("challenge_browser={mode}"),
                "create",
                "test prompt",
                "--no-captcha",
                "--json",
            ])
            .timeout(Duration::from_secs(15));
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
        ] {
            command.env_remove(name);
        }
        let assertion = command.assert().failure();
        let stderr = String::from_utf8_lossy(&assertion.get_output().stderr);
        assert!(
            stderr.contains("browser-policy-mock-stop"),
            "{mode}: {stderr}"
        );
        server.join().expect("billing mock");
        assert_eq!(
            invocation.exists(),
            expect_probe,
            "mode={mode}, recorded_bridge={recorded_bridge}"
        );
        if expect_probe {
            assert!(
                std::fs::read_to_string(&invocation)
                    .unwrap()
                    .contains("--headless=new")
            );
        }
    }
}

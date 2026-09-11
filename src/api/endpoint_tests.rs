use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;
use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::{Duration, timeout};

use super::SunoClient;
use super::extend::ExtendClipOptions;
use super::inspiration::InspirationOptions;
use super::lyrics::CowriteLyricsOptions;
use super::paint::{PaintMode, PaintOptions};
use super::remaster::RemasterOptions;
use super::types::{
    Clip, ClipReaction, ControlSliders, CreateAudioUploadRequest, CreateAudioUploadSpec,
    CreateImageUploadRequest, CreatePersonaRequest, CreateVoiceVerificationRequest,
    EditPersonaRequest, FeedFilters, FinishAudioUploadRequest, GenerateRequest,
    InitializeAudioClipRequest, PersonaListScope, PlaylistReaction, ProcessVoiceSampleRequest,
    ProcessVoiceVerificationRecordingRequest, SetMetadataRequest,
};
use crate::auth::{AuthState, BrowserEnvironment};
use crate::cli::{Cli, Commands};
use crate::core::{AppConfig, CliError};

struct CapturedRequest {
    method: String,
    path: String,
    headers: String,
    body: String,
}

struct MockServer {
    base_url: String,
    requests: oneshot::Receiver<Vec<CapturedRequest>>,
    idle_timeout: Duration,
}

const MOCK_SERVER_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

fn billing_info_response(plan_id: &str) -> String {
    serde_json::json!({
        "credits": 0,
        "total_credits_left": 0,
        "monthly_usage": 0,
        "monthly_limit": 0,
        "is_active": true,
        "plan": {
            "id": plan_id,
            "name": "Pro Plan",
            "plan_key": "pro",
            "usage_plan_features": []
        },
        "models": [
            {
                "name": "v5.5",
                "external_key": "chirp-fenix",
                "can_use": true,
                "is_default_model": true,
                "description": "default test model",
                "capabilities": ["all"],
                "badges": ["custom"],
                "max_lengths": {}
            },
            {
                "name": "v4.5 fixture",
                "external_key": "chirp-v4-5",
                "can_use": true,
                "is_default_model": false,
                "description": "contract fixture",
                "max_lengths": {}
            },
            {
                "name": "v3 fixture",
                "external_key": "chirp-v3-0",
                "can_use": true,
                "is_default_model": false,
                "description": "contract fixture",
                "max_lengths": {}
            }
        ],
        "period": "month",
        "renews_on": null,
        "remaster_model_types": []
    })
    .to_string()
}

#[tokio::test]
async fn stale_command_mutation_preflight_is_revalidated_before_a_later_write() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[billing.as_str(), "{}"]).await;
    let client = server.client();
    *client
        .mutation_auth_preflight_at
        .lock()
        .expect("mutation preflight mutex") =
        Some(std::time::Instant::now() - Duration::from_secs(31));

    client
        .set_visibility("clip-stale-auth", false)
        .await
        .expect("write after refreshing stale mutation auth");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/gen/clip-stale-auth/set_visibility/");
}

#[tokio::test]
async fn stale_preflight_covers_legacy_visual_and_voice_mutation_senders() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"image_url":"https://cdn.example/generated.jpeg"}"#,
        billing.as_str(),
        r#"{"id":"verification-1","status":"pending"}"#,
    ])
    .await;
    let client = server.client();
    let stale = || Some(std::time::Instant::now() - Duration::from_secs(31));

    *client
        .mutation_auth_preflight_at
        .lock()
        .expect("mutation preflight mutex") = stale();
    client
        .generate_prompt_image("neon rain")
        .await
        .expect("visual write after stale auth");

    *client
        .mutation_auth_preflight_at
        .lock()
        .expect("mutation preflight mutex") = stale();
    client
        .create_voice_verification(
            "workflow-1",
            &CreateVoiceVerificationRequest {
                voice_recording_id: "recording-main".into(),
                verification_recording_id: "recording-verify".into(),
                phrase_id: "phrase-1".into(),
            },
        )
        .await
        .expect("voice write after stale auth");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/gen/prompt_image/");
    assert_eq!(requests[2].path, "/api/billing/info/");
    assert_eq!(requests[3].path, "/api/voice-verification/");
    assert!(requests[1].headers.contains("authorization: Bearer "));
    assert!(requests[3].headers.contains("authorization: Bearer "));
}

fn billing_info_with_models(plan_id: &str, models: serde_json::Value) -> String {
    let mut value = serde_json::from_str::<serde_json::Value>(&billing_info_response(plan_id))
        .expect("billing fixture");
    value["models"] = models;
    value.to_string()
}

impl MockServer {
    async fn json(response_body: &str) -> Self {
        Self::json_sequence(&[response_body]).await
    }

    async fn json_sequence(response_bodies: &[&str]) -> Self {
        let responses = response_bodies
            .iter()
            .map(|body| (200, body.to_string()))
            .collect::<Vec<_>>();
        Self::response_sequence(responses).await
    }

    async fn response_sequence(responses: Vec<(u16, String)>) -> Self {
        Self::response_sequence_with_idle_timeout(responses, MOCK_SERVER_IDLE_TIMEOUT).await
    }

    async fn response_sequence_with_idle_timeout(
        responses: Vec<(u16, String)>,
        idle_timeout: Duration,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut captured = Vec::with_capacity(responses.len());
            for (status, response_body) in responses {
                let Ok(Ok((stream, _))) = timeout(idle_timeout, listener.accept()).await else {
                    break;
                };
                captured.push(capture_request_with_status(stream, status, &response_body).await);
            }
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout,
        }
    }

    async fn delayed_response_sequence(responses: Vec<(u16, String, Duration)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut captured = Vec::with_capacity(responses.len());
            for (status, response_body, delay) in responses {
                let Ok(Ok((stream, _))) =
                    timeout(MOCK_SERVER_IDLE_TIMEOUT, listener.accept()).await
                else {
                    break;
                };
                tokio::time::sleep(delay).await;
                captured.push(capture_request_with_status(stream, status, &response_body).await);
            }
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout: MOCK_SERVER_IDLE_TIMEOUT,
        }
    }

    async fn delayed_json(response_body: &str, delay: Duration) -> Self {
        Self::delayed_response_sequence(vec![(200, response_body.to_string(), delay)]).await
    }

    async fn json_status_sequence(response_bodies: &[(u16, &str)]) -> Self {
        Self::response_sequence(
            response_bodies
                .iter()
                .map(|(status, body)| (*status, body.to_string()))
                .collect(),
        )
        .await
    }

    async fn json_until_idle(response_body: &str, max_requests: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();
        let response_body = response_body.to_string();

        tokio::spawn(async move {
            let mut captured = Vec::new();
            while captured.len() < max_requests {
                let Ok(Ok((stream, _))) = timeout(Duration::from_secs(1), listener.accept()).await
                else {
                    break;
                };
                captured.push(capture_request(stream, &response_body).await);
            }
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout: Duration::from_secs(1),
        }
    }

    async fn resets_then_json(reset_count: usize, response_body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();
        let response_body = response_body.to_string();

        tokio::spawn(async move {
            let mut captured = Vec::with_capacity(reset_count + 1);

            for _ in 0..reset_count {
                let (mut reset_stream, _) = listener.accept().await.expect("accept reset request");
                captured.push(read_request(&mut reset_stream).await);
                drop(reset_stream);
            }

            let (stream, _) = listener.accept().await.expect("accept fallback request");
            captured.push(capture_request(stream, &response_body).await);
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout: MOCK_SERVER_IDLE_TIMEOUT,
        }
    }

    async fn resets_until_idle(max_requests: usize, idle_timeout: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut captured = Vec::new();
            while captured.len() < max_requests {
                let Ok(Ok((mut stream, _))) = timeout(idle_timeout, listener.accept()).await else {
                    break;
                };
                captured.push(read_request(&mut stream).await);
                drop(stream);
            }
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout,
        }
    }

    async fn truncated_json_then_json(response_body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();
        let response_body = response_body.to_string();

        tokio::spawn(async move {
            let mut captured = Vec::with_capacity(2);

            let (mut truncated_stream, _) = listener
                .accept()
                .await
                .expect("accept truncated response request");
            captured.push(read_request(&mut truncated_stream).await);
            write_truncated_json_response(&mut truncated_stream, &response_body).await;
            drop(truncated_stream);

            let (stream, _) = listener.accept().await.expect("accept fallback request");
            captured.push(capture_request(stream, &response_body).await);
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout: MOCK_SERVER_IDLE_TIMEOUT,
        }
    }

    async fn truncated_json_once(response_body: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();
        let response_body = response_body.to_string();

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let captured = read_request(&mut stream).await;
            write_truncated_json_response(&mut stream, &response_body).await;
            drop(stream);
            let _ = tx.send(vec![captured]);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
            idle_timeout: MOCK_SERVER_IDLE_TIMEOUT,
        }
    }

    fn client(&self) -> SunoClient {
        self.client_with_auth(AuthState {
            jwt: Some(test_jwt_with_subject("user-1")),
            device_id: Some("device-1".into()),
            ..AuthState::default()
        })
    }

    fn client_with_auth(&self, auth: AuthState) -> SunoClient {
        SunoClient::new_for_tests(self.base_url.clone(), auth).expect("test client")
    }

    async fn captured(self) -> CapturedRequest {
        let mut requests = self.captured_all().await;
        assert_eq!(requests.len(), 1);
        requests.remove(0)
    }

    async fn captured_all(self) -> Vec<CapturedRequest> {
        timeout(self.idle_timeout + Duration::from_secs(1), self.requests)
            .await
            .expect("mock server did not finish capturing requests")
            .expect("captured requests")
    }
}

fn test_jwt_with_subject(subject: &str) -> String {
    let header = BASE64URL.encode(r#"{"alg":"none","typ":"JWT"}"#);
    let claims =
        BASE64URL.encode(serde_json::json!({"sub": subject, "exp": 4_102_444_800_u64}).to_string());
    format!("{header}.{claims}.test-signature")
}

async fn capture_request(mut stream: TcpStream, response_body: &str) -> CapturedRequest {
    capture_request_with_status_inner(&mut stream, 200, response_body).await
}

async fn capture_request_with_status(
    mut stream: TcpStream,
    status: u16,
    response_body: &str,
) -> CapturedRequest {
    capture_request_with_status_inner(&mut stream, status, response_body).await
}

#[tokio::test]
async fn mock_server_returns_captured_requests_when_a_sequence_is_incomplete() {
    let server = MockServer::response_sequence_with_idle_timeout(
        vec![(200, "{}".into()), (200, "{}".into())],
        Duration::from_millis(50),
    )
    .await;
    reqwest::Client::new()
        .get(format!("{}/only-request", server.base_url))
        .send()
        .await
        .expect("send one request");

    let requests = timeout(Duration::from_secs(1), server.captured_all())
        .await
        .expect("incomplete request sequences must not hang tests");

    assert_eq!(requests.len(), 1);
}

#[test]
fn default_mock_idle_timeout_covers_production_poll_backoff() {
    assert!(MOCK_SERVER_IDLE_TIMEOUT > crate::workflow::tasks::MAX_POLL_BACKOFF);
}

async fn capture_request_with_status_inner(
    stream: &mut TcpStream,
    status: u16,
    response_body: &str,
) -> CapturedRequest {
    let captured = read_request(stream).await;
    let reason = match status {
        200 => "OK",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    stream
        .write_all(response.as_bytes())
        .await
        .expect("write response");

    captured
}

async fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut data = Vec::new();
    let mut buf = [0_u8; 1024];

    let header_end = loop {
        let n = stream.read(&mut buf).await.expect("read request");
        assert_ne!(n, 0, "connection closed before headers");
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let headers = String::from_utf8_lossy(&data[..header_end]).to_string();
    let request_line = headers.lines().next().expect("request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().expect("method").to_string();
    let path = request_parts.next().expect("path").to_string();
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .or_else(|| {
            headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
        })
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    while data.len() < header_end + content_length {
        let n = stream.read(&mut buf).await.expect("read body");
        assert_ne!(n, 0, "connection closed before body");
        data.extend_from_slice(&buf[..n]);
    }

    let body = String::from_utf8_lossy(&data[header_end..header_end + content_length]).into();
    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

async fn write_truncated_json_response(stream: &mut TcpStream, full_body: &str) {
    let partial_body = &full_body.as_bytes()[..full_body.len() / 2];
    let headers = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        full_body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .expect("write truncated response headers");
    stream
        .write_all(partial_body)
        .await
        .expect("write truncated response body");
}

#[tokio::test]
async fn idempotent_read_retries_after_a_transport_reset() {
    let server = MockServer::resets_then_json(1, r#"{"ok":true}"#).await;
    let client = server.client();

    let response: serde_json::Value = client
        .read_json_with_transport_retry(client.get("/read-only"))
        .await
        .expect("fallback read");
    assert_eq!(response, serde_json::json!({"ok": true}));

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.method == "GET" && request.path == "/read-only")
    );
}

#[tokio::test]
async fn idempotent_read_retries_when_a_json_body_is_truncated() {
    let server = MockServer::truncated_json_then_json(r#"{"ok":true}"#).await;
    let client = server.client();

    let response: serde_json::Value = client
        .read_json_with_transport_retry(client.get("/read-only"))
        .await
        .expect("fallback after truncated body");
    assert_eq!(response, serde_json::json!({"ok": true}));

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.method == "GET" && request.path == "/read-only")
    );
}

#[tokio::test]
async fn idempotent_read_uses_final_normal_retry_after_two_resets() {
    let server = MockServer::resets_then_json(2, r#"{"ok":true}"#).await;
    let client = server.client();

    let response: serde_json::Value = client
        .read_json_with_transport_retry(client.get("/read-only"))
        .await
        .expect("final normal fallback read");
    assert_eq!(response, serde_json::json!({"ok": true}));

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.method == "GET" && request.path == "/read-only")
    );
}

#[tokio::test]
async fn idempotent_read_rejects_mutation_methods_before_network_io() {
    let client = SunoClient::new_for_tests(
        "http://127.0.0.1:9".into(),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");

    let result: Result<serde_json::Value, CliError> = client
        .read_json_with_transport_retry(client.post("/must-not-send"))
        .await;
    let error = result.expect_err("mutation requests must never enter read fallback");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("cannot send POST requests"))
    );
}

#[tokio::test]
async fn delete_clips_posts_current_web_trash_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"clip-a","title":"A","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z","is_trashed":true}"#,
        r#"{"id":"clip-b","title":"B","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z","is_trashed":true}"#,
    ])
    .await;
    let client = server.client();

    client
        .delete_clips(&["clip-a".to_string(), "clip-b".to_string()])
        .await
        .expect("delete clips");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "trash": true,
            "clip_ids": ["clip-a", "clip-b"]
        })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/clip/clip-a");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/clip/clip-b");
}

#[tokio::test]
async fn clip_trash_server_error_is_ambiguous_and_not_replayed() {
    let server = MockServer::response_sequence_with_idle_timeout(
        vec![(500, r#"{"detail":"unknown accepted state"}"#.into())],
        Duration::from_millis(50),
    )
    .await;
    let client = server.client();

    let error = client
        .delete_clips(&["clip-a".to_string()])
        .await
        .expect_err("5xx cannot prove the trash write was rejected");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(error.details().expect("details")["operation"], "clip_trash");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
}

#[tokio::test]
async fn clip_trash_transport_loss_is_ambiguous_and_not_replayed() {
    let server = MockServer::resets_until_idle(2, Duration::from_millis(50)).await;
    let client = server.client();

    let error = client
        .delete_clips(&["clip-a".to_string()])
        .await
        .expect_err("a lost response cannot prove the trash write was rejected");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1, "mutation transport must never retry");
}

#[tokio::test]
async fn clip_trash_explicit_auth_rejection_is_not_replayed_or_ambiguous() {
    let server = MockServer::response_sequence_with_idle_timeout(
        vec![(401, String::new())],
        Duration::from_millis(50),
    )
    .await;
    let client = server.client();

    let error = client
        .delete_clips(&["clip-a".to_string()])
        .await
        .expect_err("an explicit 401 is a reliable rejection");

    assert!(matches!(error, CliError::AuthExpired));
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn clip_trash_never_follows_a_redirect_with_a_second_post() {
    let target_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind redirect target");
    let target_addr = target_listener
        .local_addr()
        .expect("redirect target address");
    let redirect_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind redirect source");
    let redirect_addr = redirect_listener
        .local_addr()
        .expect("redirect source address");

    let redirect_task = tokio::spawn(async move {
        let (mut stream, _) = redirect_listener
            .accept()
            .await
            .expect("accept redirect POST");
        let request = read_request(&mut stream).await;
        let response = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{target_addr}/api/gen/trash\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write redirect response");
        request
    });
    let target_task = tokio::spawn(async move {
        let Ok(Ok((stream, _))) =
            timeout(Duration::from_millis(250), target_listener.accept()).await
        else {
            return None;
        };
        Some(capture_request(stream, "{}").await)
    });

    let client = SunoClient::new_for_tests(
        format!("http://{redirect_addr}"),
        AuthState {
            jwt: Some(test_jwt_with_subject("user-1")),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let result = client.delete_clips(&["clip-a".to_string()]).await;
    let redirect_request = redirect_task.await.expect("redirect source task");
    let redirected_request = target_task.await.expect("redirect target task");

    assert_eq!(redirect_request.method, "POST");
    let error = result.expect_err("redirected mutation result is ambiguous");
    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert!(
        redirected_request.is_none(),
        "mutation must not follow redirect"
    );
}

#[tokio::test]
async fn purge_clips_posts_current_web_permanent_delete_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .purge_clips(&["clip-a".to_string(), "clip-b".to_string()])
        .await
        .expect("purge clips");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/clips/delete/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "ids": ["clip-a", "clip-b"]
        })
    );
}

#[tokio::test]
async fn purge_clips_enforces_the_current_batch_size() {
    let server = MockServer::json_until_idle("{}", 3).await;
    let client = server.client();
    let ids = (0..21)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();

    client.purge_clips(&ids).await.expect("purge clips");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let first =
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("first purge request");
    let second =
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("second purge request");
    assert_eq!(first["ids"].as_array().expect("first ids").len(), 20);
    assert_eq!(second["ids"], serde_json::json!(["clip-20"]));
}

#[tokio::test]
async fn purge_clips_reports_completed_and_unattempted_batches() {
    let server =
        MockServer::json_status_sequence(&[(200, "{}"), (500, r#"{"detail":"delete failed"}"#)])
            .await;
    let client = server.client();
    let ids = (0..41)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();

    let error = client
        .purge_clips(&ids)
        .await
        .expect_err("second purge batch must report a partial mutation");

    let details = error.details().expect("partial mutation details");
    assert_eq!(
        details["purged_clip_ids"].as_array().expect("purged").len(),
        20
    );
    assert_eq!(
        details["failed"]["clip_ids"]
            .as_array()
            .expect("failed")
            .len(),
        20
    );
    assert_eq!(
        details["not_attempted_clip_ids"],
        serde_json::json!(["clip-40"])
    );
}

#[tokio::test]
async fn empty_clip_trash_pages_before_deleting_in_serial_batches() {
    let first_clips = (0..20)
        .map(|index| {
            serde_json::json!({
                "id": format!("clip-{index}"),
                "title": format!("Clip {index}"),
                "status": "complete",
                "model_name": "chirp-fenix",
                "created_at": "2026-07-10T00:00:00Z"
            })
        })
        .collect::<Vec<_>>();
    let first_page = serde_json::json!({
        "clips": first_clips,
        "next_cursor": "next-page",
        "has_more": true
    })
    .to_string();
    let second_page = serde_json::json!({
        "clips": [{
            "id": "clip-20",
            "title": "Clip 20",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-07-10T00:00:00Z"
        }],
        "next_cursor": null,
        "has_more": false
    })
    .to_string();
    let server =
        MockServer::json_sequence(&[first_page.as_str(), second_page.as_str(), "{}", "{}"]).await;
    let client = server.client();

    let purged = client.empty_clip_trash().await.expect("empty clip trash");

    assert_eq!(purged.len(), 21);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].path, "/api/feed/v3");
    let first_filter = serde_json::from_str::<serde_json::Value>(&requests[0].body)
        .expect("first trash page request");
    assert_eq!(
        first_filter["filters"],
        serde_json::json!({ "trashed": "True" })
    );
    let second_filter = serde_json::from_str::<serde_json::Value>(&requests[1].body)
        .expect("second trash page request");
    assert_eq!(second_filter["cursor"], "next-page");
    assert_eq!(requests[2].path, "/api/clips/delete/");
    assert_eq!(requests[3].path, "/api/clips/delete/");
    let first_delete =
        serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("first delete request");
    let second_delete = serde_json::from_str::<serde_json::Value>(&requests[3].body)
        .expect("second delete request");
    assert_eq!(
        first_delete["ids"].as_array().expect("first batch").len(),
        20
    );
    assert_eq!(second_delete["ids"], serde_json::json!(["clip-20"]));
}

#[tokio::test]
async fn empty_clip_trash_rejects_a_repeated_pagination_cursor_before_deleting() {
    let page = serde_json::json!({
        "clips": [],
        "next_cursor": "same-cursor",
        "has_more": true
    })
    .to_string();
    let server = MockServer::json_sequence(&[page.as_str(), page.as_str()]).await;
    let client = server.client();

    let error = client
        .empty_clip_trash()
        .await
        .expect_err("repeated cursor must stop enumeration");

    assert!(matches!(
        error,
        CliError::Api {
            code: "schema_drift",
            ..
        }
    ));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2, "no permanent delete may start");
}

#[tokio::test]
async fn empty_clip_trash_rejects_a_missing_pagination_cursor_before_deleting() {
    let server = MockServer::json(r#"{"clips":[],"next_cursor":null,"has_more":true}"#).await;
    let client = server.client();

    let error = client
        .empty_clip_trash()
        .await
        .expect_err("missing cursor must stop enumeration");

    assert!(matches!(
        error,
        CliError::Api {
            code: "schema_drift",
            ..
        }
    ));
    let request = server.captured().await;
    assert_eq!(request.path, "/api/feed/v3");
}

#[tokio::test]
async fn empty_clip_trash_does_not_submit_a_delete_for_an_empty_trash() {
    let server = MockServer::json(r#"{"clips":[],"next_cursor":null,"has_more":false}"#).await;
    let client = server.client();

    let purged = client.empty_clip_trash().await.expect("empty clip trash");

    assert!(purged.is_empty());
    let request = server.captured().await;
    assert_eq!(request.path, "/api/feed/v3");
}

#[tokio::test]
async fn empty_clip_trash_preserves_the_first_delete_error() {
    let feed = serde_json::json!({
        "clips": [{
            "id": "clip-1",
            "title": "Clip 1",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-07-10T00:00:00Z"
        }],
        "next_cursor": null,
        "has_more": false
    })
    .to_string();
    let server = MockServer::json_status_sequence(&[(200, feed.as_str()), (429, "")]).await;
    let client = server.client();

    let error = client
        .empty_clip_trash()
        .await
        .expect_err("first delete failure must keep its semantic error");

    assert!(matches!(error, CliError::RateLimited));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn empty_clip_trash_reports_completed_and_unattempted_batches() {
    let first_clips = (0..20)
        .map(|index| {
            serde_json::json!({
                "id": format!("clip-{index}"),
                "title": format!("Clip {index}"),
                "status": "complete",
                "model_name": "chirp-fenix",
                "created_at": "2026-07-10T00:00:00Z"
            })
        })
        .collect::<Vec<_>>();
    let second_clips = (20..41)
        .map(|index| {
            serde_json::json!({
                "id": format!("clip-{index}"),
                "title": format!("Clip {index}"),
                "status": "complete",
                "model_name": "chirp-fenix",
                "created_at": "2026-07-10T00:00:00Z"
            })
        })
        .collect::<Vec<_>>();
    let first_page = serde_json::json!({
        "clips": first_clips,
        "next_cursor": "next-page",
        "has_more": true
    })
    .to_string();
    let second_page = serde_json::json!({
        "clips": second_clips,
        "next_cursor": null,
        "has_more": false
    })
    .to_string();
    let server = MockServer::json_status_sequence(&[
        (200, first_page.as_str()),
        (200, second_page.as_str()),
        (200, "{}"),
        (500, r#"{"detail":"delete failed"}"#),
    ])
    .await;
    let client = server.client();

    let error = client
        .empty_clip_trash()
        .await
        .expect_err("second delete batch must fail");

    let details = error.details().expect("partial mutation details");
    assert_eq!(
        details["purged_clip_ids"].as_array().expect("purged").len(),
        20
    );
    assert_eq!(
        details["failed"]["clip_ids"]
            .as_array()
            .expect("failed")
            .len(),
        20
    );
    assert_eq!(details["failed"]["code"], "ambiguous_mutation");
    assert!(
        details["failed"]["details"]["cause"]["message"]
            .as_str()
            .expect("failure message")
            .contains("delete failed")
    );
    assert_eq!(
        details["not_attempted_clip_ids"],
        serde_json::json!(["clip-40"])
    );
}

#[tokio::test]
async fn requests_use_stored_browser_environment_headers_when_available() {
    let server = MockServer::json(r#"{"required":false}"#).await;
    let client = server.client_with_auth(AuthState {
        jwt: Some("test-jwt".into()),
        device_id: Some("device-1".into()),
        browser_environment: Some(BrowserEnvironment {
            browser_source: Some("interactive-browser".into()),
            user_agent: Some("SunoxTestBrowser/1.0".into()),
            accept_language: Some("en-US,en;q=0.9".into()),
            client_hints: None,
        }),
        ..AuthState::default()
    });

    client
        .generation_challenge()
        .await
        .expect("generation challenge");

    let request = server.captured().await;
    let headers = request.headers.to_ascii_lowercase();
    assert!(headers.contains("user-agent: sunoxtestbrowser/1.0"));
    assert!(headers.contains("accept-language: en-us,en;q=0.9"));
}

#[tokio::test]
async fn requests_use_browser_like_fallback_headers_when_environment_is_partial() {
    let server = MockServer::json(r#"{"required":false}"#).await;
    let client = server.client();

    client
        .generation_challenge()
        .await
        .expect("generation challenge");

    let request = server.captured().await;
    let headers = request.headers.to_ascii_lowercase();
    assert!(headers.contains("user-agent: mozilla/5.0"));
    assert!(headers.contains("accept: */*"));
    assert!(headers.contains("accept-language: en"));
    assert!(headers.contains("sec-ch-ua: \"google chrome\";v=\"149\""));
    assert!(headers.contains("sec-ch-ua-mobile: ?0"));
    assert!(headers.contains("sec-ch-ua-platform: "));
    assert!(headers.contains("sec-fetch-mode: cors"));
    assert!(headers.contains("sec-fetch-dest: empty"));
    assert!(headers.contains("sec-fetch-site: same-site"));
    assert!(headers.contains("priority: u=1, i"));
}

#[tokio::test]
async fn requests_omit_device_id_instead_of_fabricating_one() {
    let server = MockServer::json(r#"{"required":false}"#).await;
    let client = server.client_with_auth(AuthState {
        jwt: Some("test-jwt".into()),
        device_id: None,
        ..AuthState::default()
    });

    client
        .generation_challenge()
        .await
        .expect("generation challenge");

    let request = server.captured().await;
    assert!(!request.headers.to_ascii_lowercase().contains("device-id:"));
}

#[tokio::test]
async fn challenge_recheck_refresh_skips_without_clerk_material() {
    let client = SunoClient::new_for_tests(
        "http://127.0.0.1:1".into(),
        AuthState {
            jwt: Some("test-jwt".into()),
            device_id: Some("device-1".into()),
            clerk_client_cookie: None,
            ..AuthState::default()
        },
    )
    .expect("test client");

    assert!(
        !client
            .try_refresh_jwt_for_challenge_recheck()
            .await
            .expect("refresh recheck")
    );
}

#[tokio::test]
async fn mutation_auth_preflight_proves_the_jwt_with_a_read_only_billing_request() {
    let billing = billing_info_response("pro");
    let server = MockServer::json(&billing).await;
    let client = server.client();

    client
        .prepare_mutation_auth()
        .await
        .expect("mutation auth preflight");

    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/billing/info/");
}

#[tokio::test]
async fn restore_clips_posts_current_web_trash_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"clip-a","title":"A","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z","is_trashed":false}"#,
    ])
    .await;
    let client = server.client();

    client
        .restore_clips(&["clip-a".to_string()])
        .await
        .expect("restore clips");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "trash": false,
            "clip_ids": ["clip-a"]
        })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/clip/clip-a");
}

#[tokio::test]
async fn get_clips_uses_current_feed_v3_exact_id_contract_in_input_order() {
    let server = MockServer::json(
        r#"{"clips":[{"id":"clip-c","title":"C","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"},{"id":"clip-a","title":"A","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"},{"id":"clip-b","title":"B","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}],"has_more":false}"#,
    )
    .await;
    let client = server.client();

    let clips = client
        .get_clips(&[
            "clip-a".to_string(),
            "clip-b".to_string(),
            "clip-c".to_string(),
        ])
        .await
        .expect("get clips");

    assert_eq!(clips.len(), 3);
    assert_eq!(
        clips
            .iter()
            .map(|clip| clip.id.as_str())
            .collect::<Vec<_>>(),
        vec!["clip-a", "clip-b", "clip-c"]
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/feed/v3");
    let body = serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json");
    assert_eq!(body["limit"], 3);
    assert_eq!(body["filters"]["ids"]["presence"], "True");
    assert_eq!(
        body["filters"]["ids"]["clipIds"],
        serde_json::json!(["clip-a", "clip-b", "clip-c"])
    );
}

#[tokio::test]
async fn get_clips_omits_ids_missing_from_current_feed_response() {
    let server = MockServer::json(
        r#"{"clips":[{"id":"clip-c","title":"C","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}],"has_more":false}"#,
    )
    .await;
    let client = server.client();

    let clips = client
        .get_clips(&[
            "clip-a".to_string(),
            "clip-b".to_string(),
            "clip-c".to_string(),
        ])
        .await
        .expect("get clips");

    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, "clip-c");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/feed/v3");
}

#[tokio::test]
async fn get_clips_keeps_current_direct_contract_for_one_clip() {
    let server = MockServer::json(
        r#"{"id":"clip-a","title":"A","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}"#,
    )
    .await;
    let client = server.client();

    let clips = client
        .get_clips(&["clip-a".to_string()])
        .await
        .expect("get clip");

    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, "clip-a");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/clip/clip-a");
}

#[tokio::test]
async fn feed_posts_v3_workspace_filter_contract() {
    let server = MockServer::json(r#"{"clips":[],"next_cursor":"next","has_more":true}"#).await;
    let client = server.client();

    let response = client
        .feed(
            Some("cursor-1".into()),
            None,
            FeedFilters::default_workspace(),
        )
        .await
        .expect("feed");

    assert!(response.has_more);
    assert_eq!(response.next_cursor.as_deref(), Some("next"));
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/feed/v3");
    let body = serde_json::from_str::<serde_json::Value>(&request.body).expect("request json");
    assert_eq!(body["cursor"], "cursor-1");
    assert_eq!(body["limit"], 20);
    assert_eq!(body["filters"]["workspace"]["presence"], "True");
    assert_eq!(body["filters"]["workspace"]["workspaceId"], "default");
    assert_eq!(body["filters"]["fromStudioProject"]["presence"], "False");
    assert_eq!(body["filters"]["stem"]["presence"], "False");
    assert_eq!(body["filters"]["trashed"], "False");
}

#[tokio::test]
async fn feed_posts_public_liked_upload_cover_extend_popular_filter_contract() {
    let server = MockServer::json(r#"{"clips":[]}"#).await;
    let client = server.client();

    client
        .feed(
            None,
            Some(20),
            FeedFilters::default_workspace()
                .with_public()
                .with_liked()
                .with_upload()
                .with_cover()
                .with_extend()
                .with_popular_sort(),
        )
        .await
        .expect("feed");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/feed/v3");
    let body = serde_json::from_str::<serde_json::Value>(&request.body).expect("request json");
    assert_eq!(body["limit"], 20);
    assert_eq!(body["filters"]["liked"], "True");
    assert_eq!(body["filters"]["public"], "True");
    assert_eq!(body["filters"]["upload"], "True");
    assert!(body["filters"].get("disliked").is_none());
    assert_eq!(body["filters"]["cover"]["presence"], "True");
    assert_eq!(body["filters"]["extend"]["presence"], "True");
    assert_eq!(body["filters"]["sort"]["sortBy"], "upvote_count");
    assert_eq!(body["filters"]["sort"]["sortDirection"], "desc");
}

#[tokio::test]
async fn search_posts_v3_search_text_filter_contract() {
    let server = MockServer::json(r#"{"clips":[]}"#).await;
    let client = server.client();

    client.search("summer pop").await.expect("search");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/feed/v3");
    let body = serde_json::from_str::<serde_json::Value>(&request.body).expect("request json");
    assert_eq!(body["limit"], 50);
    assert_eq!(body["filters"]["searchText"], "summer pop");
    assert_eq!(body["filters"]["workspace"]["workspaceId"], "default");
}

#[tokio::test]
async fn search_page_preserves_cursor_and_limit_contract() {
    let server = MockServer::json(r#"{"clips":[],"has_more":false}"#).await;
    let client = server.client();

    client
        .search_page("summer pop", Some("cursor-2".into()), Some(12))
        .await
        .expect("search page");

    let request = server.captured().await;
    let body = serde_json::from_str::<serde_json::Value>(&request.body).expect("request json");
    assert_eq!(body["cursor"], "cursor-2");
    assert_eq!(body["limit"], 12);
    assert_eq!(body["filters"]["searchText"], "summer pop");
}

#[tokio::test]
async fn clip_info_fetches_song_page_supplemental_contract() {
    let server = MockServer::json_sequence(&[
        r#"{"source_clips":[{"clip_id":"source-1","title":"Source Song","image_url":"https://cdn2.suno.ai/image_source-1.jpeg","audio_url":"https://cdn1.suno.ai/source-1.mp3","is_deleted":true,"relationship":"COV","user":{"user_id":"user-1","user_display_name":"Source User","user_handle":"source"}}]}"#,
        r#"{"results":[{"id":"comment-1","clip_id":"clip-a","content":"Nice","num_likes":2}],"allow_comment":true,"total_count":1}"#,
        r#"{"count":3,"is_capped":true,"ranking_version":2}"#,
        r#"{"similar_clips":[{"id":"similar-1","title":"Similar","status":"complete","model_name":"chirp-fenix","created_at":"2026-07-03T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let info = client
        .clip_info(Clip {
            id: "clip-a".into(),
            title: "Demo".into(),
            status: "complete".into(),
            model_name: "chirp-fenix".into(),
            audio_url: Some("https://studio-api.prod.suno.com/api/forbidden".into()),
            video_url: None,
            image_url: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            is_trashed: None,
            is_download_unlocked: None,
            action_config: None,
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
            extra: [(
                "media_urls".into(),
                serde_json::json!([{
                    "url": "https://stream.example/clip-a.m4a",
                    "content_type": "m4a-opus",
                    "delivery": "progressive"
                }]),
            )]
            .into_iter()
            .collect(),
        })
        .await
        .expect("clip info");

    assert_eq!(info.clip.id, "clip-a");
    assert_eq!(
        info.playback_url.as_deref(),
        Some("https://stream.example/clip-a.m4a")
    );
    assert_eq!(info.attribution.source_clips.len(), 1);
    assert_eq!(
        info.attribution.source_clips[0].clip_id.as_deref(),
        Some("source-1")
    );
    assert_eq!(
        info.attribution.source_clips[0].title.as_deref(),
        Some("Source Song")
    );
    assert_eq!(info.comments.total_count, 1);
    assert_eq!(info.remix_count.count, 3);
    assert!(info.remix_count.is_capped);
    assert_eq!(info.remix_count.extra["ranking_version"], 2);
    assert_eq!(info.similar_clips[0].id, "similar-1");
    assert!(info.supplemental_errors.is_empty());
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clips/clip-a/attribution");
    assert_eq!(
        requests[1].path,
        "/api/gen/clip-a/comments?order=most_liked"
    );
    assert_eq!(requests[2].path, "/api/clips/remixes/count?clip_id=clip-a");
    assert_eq!(requests[3].path, "/api/clips/get_similar/?id=clip-a");
}

#[tokio::test]
async fn clip_info_keeps_base_clip_when_supplemental_read_fails() {
    let server = MockServer::json_status_sequence(&[
        (500, r#"{"detail":"attribution unavailable"}"#),
        (
            200,
            r#"{"results":[],"allow_comment":true,"total_count":0}"#,
        ),
        (200, r#"{"count":0}"#),
        (200, r#"{"similar_clips":[]}"#),
    ])
    .await;
    let client = server.client();

    let info = client
        .clip_info(Clip {
            id: "clip-a".into(),
            title: "Demo".into(),
            status: "complete".into(),
            model_name: "chirp-fenix".into(),
            audio_url: Some("https://cdn1.suno.ai/clip-a.mp3".into()),
            video_url: None,
            image_url: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            is_trashed: None,
            is_download_unlocked: None,
            action_config: None,
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
            extra: Default::default(),
        })
        .await
        .expect("clip info should keep base clip when supplemental reads fail");

    assert_eq!(info.clip.id, "clip-a");
    assert_eq!(
        info.clip.audio_url.as_deref(),
        Some("https://cdn1.suno.ai/clip-a.mp3")
    );
    assert_eq!(
        info.playback_url.as_deref(),
        Some("https://cdn1.suno.ai/clip-a.mp3")
    );
    assert!(info.attribution.source_clips.is_empty());
    assert_eq!(info.comments.total_count, 0);
    assert_eq!(info.remix_count.count, 0);
    assert!(!info.remix_count.is_capped);
    assert!(info.similar_clips.is_empty());
    assert_eq!(info.supplemental_errors.len(), 1);
    assert_eq!(info.supplemental_errors[0].field, "attribution");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
}

#[tokio::test]
async fn clip_info_aborts_on_rate_limited_supplemental_read() {
    let server = MockServer::json_status_sequence(&[(429, "")]).await;
    let client = server.client();

    let err = client
        .clip_info(Clip {
            id: "clip-a".into(),
            title: "Demo".into(),
            status: "complete".into(),
            model_name: "chirp-fenix".into(),
            audio_url: Some("https://cdn1.suno.ai/clip-a.mp3".into()),
            video_url: None,
            image_url: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            is_trashed: None,
            is_download_unlocked: None,
            action_config: None,
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
            extra: Default::default(),
        })
        .await
        .expect_err("rate limit should not be hidden as supplemental data");

    assert!(matches!(err, CliError::RateLimited));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn clip_info_aborts_on_auth_expired_supplemental_read() {
    let server = MockServer::json_status_sequence(&[(401, "")]).await;
    let client = server.client();

    let err = client
        .clip_info(Clip {
            id: "clip-a".into(),
            title: "Demo".into(),
            status: "complete".into(),
            model_name: "chirp-fenix".into(),
            audio_url: Some("https://cdn1.suno.ai/clip-a.mp3".into()),
            video_url: None,
            image_url: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            is_trashed: None,
            is_download_unlocked: None,
            action_config: None,
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
            extra: Default::default(),
        })
        .await
        .expect_err("auth failure should not be hidden as supplemental data");

    assert!(matches!(err, CliError::AuthExpired));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn clip_reaction_posts_current_web_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_clip_reaction("clip-a", Some(ClipReaction::Dislike))
        .await
        .expect("set clip reaction");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/clip-a/update_reaction_type/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "reaction": "DISLIKE",
            "recommendation_metadata": {}
        })
    );
}

#[tokio::test]
async fn set_clip_metadata_posts_current_web_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_metadata(
            "clip-a",
            &SetMetadataRequest {
                title: Some("Renamed".into()),
                lyrics: None,
                caption: Some("Caption".into()),
                image_url: None,
                image_s3_id: None,
                is_audio_upload_tos_accepted: None,
                remove_image_cover: None,
                remove_video_cover: None,
            },
        )
        .await
        .expect("set metadata");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/clip-a/set_metadata/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "title": "Renamed",
            "caption": "Caption"
        })
    );
}

#[tokio::test]
async fn set_clip_metadata_posts_cover_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_metadata(
            "clip-a",
            &SetMetadataRequest {
                title: None,
                lyrics: None,
                caption: None,
                image_url: Some("https://cdn2.suno.ai/image_upload-1.jpeg".into()),
                image_s3_id: None,
                is_audio_upload_tos_accepted: None,
                remove_image_cover: None,
                remove_video_cover: Some(true),
            },
        )
        .await
        .expect("set metadata");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/clip-a/set_metadata/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "image_url": "https://cdn2.suno.ai/image_upload-1.jpeg",
            "remove_video_cover": true
        })
    );
}

#[tokio::test]
async fn set_clip_metadata_uses_current_uploaded_cover_identity() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_metadata(
            "clip-a",
            &SetMetadataRequest {
                image_s3_id: Some("image_upload-1".into()),
                ..SetMetadataRequest::default()
            },
        )
        .await
        .expect("set uploaded cover");

    let request = server.captured().await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "image_s3_id": "image_upload-1" })
    );
}

#[tokio::test]
async fn set_clip_metadata_surfaces_http_200_api_errors() {
    let server = MockServer::json(
        r#"{"error_type":"image_moderation_error","moderation_error_message":"Cover rejected"}"#,
    )
    .await;
    let client = server.client();

    let error = client
        .set_metadata(
            "clip-a",
            &SetMetadataRequest {
                image_s3_id: Some("image_upload-1".into()),
                ..SetMetadataRequest::default()
            },
        )
        .await
        .expect_err("semantic API error must not be reported as success");

    assert_eq!(error.error_code(), "metadata_update_rejected");
    assert!(error.to_string().contains("Cover rejected"));
}

#[tokio::test]
async fn set_clip_metadata_treats_malformed_accepted_body_as_ambiguous() {
    let server = MockServer::json("not-json").await;
    let client = server.client();

    let error = client
        .set_metadata("clip-a", &SetMetadataRequest::default())
        .await
        .expect_err("an unreadable accepted response cannot prove the metadata outcome");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity details")["stage"],
        "response_body"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn clip_visibility_preserves_an_explicit_client_rejection() {
    let server = MockServer::json_status_sequence(&[(401, r#"{"detail":"expired"}"#)]).await;
    let client = server.client();

    let error = client
        .set_visibility("clip-a", false)
        .await
        .expect_err("an explicit 401 proves the write was rejected");

    assert_ne!(error.error_code(), "ambiguous_mutation");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn set_clip_metadata_accepts_an_empty_success_body() {
    let server = MockServer::json("").await;
    let client = server.client();

    client
        .set_metadata(
            "clip-a",
            &SetMetadataRequest {
                title: Some("Updated".into()),
                ..SetMetadataRequest::default()
            },
        )
        .await
        .expect("empty successful response");
}

#[tokio::test]
async fn clip_cover_workflow_preserves_uploaded_image_when_metadata_update_fails() {
    let server = MockServer::json_status_sequence(&[(429, r#"{"detail":"rate limited"}"#)]).await;
    let client = server.client();
    let request = SetMetadataRequest {
        title: None,
        lyrics: None,
        caption: None,
        image_url: None,
        image_s3_id: Some("image_image-upload-1".into()),
        is_audio_upload_tos_accepted: None,
        remove_image_cover: None,
        remove_video_cover: None,
    };
    let cover = crate::workflow::image_upload::ImageUploadResult {
        upload_id: "image-upload-1".into(),
        image_url: "https://cdn2.suno.ai/image_image-upload-1.jpeg".into(),
        cover_image_s3_id: "image_image-upload-1".into(),
        moderation_status: Some("approved".into()),
    };

    let error = crate::workflow::image_upload::apply_uploaded_cover_to_clip(
        &client, "clip-a", &request, &cover,
    )
    .await
    .expect_err("metadata failure must keep the uploaded image identity");

    assert_eq!(error.error_code(), "partial_mutation");
    assert_eq!(
        error.details().expect("clip cover partial details"),
        &serde_json::json!({
            "operation": "clip_set",
            "clip_id": "clip-a",
            "cover": {
                "upload_id": "image-upload-1",
                "image_url": "https://cdn2.suno.ai/image_image-upload-1.jpeg",
                "uploaded_here": true
            },
            "completed_steps": ["cover_uploaded"],
            "failed": {
                "step": "metadata_update",
                "code": "rate_limited",
                "message": "Rate limited by Suno — wait and retry"
            },
            "recovery": {
                "resumable": true,
                "command": "sunox clip set",
                "arguments": {
                    "clip_id": "clip-a",
                    "image_url": "https://cdn2.suno.ai/image_image-upload-1.jpeg"
                },
                "reuse_original_arguments": true,
                "omit_original_arguments": ["image_file"]
            }
        })
    );
    assert_eq!(
        server.captured().await.path,
        "/api/gen/clip-a/set_metadata/"
    );
}

#[tokio::test]
async fn set_clip_visibility_posts_current_web_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_visibility("clip-a", false)
        .await
        .expect("set visibility");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/clip-a/set_visibility/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "is_public": false,
            "submit_to_contest": false
        })
    );
}
#[tokio::test]
async fn generate_posts_current_web_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"id":"request-1","clip_review_prompt_id":"review-1","protocol_revision":"2026-07","clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let mut generate = GenerateRequest::new("chirp-v4-5", "custom");
    generate.set_challenge_token(Some("captcha-token".into()));
    generate.title = Some("Demo".into());
    generate.tags = Some("pop, upbeat".into());
    generate.prompt = "first line\nsecond line".into();

    let clips = client.generate(&generate).await.expect("generate");

    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, "clip-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json");
    assert_eq!(body["token"], "captcha-token");
    assert_eq!(body["generation_type"], "TEXT");
    assert_eq!(body["mv"], "chirp-v4-5");
    assert_eq!(body["prompt"], "first line\nsecond line");
    assert!(body.get("gpt_description_prompt").is_none());
    assert_eq!(body["token_provider"], 1);
    assert_eq!(body["metadata"]["create_mode"], "custom");
    assert_eq!(body["metadata"]["is_max_mode"], false);
    assert!(body["metadata"].get("lyrics_model").is_none());
    assert_eq!(body["metadata"]["web_client_pathname"], "/create");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert!(
        body["transaction_uuid"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(
        body["metadata"]["create_session_token"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}

#[tokio::test]
async fn prepared_generation_preserves_current_response_envelope() {
    let raw = serde_json::json!({
        "id": null,
        "clip_review_prompt_id": "review-1",
        "protocol_revision": "2026-07",
        "clips": [{
            "id": "clip-1",
            "title": "Demo",
            "status": "submitted",
            "model_name": "chirp-fenix",
            "created_at": "2026-07-27T00:00:00Z"
        }]
    });
    let server = MockServer::json(&raw.to_string()).await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-fenix", "custom");
    generate.metadata.user_tier = "tier-pro".into();
    generate.set_challenge_token(Some("captcha-token".into()));

    let result = client
        .submit_prepared_generation_after_challenge(&generate)
        .await
        .expect("generate");
    let output = serde_json::to_value(result).expect("result json");

    assert_eq!(output, raw);
}

#[tokio::test]
async fn prepared_generation_submits_detected_turnstile_provider_without_second_preflight() {
    let server = MockServer::json(
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    )
    .await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-fenix", "custom");
    generate.metadata.user_tier = "tier-pro".into();
    generate.set_challenge_token_with_provider(
        Some("turnstile-token".into()),
        crate::api::challenge::ChallengeProvider::Turnstile,
    );

    client
        .submit_prepared_generation_after_challenge(&generate)
        .await
        .expect("generate");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json");
    assert_eq!(body["token"], "turnstile-token");
    assert_eq!(body["token_provider"], 2);
}

#[tokio::test]
async fn generate_preserves_existing_user_tier_while_revalidating_the_model() {
    let billing = billing_info_response("replacement-tier");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-v4-5", "custom");
    generate.set_challenge_token(Some("captcha-token".into()));
    generate.metadata.user_tier = "existing-tier".into();

    let clips = client.generate(&generate).await.expect("generate");

    assert_eq!(clips[0].id, "clip-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json");
    assert_eq!(body["metadata"]["user_tier"], "existing-tier");
}

#[tokio::test]
async fn generate_rejects_control_sliders_for_a_model_without_the_web_capability() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-v4-5", "custom");
    generate.metadata.user_tier = "existing-tier".into();
    generate.metadata.control_sliders = Some(ControlSliders {
        weirdness_constraint: Some(0.4),
        style_weight: Some(0.7),
        audio_weight: None,
        aug_creativity: None,
    });
    generate.set_challenge_token(Some("captcha-token".into()));

    let error = client
        .generate(&generate)
        .await
        .expect_err("unsupported controls must stop before generation");

    let requests = server.captured_all().await;
    assert!(matches!(error, CliError::Config(message) if message.contains("does not support")));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn v6_generation_preserves_variety_mumble_and_max_mode_contract() {
    let mut billing = serde_json::from_str::<serde_json::Value>(&billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v6 Pro",
            "external_key": "chirp-hawk",
            "can_use": true,
            "is_default_model": true,
            "description": "v6 fixture",
            "features": ["create_control_sliders", "mumble_mode"],
            "max_lengths": {},
            "major_version": 6
        }]),
    ))
    .expect("billing fixture");
    billing["accessible_features"] = serde_json::json!(["max_mode"]);
    let billing = billing.to_string();
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"flags":{"aug-creativity":true,"mumble-mode":true,"max-mode":true},"roles":{}}"#,
        r#"{"clips":[{"id":"clip-v6","title":"Mumble","status":"submitted","model_name":"chirp-hawk","created_at":"2026-09-11T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-hawk", "custom");
    generate.set_challenge_token(Some("captcha-token".into()));
    generate.metadata.control_sliders = Some(ControlSliders {
        weirdness_constraint: None,
        style_weight: None,
        audio_weight: None,
        aug_creativity: Some(3.0),
    });
    generate.metadata.is_mumble = Some(true);
    generate.metadata.is_max_mode = Some(true);

    client.generate(&generate).await.expect("v6 generation");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].path, "/api/session/");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["mv"], "chirp-hawk");
    assert_eq!(body["metadata"]["control_sliders"]["aug_creativity"], 3.0);
    assert_eq!(body["metadata"]["is_mumble"], true);
    assert_eq!(body["metadata"]["is_max_mode"], true);
}

#[tokio::test]
async fn v6_generation_applies_current_web_variety_defaults() {
    for (external_key, expected) in [("chirp-hawk", 1.0), ("chirp-hawk-wild", 0.0)] {
        let billing = billing_info_with_models(
            "tier-pro",
            serde_json::json!([{
                "name": "v6",
                "external_key": external_key,
                "can_use": true,
                "is_default_model": true,
                "description": "v6 fixture",
                "features": ["create_control_sliders"],
                "max_lengths": {},
                "major_version": 6
            }]),
        );
        let session = r#"{"flags":{"aug-creativity":true},"roles":{}}"#;
        let server = MockServer::json_sequence(&[billing.as_str(), session]).await;
        let client = server.client();
        let mut request = GenerateRequest::new(external_key, "custom");

        client
            .prepare_generation_request(&mut request)
            .await
            .expect("v6 variety default");

        assert_eq!(
            request
                .metadata
                .control_sliders
                .as_ref()
                .and_then(|sliders| sliders.aug_creativity),
            Some(expected),
            "unexpected default for {external_key}"
        );
        assert_eq!(request.duration, Some(180.0));
        let requests = server.captured_all().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].path, "/api/session/");
    }
}

#[tokio::test]
async fn v6_generation_omits_gated_variety_default_when_session_gate_is_absent() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v6",
            "external_key": "chirp-hawk",
            "can_use": true,
            "is_default_model": true,
            "description": "v6 fixture",
            "features": ["create_control_sliders"],
            "max_lengths": {},
            "major_version": 6
        }]),
    );
    let server = MockServer::json_sequence(&[billing.as_str(), r#"{"flags":{},"roles":{}}"#]).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-hawk", "custom");

    client
        .prepare_generation_request(&mut request)
        .await
        .expect("ungated default should be omitted, not reject generation");

    assert!(request.metadata.control_sliders.is_none());
}

#[tokio::test]
async fn explicit_v6_variety_requires_the_live_session_gate() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v6",
            "external_key": "chirp-hawk",
            "can_use": true,
            "is_default_model": true,
            "description": "v6 fixture",
            "features": ["create_control_sliders"],
            "max_lengths": {},
            "major_version": 6
        }]),
    );
    let server = MockServer::json_sequence(&[billing.as_str(), r#"{"flags":{},"roles":{}}"#]).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-hawk", "custom");
    request.metadata.control_sliders = Some(ControlSliders {
        weirdness_constraint: None,
        style_weight: None,
        audio_weight: None,
        aug_creativity: Some(2.0),
    });

    let error = client
        .prepare_generation_request(&mut request)
        .await
        .expect_err("explicit Variety must fail closed without its Web gate");

    assert!(matches!(error, CliError::Config(message) if message.contains("aug-creativity")));
}

#[tokio::test]
async fn v6_generation_controls_fail_closed_on_missing_model_or_account_gates() {
    let v6_without_mumble = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v6 Pro",
            "external_key": "chirp-hawk",
            "can_use": true,
            "is_default_model": true,
            "description": "v6 fixture",
            "features": ["create_control_sliders"],
            "max_lengths": {},
            "major_version": 6
        }]),
    );
    let server = MockServer::json(&v6_without_mumble).await;
    let client = server.client();
    let mut mumble = GenerateRequest::new("chirp-hawk", "custom");
    mumble.metadata.is_mumble = Some(true);
    let error = client
        .prepare_generation_request(&mut mumble)
        .await
        .expect_err("Mumble must require the model feature");
    assert!(matches!(error, CliError::Config(message) if message.contains("Mumble Mode")));
    assert_eq!(server.captured_all().await.len(), 1);

    let v6_with_mumble = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v6 Pro",
            "external_key": "chirp-hawk",
            "can_use": true,
            "is_default_model": true,
            "description": "v6 fixture",
            "features": ["create_control_sliders", "mumble_mode"],
            "max_lengths": {},
            "major_version": 6
        }]),
    );
    let server =
        MockServer::json_sequence(&[v6_with_mumble.as_str(), r#"{"flags":{},"roles":{}}"#]).await;
    let client = server.client();
    let mut mumble = GenerateRequest::new("chirp-hawk", "custom");
    mumble.metadata.is_mumble = Some(true);
    let error = client
        .prepare_generation_request(&mut mumble)
        .await
        .expect_err("Mumble must require the account gate");
    assert!(matches!(error, CliError::Config(message) if message.contains("mumble-mode")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path, "/api/session/");

    let v6_without_entitlement = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v6 Pro",
            "external_key": "chirp-hawk",
            "can_use": true,
            "is_default_model": true,
            "description": "v6 fixture",
            "max_lengths": {},
            "major_version": 6
        }]),
    );
    let server = MockServer::json(&v6_without_entitlement).await;
    let client = server.client();
    let mut max_mode = GenerateRequest::new("chirp-hawk", "custom");
    max_mode.metadata.is_max_mode = Some(true);
    let error = client
        .prepare_generation_request(&mut max_mode)
        .await
        .expect_err("Max Mode must require the account entitlement");
    assert!(matches!(error, CliError::Config(message) if message.contains("max_mode entitlement")));
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn sourced_vox_reference_falls_back_to_current_legacy_task_for_an_older_model() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v4.5+",
            "external_key": "chirp-bluejay",
            "can_use": true,
            "is_default_model": true,
            "description": "legacy Persona capable",
            "capabilities": ["artist_consistency"],
            "allowed_condition_combinations": [["persona"]],
            "max_lengths": {}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-bluejay", "custom");
    request.task = Some("vox".into());
    request.persona_id = Some("persona-1".into());
    request.artist_clip_id = Some("clip-root".into());

    client
        .prepare_generation_request(&mut request)
        .await
        .expect("sourced Vox reference should use legacy task on an older model");

    assert_eq!(request.task.as_deref(), Some("artist_consistency"));
    assert_eq!(request.mv, "chirp-bluejay");
}

#[tokio::test]
async fn rootless_vox_reference_rejects_a_model_without_vox_support() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v4.5+",
            "external_key": "chirp-bluejay",
            "can_use": true,
            "is_default_model": true,
            "description": "legacy Persona capable",
            "capabilities": ["artist_consistency"],
            "allowed_condition_combinations": [["persona"]],
            "max_lengths": {}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-bluejay", "custom");
    request.task = Some("vox".into());
    request.persona_id = Some("persona-1".into());

    let error = client
        .prepare_generation_request(&mut request)
        .await
        .expect_err("rootless Vox must require a Vox-capable model");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("does not support Voice Persona"))
    );
}

#[tokio::test]
async fn generate_rejects_tag_upsample_before_calling_it_for_an_unsupported_model() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-v4-5", "custom");
    generate.metadata.user_tier = "existing-tier".into();

    let error = client
        .prepare_generation_request_with_features(
            &mut generate,
            &[super::generate::TAG_UPSAMPLE_FEATURE],
        )
        .await
        .expect_err("unsupported tag upsample must stop before its endpoint");

    let requests = server.captured_all().await;
    assert!(matches!(error, CliError::Config(message) if message.contains("tag_upsample")));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn generation_challenge_posts_current_web_contract() {
    let server = MockServer::json(r#"{"required":true,"captcha_version":1}"#).await;
    let client = server.client();

    let challenge = client
        .generation_challenge()
        .await
        .expect("generation challenge");

    assert!(challenge.required);
    assert_eq!(challenge.captcha_version, Some(1));
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/c/check");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "ctype": "generation" })
    );
}

#[tokio::test]
async fn prompt_upsample_posts_current_web_contract() {
    let server =
        MockServer::json(r#"{"upsampled":"garage pop, dry drums","request_id":"request-1"}"#).await;
    let client = server.client();

    let response = client
        .upsample_tags(crate::api::types::PromptUpsampleRequest {
            original_tags: "garage pop",
            lyrics: Some("[Verse]\nNeon rain"),
            is_instrumental: false,
            user_guidance: None,
        })
        .await
        .expect("upsample tags");

    assert_eq!(response.upsampled, "garage pop, dry drums");
    assert_eq!(response.request_id, "request-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/prompts/upsample");
    let body = serde_json::from_str::<serde_json::Value>(&request.body).expect("request json");
    assert_eq!(
        body,
        serde_json::json!({
            "original_tags": "garage pop",
            "lyrics": "[Verse]\nNeon rain",
            "is_instrumental": false
        })
    );
}

#[tokio::test]
async fn personalization_settings_preserve_explicit_disabled_styles_augmentation() {
    let server = MockServer::json(r#"{"styles_augmentation":false}"#).await;
    let client = server.client();

    assert!(
        !client
            .styles_augmentation_enabled()
            .await
            .expect("personalization settings")
    );
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/personalization/settings");
}

#[tokio::test]
async fn personalization_settings_default_to_enabled_when_field_is_missing() {
    let server = MockServer::json(r#"{}"#).await;
    let client = server.client();

    assert!(
        client
            .styles_augmentation_enabled()
            .await
            .expect("default personalization settings")
    );
}

#[tokio::test]
async fn inspiration_posts_live_captured_playlist_condition_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"styles_augmentation":false}"#,
        r#"{"upsampled":"dry garage pop, tight drums","request_id":"request-inspire"}"#,
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"clip-inspired","title":"New Song","status":"submitted","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client
        .inspire(InspirationOptions {
            clip_id: "clip-source",
            title: "New Song",
            tags: "garage pop",
            enhance_tags: true,
            negative_tags: "ballad",
            lyrics: "[Verse]\nNew words",
            weirdness: 40.0,
            audio_influence: Some(60.0),
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect("inspiration generation");

    assert_eq!(clips[0].id, "clip-inspired");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/personalization/settings");
    assert_eq!(requests[2].path, "/api/prompts/upsample");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[2].body)
            .expect("upsample request json"),
        serde_json::json!({
            "original_tags": "garage pop",
            "lyrics": "[Verse]\nNew words",
            "is_instrumental": false
        })
    );
    assert_eq!(requests[3].path, "/api/c/check");
    assert_eq!(requests[4].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[4].body).expect("request json");
    assert_eq!(body["task"], "playlist_condition");
    assert_eq!(body["title"], "New Song");
    assert_eq!(body["tags"], "dry garage pop, tight drums");
    assert_eq!(body["negative_tags"], "ballad");
    assert_eq!(body["prompt"], "[Verse]\nNew words");
    assert!(body.get("gpt_description_prompt").is_none());
    assert_eq!(body["metadata"]["create_mode"], "custom");
    assert_eq!(
        body["metadata"]["control_sliders"]["weirdness_constraint"],
        0.4
    );
    assert_eq!(body["metadata"]["control_sliders"]["audio_weight"], 0.6);
    assert_eq!(
        body["metadata"]["last_tags_generation"]["request_id"],
        "request-inspire"
    );
    assert_eq!(
        body["metadata"]["last_tags_generation"]["personalization_enabled"],
        false
    );
    assert_eq!(body["override_fields"], serde_json::json!([]));
    assert_eq!(body["playlist_id"], "inspiration");
    assert_eq!(
        body["playlist_clip_ids"],
        serde_json::json!(["clip-source"])
    );
}

#[tokio::test]
async fn inspiration_preserves_tags_without_explicit_web_enhance_action() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"clip-inspired","title":"New Song","status":"submitted","model_name":"chirp-fenix","created_at":"2026-08-23T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client
        .inspire(InspirationOptions {
            clip_id: "clip-source",
            title: "New Song",
            tags: "garage pop",
            enhance_tags: false,
            negative_tags: "",
            lyrics: "[Verse]\nNew words",
            weirdness: 40.0,
            audio_influence: None,
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect("inspiration generation without tag enhancement");

    assert_eq!(clips[0].id, "clip-inspired");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/c/check");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["tags"], "garage pop");
    assert!(body["metadata"].get("last_tags_generation").is_none());
}

#[tokio::test]
async fn inspiration_revalidates_upsampled_tags_against_the_resolved_model_limit() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v5.5",
            "external_key": "chirp-fenix",
            "can_use": true,
            "is_default_model": true,
            "description": "default test model",
            "capabilities": ["playlist_condition"],
            "allowed_condition_combinations": [["playlist"]],
            "badges": ["custom"],
            "max_lengths": {"tags": 4}
        }]),
    );
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{}"#,
        r#"{"upsampled":"tags are too long","request_id":"request-inspire"}"#,
    ])
    .await;
    let client = server.client();

    let error = client
        .prepare_inspiration_request(InspirationOptions {
            clip_id: "clip-source",
            title: "New Song",
            tags: "pop",
            enhance_tags: true,
            negative_tags: "",
            lyrics: "[Verse]\nNew words",
            weirdness: 40.0,
            audio_influence: None,
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect_err("upsampled tags must still respect the selected model limit");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("generation field `tags`"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/personalization/settings");
    assert_eq!(requests[2].path, "/api/prompts/upsample");
}

#[tokio::test]
async fn clip_wait_retries_ids_that_are_temporarily_missing() {
    let server = MockServer::json_sequence(&[
        r#"null"#,
        r#"{"id":"clip-a","title":"Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
    ])
    .await;
    let client = server.client();
    let ids = vec!["clip-a".to_string()];

    let clips = crate::workflow::tasks::wait_for_clips(&client, &ids, 3, 1)
        .await
        .expect("temporarily missing clip should become visible");

    assert_eq!(clips[0].id, "clip-a");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn clip_wait_does_not_poll_again_after_its_deadline() {
    let server = MockServer::json_until_idle(
        r#"{"id":"clip-a","title":"Song","status":"processing","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
        3,
    )
    .await;
    let client = server.client();
    let ids = vec!["clip-a".to_string()];

    let error = crate::workflow::tasks::wait_for_clips(&client, &ids, 1, 5)
        .await
        .expect_err("wait must stop at its deadline");

    assert!(matches!(error, CliError::GenerationFailed(message) if message.contains("timed out")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1, "deadline must prevent a second request");
}

#[tokio::test]
async fn clip_wait_deadline_bounds_an_in_flight_request() {
    let server = MockServer::delayed_json(r#"[]"#, Duration::from_secs(2)).await;
    let client = server.client();
    let ids = vec!["clip-a".to_string()];

    let error = timeout(
        Duration::from_millis(1200),
        crate::workflow::tasks::wait_for_clips(&client, &ids, 1, 1),
    )
    .await
    .expect("configured deadline must bound the in-flight request")
    .expect_err("delayed request must time out");

    assert!(matches!(error, CliError::GenerationFailed(message) if message.contains("timed out")));
}

#[tokio::test]
async fn generate_without_token_preflights_then_submits_when_challenge_is_not_required() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();
    let generate = GenerateRequest::new("chirp-fenix", "custom");

    let clips = client.generate(&generate).await.expect("generate");

    assert_eq!(clips[0].id, "clip-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json"),
        serde_json::json!({ "ctype": "generation" })
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/c/check");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert!(body["token"].is_null());
    assert!(body["token_provider"].is_null());
}

#[tokio::test]
async fn generate_does_not_fallback_across_a_billing_server_error() {
    let server =
        MockServer::json_status_sequence(&[(500, r#"{"detail":"billing unavailable"}"#)]).await;
    let client = server.client();
    let generate = GenerateRequest::new("auto", "custom");

    let error = client
        .generate(&generate)
        .await
        .expect_err("billing server errors must not change the selected model");

    assert!(matches!(error, CliError::SunoApi { status: 500, .. }));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn generate_preserves_a_billing_server_error_when_controls_were_requested() {
    let server =
        MockServer::json_status_sequence(&[(500, r#"{"detail":"billing unavailable"}"#)]).await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-fenix", "custom");
    generate.metadata.control_sliders = Some(ControlSliders {
        weirdness_constraint: Some(0.4),
        style_weight: None,
        audio_weight: None,
        aug_creativity: None,
    });

    let error = client
        .generate(&generate)
        .await
        .expect_err("billing server error must stop generation");

    assert!(matches!(error, CliError::SunoApi { status: 500, .. }));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn generate_does_not_fallback_across_billing_schema_drift() {
    let server = MockServer::json(r#"{"credits":"not-a-number"}"#).await;
    let client = server.client();
    let generate = GenerateRequest::new("auto", "custom");

    let error = client
        .generate(&generate)
        .await
        .expect_err("malformed billing JSON must not silently select another model");

    assert!(matches!(error, CliError::Http(error) if error.is_decode()));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn auto_model_fails_closed_before_generation_when_billing_transport_is_unavailable() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve unused port");
    let address = listener.local_addr().expect("unused address");
    drop(listener);
    let client = SunoClient::new_for_tests(
        format!("http://{address}"),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let generate = GenerateRequest::new("auto", "custom");

    let error = client
        .generate(&generate)
        .await
        .expect_err("billing transport failure must stop before generation");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("billing info") && message.contains("refusing to submit"))
    );
}

#[tokio::test]
async fn auto_model_with_duration_fails_closed_during_a_billing_transport_outage() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve unused port");
    let address = listener.local_addr().expect("unused address");
    drop(listener);
    let client = SunoClient::new_for_tests(
        format!("http://{address}"),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let mut generate = GenerateRequest::new("auto", "custom");
    generate.duration = Some(120.0);

    let error = client
        .prepare_generation_request(&mut generate)
        .await
        .expect_err("duration requires an exactly validated v5.5 account model");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("duration") && message.contains("billing"))
    );
    assert_eq!(generate.mv, "auto");
}

#[tokio::test]
async fn explicit_model_selector_fails_closed_during_a_billing_transport_outage() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve unused port");
    let address = listener.local_addr().expect("unused address");
    drop(listener);
    let client = SunoClient::new_for_tests(
        format!("http://{address}"),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let mut generate = GenerateRequest::new("account-model-7", "custom");

    let error = client
        .prepare_generation_request(&mut generate)
        .await
        .expect_err("explicit selectors require exact account validation");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("exact billing validation"))
    );
    assert_eq!(generate.mv, "account-model-7");
}

#[tokio::test]
async fn cover_fails_closed_when_billing_transport_cannot_validate_and_map_the_base_model() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve unused port");
    let address = listener.local_addr().expect("unused address");
    drop(listener);
    let client = SunoClient::new_for_tests(
        format!("http://{address}"),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let mut request = GenerateRequest::new("chirp-v4", "simple");
    request.task = Some("cover".into());

    let error = client
        .prepare_generation_request(&mut request)
        .await
        .expect_err("Cover must not submit an unmapped model after a billing outage");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("validating and mapping its model"))
    );
    assert_eq!(
        request.mv, "chirp-v4",
        "the request must remain unprepared and must never reach submission"
    );
}

#[tokio::test]
async fn generate_does_not_guess_when_billing_succeeds_with_no_models() {
    let billing = billing_info_with_models("tier-pro", serde_json::json!([]));
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let generate = GenerateRequest::new("auto", "custom");

    let error = client
        .generate(&generate)
        .await
        .expect_err("a successful empty capability response must not select a guessed model");

    assert!(matches!(error, CliError::Config(message) if message.contains("no generation models")));
    let request = server.captured().await;
    assert_eq!(request.path, "/api/billing/info/");
}

#[tokio::test]
async fn generate_propagates_rate_limit_from_billing_without_submitting() {
    let server = MockServer::json_status_sequence(&[(429, "")]).await;
    let client = server.client();
    let generate = GenerateRequest::new("chirp-fenix", "custom");

    let error = client
        .generate(&generate)
        .await
        .expect_err("billing rate limit must stop generation");

    assert!(matches!(error, CliError::RateLimited));
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/billing/info/");
}

#[tokio::test]
async fn generation_challenge_invalid_token_stays_a_structured_api_error() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_status_sequence(&[
        (200, billing.as_str()),
        (403, r#"{"detail":"invalid token"}"#),
    ])
    .await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-fenix", "custom");
    generate.metadata.user_tier = "tier-pro".into();
    generate.set_challenge_token(Some("challenge-token".into()));

    let error = client
        .generate(&generate)
        .await
        .expect_err("challenge-token failure must not trigger JWT refresh");

    assert!(matches!(
        error,
        CliError::SunoApi {
            code: "forbidden",
            status: 403,
            ..
        }
    ));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/generate/v2-web/");
}

#[tokio::test]
async fn generate_auto_model_uses_the_accounts_usable_default() {
    let billing = billing_info_with_models(
        "tier-free",
        serde_json::json!([
            {
                "name": "v5.5",
                "external_key": "chirp-fenix",
                "can_use": false,
                "is_default_model": false,
                "description": "paid",
                "max_lengths": {}
            },
            {
                "name": "v4.5-all",
                "external_key": "chirp-auk-turbo",
                "can_use": true,
                "is_default_model": true,
                "description": "free",
                "max_lengths": {"title":100,"prompt":5000,"tags":1000,"negative_tags":1000,"gpt_description_prompt":3000}
            }
        ]),
    );
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-auk-turbo","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();
    let generate = GenerateRequest::new("auto", "custom");

    client.generate(&generate).await.expect("generate");

    let requests = server.captured_all().await;
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/c/check");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["mv"], "chirp-auk-turbo");
    assert_eq!(body["metadata"]["user_tier"], "tier-free");
}

#[tokio::test]
async fn generation_preflight_resolves_account_model_id_to_external_key() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "id": "account-model-7",
            "name": "My Custom Model",
            "external_key": "chirp-custom-7",
            "can_use": true,
            "is_default_model": false,
            "description": "custom account model",
            "capabilities": ["all"],
            "badges": ["custom"],
            "max_lengths": {}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("account-model-7", "custom");

    client
        .prepare_generation_request(&mut request)
        .await
        .expect("account model ID");

    assert_eq!(request.mv, "chirp-custom-7");
    assert_eq!(server.captured().await.path, "/api/billing/info/");
}

#[tokio::test]
async fn cover_preflight_resolves_account_model_id_before_reference_mapping() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "id": "account-cover-model",
            "name": "Account Cover Model",
            "external_key": "chirp-v4",
            "can_use": true,
            "is_default_model": false,
            "description": "cover account model",
            "capabilities": ["cover"],
            "allowed_condition_combinations": [["cover"]],
            "max_lengths": {}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("account-cover-model", "simple");
    request.task = Some("cover".into());

    client
        .prepare_generation_request(&mut request)
        .await
        .expect("cover account model ID");

    assert_eq!(request.mv, "chirp-v4-tau");
    assert_eq!(server.captured().await.path, "/api/billing/info/");
}

#[tokio::test]
async fn generation_preflight_validates_v55_duration_against_account_limit() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v5.5",
            "external_key": "chirp-fenix",
            "can_use": true,
            "is_default_model": true,
            "description": "v5.5",
            "capabilities": ["all"],
            "badges": ["custom"],
            "max_lengths": {"duration": 480}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("V5.5", "simple");
    request.duration = Some(481.0);

    let error = client
        .prepare_generation_request(&mut request)
        .await
        .expect_err("duration over account limit");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("exceeds the current account limit"))
    );
    assert_eq!(server.captured().await.path, "/api/billing/info/");
}

#[tokio::test]
async fn generate_without_token_stops_when_challenge_is_required() {
    let billing = billing_info_response("tier-pro");
    let server =
        MockServer::json_sequence(&[billing.as_str(), r#"{"required":true,"captcha_version":1}"#])
            .await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-fenix", "custom");
    generate.metadata.user_tier = "tier-pro".into();

    let err = client
        .generate(&generate)
        .await
        .expect_err("challenge error");

    assert!(err.to_string().contains("generation challenge"));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/c/check");
}

#[tokio::test]
async fn cover_posts_generate_v2_cover_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source title","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"cover-1","title":"Cover","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client
        .cover("clip-a", "chirp-fenix", Some("pop"), None)
        .await
        .expect("cover");

    assert_eq!(clips[0].id, "cover-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/c/check");
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("request json");
    assert_eq!(body["mv"], "chirp-fenix");
    assert_eq!(body["title"], "Source title");
    assert_eq!(body["tags"], "pop");
    assert_eq!(body["cover_clip_id"], "clip-a");
    assert_eq!(body["task"], "cover");
    assert_eq!(body["generation_type"], "SIMPLE_REMIX");
    assert_eq!(body["metadata"]["create_mode"], "simple");
    assert_eq!(body["metadata"]["is_remix"], true);
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
}

#[tokio::test]
async fn cover_rejects_an_unavailable_legacy_base_model_before_tau_mapping() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v4",
            "external_key": "chirp-v4",
            "can_use": false,
            "is_default_model": false,
            "description": "unavailable legacy model",
            "max_lengths": {}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-v4", "simple");
    request.task = Some("cover".into());

    let error = client
        .prepare_generation_request(&mut request)
        .await
        .expect_err("unavailable base model must not be hidden by tau mapping");

    assert!(matches!(error, CliError::Config(message) if message.contains("cannot use")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn cover_rejects_a_usable_model_that_no_longer_supports_cover() {
    let billing = billing_info_with_models(
        "tier-pro",
        serde_json::json!([{
            "name": "v3",
            "external_key": "chirp-v3-0",
            "can_use": true,
            "is_default_model": false,
            "description": "generation and extend only",
            "capabilities": ["generate", "extend"],
            "allowed_condition_combinations": [["extend"]],
            "max_lengths": {}
        }]),
    );
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-v3-0", "simple");
    request.task = Some("cover".into());

    let error = client
        .prepare_generation_request(&mut request)
        .await
        .expect_err("current Web-incompatible Cover model must be rejected");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("does not support Cover"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn cover_with_challenge_token_posts_generate_without_preflight_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source title","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}"#,
        billing.as_str(),
        r#"{"clips":[{"id":"cover-1","title":"Cover","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client
        .cover(
            "clip-a",
            "chirp-fenix",
            Some("pop"),
            Some("captcha-token".into()),
        )
        .await
        .expect("cover");

    assert_eq!(clips[0].id, "cover-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["title"], "Source title");
    assert_eq!(body["cover_clip_id"], "clip-a");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert_eq!(body["token"], "captcha-token");
    assert_eq!(body["token_provider"], 1);
}

#[tokio::test]
async fn remaster_posts_generate_v2_remaster_contract() {
    let source = serde_json::json!({
        "id": "clip-a",
        "title": "Source",
        "status": "complete",
        "model_name": "chirp-carp",
        "created_at": "2026-06-30T00:00:00Z",
        "is_trashed": false,
        "metadata": {"duration": 180.0, "infill": false},
        "action_config": {"actions": [{
            "action_type": "remaster",
            "visible": true,
            "disabled": false
        }]}
    });
    let raw = serde_json::json!({
        "clips": [{
            "id": "remaster-1",
            "title": "Remaster",
            "status": "submitted",
            "model_name": "chirp-flounder",
            "created_at": "2026-06-30T00:00:00Z"
        }],
        "batch_size": 1,
        "status": "submitted",
        "upstream_metadata": {"request_id": "remaster-request-1"}
    });
    let source_body = source.to_string();
    let raw_body = raw.to_string();
    let server = MockServer::json_sequence(&[&source_body, &raw_body]).await;
    let client = server.client();

    let result = client
        .remaster(
            "clip-a",
            "chirp-flounder",
            Some(crate::api::types::RemasterVariation::High),
        )
        .await
        .expect("remaster");

    assert_eq!(result.clips[0].id, "remaster-1");
    assert_eq!(
        serde_json::to_value(&result).expect("serialize remaster result"),
        raw,
        "remaster must preserve the exact upstream response envelope"
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/generate/upsample");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "model_name": "chirp-flounder",
            "variation_category": "high"
        })
    );
}

#[tokio::test]
async fn remaster_default_variation_posts_normal() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-06-30T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let generated = r#"{"clips":[{"id":"remaster-1","title":"Remaster","status":"submitted","model_name":"chirp-flounder","created_at":"2026-06-30T00:00:00Z"}]}"#;
    let server = MockServer::json_sequence(&[source, generated]).await;
    let client = server.client();

    client
        .remaster("clip-a", "chirp-flounder", None)
        .await
        .expect("remaster");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let body = serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json");
    assert_eq!(body["variation_category"], "normal");
}

#[tokio::test]
async fn v6_remaster_posts_current_web_defaults() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-hawk","created_at":"2026-09-11T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let generated = r#"{"clips":[{"id":"remaster-v6","title":"Remaster","status":"submitted","model_name":"chirp-halibut","created_at":"2026-09-11T00:00:00Z"}]}"#;
    let server = MockServer::json_sequence(&[source, generated]).await;
    let client = server.client();

    client
        .remaster("clip-a", "chirp-halibut", None)
        .await
        .expect("v6 remaster");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path, "/api/generate/upsample");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "model_name": "chirp-halibut",
            "variation_category": "normal",
            "style_profile": "boost"
        })
    );
}

#[tokio::test]
async fn v6_remaster_posts_explicit_variation_and_style_profile() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-hawk","created_at":"2026-09-11T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let generated = r#"{"clips":[{"id":"remaster-v6","title":"Remaster","status":"submitted","model_name":"chirp-halibut","created_at":"2026-09-11T00:00:00Z"}]}"#;
    let server = MockServer::json_sequence(&[source, generated]).await;
    let client = server.client();

    client
        .remaster_with_options(
            "clip-a",
            "chirp-halibut",
            RemasterOptions {
                variation: Some(crate::api::types::RemasterVariation::High),
                style_profile: Some(crate::api::types::RemasterStyleProfile::Clarity),
            },
        )
        .await
        .expect("v6 explicit Remaster controls");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path, "/api/generate/upsample");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "model_name": "chirp-halibut",
            "variation_category": "high",
            "style_profile": "clarity"
        })
    );
}

#[tokio::test]
async fn remaster_v45_plus_omits_variation_category() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-06-30T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let generated = r#"{"clips":[{"id":"remaster-1","title":"Remaster","status":"submitted","model_name":"chirp-bass","created_at":"2026-06-30T00:00:00Z"}]}"#;
    let server = MockServer::json_sequence(&[source, generated]).await;
    let client = server.client();

    client
        .remaster("clip-a", "chirp-bass", None)
        .await
        .expect("v4.5+ remaster");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let body = serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json");
    assert!(body.get("variation_category").is_none());
}

#[tokio::test]
async fn remaster_fails_closed_when_the_detail_route_returns_a_different_clip() {
    let server = MockServer::json(
        r#"{"id":"clip-b","title":"Other","status":"complete","model_name":"chirp-carp","created_at":"2026-06-30T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#,
    )
    .await;
    let client = server.client();

    let error = client
        .remaster("clip-a", "chirp-flounder", None)
        .await
        .expect_err("mismatched source must not be submitted");

    assert!(matches!(error, CliError::NotFound(_)));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
}

#[tokio::test]
async fn concat_posts_current_web_contract() {
    let raw = serde_json::json!({
        "id": "concat-1",
        "title": "Concat",
        "status": "submitted",
        "model_name": "chirp-fenix",
        "created_at": "2026-06-30T00:00:00Z",
        "upstream_metadata": {"request_id": "concat-request-1"}
    });
    let server = MockServer::json(&raw.to_string()).await;
    let client = server.client();

    let result = client.concat("clip-a").await.expect("concat");

    assert_eq!(result.clips[0].id, "concat-1");
    assert_eq!(
        serde_json::to_value(&result).expect("serialize concat result"),
        raw,
        "concat must preserve the exact upstream bare-clip response"
    );
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/generate/concat/v2/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "is_infill": false
        })
    );
}

#[tokio::test]
async fn speed_adjust_posts_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"speed-1","title":"Song (0.94x)","status":"processing","model_name":"chirp-fenix","audio_url":"https://cdn.example/speed-1.mp3","created_at":"2026-06-30T00:00:00Z"}"#,
    )
    .await;
    let client = server.client();

    let clip = client
        .adjust_speed("clip-a", 0.9439, true, "Song (0.94x)")
        .await
        .expect("adjust speed");

    assert_eq!(clip.id, "speed-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/clips/adjust-speed/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "speed_multiplier": 0.9439,
            "keep_pitch": true,
            "title": "Song (0.94x)"
        })
    );
}

#[tokio::test]
async fn reverse_posts_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"reverse-1","title":"Song (Reversed)","status":"processing","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
    )
    .await;
    let client = server.client();

    let clip = client
        .reverse_clip("clip-a", "Song (Reversed)")
        .await
        .expect("reverse");

    assert_eq!(clip.id, "reverse-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/clips/reverse-clip/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "title": "Song (Reversed)"
        })
    );
}

#[tokio::test]
async fn crop_retries_result_lookup_after_action_completes() {
    let server = MockServer::json_sequence(&[
        r#"{"action_clip_id":"crop-1"}"#,
        r#"{"status":"complete"}"#,
        r#"null"#,
        r#"{"id":"crop-1","title":"Song (Crop)","status":"complete","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
    ])
    .await;
    let client = server.client();

    let clip = client
        .crop_clip(
            "clip-a",
            12.5,
            64.25,
            false,
            "Song (Crop)",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("crop");

    assert_eq!(clip.id, "crop-1");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/edit/crop/clip-a/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json"),
        serde_json::json!({
            "crop_start_s": 12.5,
            "crop_end_s": 64.25,
            "is_crop_remove": false,
            "title": "Song (Crop)",
            "ui_surface": "song_actions"
        })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/edit/action/crop-1/");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/clip/crop-1");
    assert_eq!(requests[3].path, "/api/clip/crop-1");
}

#[tokio::test]
async fn crop_waits_for_the_result_clip_to_complete() {
    let server = MockServer::json_sequence(&[
        r#"{"action_clip_id":"crop-1"}"#,
        r#"{"status":"complete"}"#,
        r#"{"id":"crop-1","title":"Song (Crop)","status":"processing","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
        r#"{"id":"crop-1","title":"Song (Crop)","status":"complete","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
    ])
    .await;
    let client = server.client();

    let clip = client
        .crop_clip(
            "clip-a",
            12.5,
            64.25,
            false,
            "Song (Crop)",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("crop result should finish");

    assert_eq!(clip.status, "complete");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[1].path, "/api/edit/action/crop-1/");
    assert_eq!(requests[2].path, "/api/clip/crop-1");
    assert_eq!(requests[3].path, "/api/clip/crop-1");
}

#[tokio::test]
async fn crop_deadline_after_submit_is_ambiguous() {
    let server = MockServer::delayed_response_sequence(vec![
        (
            200,
            r#"{"action_clip_id":"crop-1"}"#.to_string(),
            Duration::ZERO,
        ),
        (
            200,
            r#"{"status":"processing"}"#.to_string(),
            Duration::from_millis(200),
        ),
    ])
    .await;
    let client = server.client();

    let error = timeout(
        Duration::from_millis(50),
        client.crop_clip(
            "clip-a",
            12.5,
            64.25,
            false,
            "Song (Crop)",
            super::PollingOptions {
                timeout: Duration::from_millis(10),
                interval: Duration::from_millis(1),
            },
        ),
    )
    .await
    .expect("configured deadline must bound the action request")
    .expect_err("delayed action request must time out");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "crop");
    assert_eq!(details["action_clip_id"], "crop-1");
    assert_eq!(details["stage"], "action_poll");
    assert_eq!(details["cause"]["code"], "generation_failed");
}

#[tokio::test]
async fn crop_rejects_invalid_polling_before_submitting_an_edit() {
    let server = MockServer::json_until_idle(r#"{"action_clip_id":"action-1"}"#, 1).await;
    let client = server.client();

    let error = client
        .crop_clip(
            "clip-a",
            1.0,
            2.0,
            false,
            "Crop",
            super::PollingOptions {
                timeout: Duration::ZERO,
                interval: Duration::from_secs(1),
            },
        )
        .await
        .expect_err("invalid polling must fail before edit submission");

    assert!(matches!(error, CliError::Config(message) if message.contains("poll timeout")));
    assert!(server.captured_all().await.is_empty());
}

#[tokio::test]
async fn fade_reports_a_failed_result_clip() {
    let server = MockServer::json_sequence(&[
        r#"{"action_clip_id":"fade-1"}"#,
        r#"{"status":"complete"}"#,
        r#"{"id":"fade-1","title":"Song (Fade In)","status":"error","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
    ])
    .await;
    let client = server.client();

    let error = client
        .fade_clip(
            "clip-a",
            Some(4.0),
            None,
            "Song (Fade In)",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a failed result clip must fail the edit");

    assert!(matches!(error, CliError::GenerationFailed(message) if message.contains("fade-1")));
}

#[tokio::test]
async fn fade_waits_for_action_status_before_loading_clip() {
    let server = MockServer::json_sequence(&[
        r#"{"action_clip_id":"fade-1"}"#,
        r#"{"status":"complete"}"#,
        r#"{"id":"fade-1","title":"Song (Fade In)","status":"complete","model_name":"chirp-fenix","created_at":"2026-07-10T00:00:00Z"}"#,
    ])
    .await;
    let client = server.client();

    let clip = client
        .fade_clip(
            "clip-a",
            Some(4.0),
            None,
            "Song (Fade In)",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("fade");

    assert_eq!(clip.id, "fade-1");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/edit/fade/clip-a/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json"),
        serde_json::json!({
            "fade_in_time": 4.0,
            "title": "Song (Fade In)"
        })
    );
    assert_eq!(requests[1].path, "/api/edit/action/fade-1/");
    assert_eq!(requests[2].path, "/api/clip/fade-1");
}

#[tokio::test]
async fn official_download_resolves_mp3_download_url() {
    let server =
        MockServer::json(r#"{"status":"complete","download_url":"https://cdn.example/song.mp3"}"#)
            .await;
    let client = server.client();

    let url = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Mp3,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("download url");

    assert_eq!(url, "https://cdn.example/song.mp3");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/download/clip/clip-a?format=mp3");
}

#[tokio::test]
async fn prepared_download_supports_the_current_format_route_matrix() {
    use super::download::PreparedDownloadFormat;

    for (format, extension) in [
        (PreparedDownloadFormat::Mp3, "mp3"),
        (PreparedDownloadFormat::M4a, "m4a"),
        (PreparedDownloadFormat::Wav, "wav"),
        (PreparedDownloadFormat::Mp4, "mp4"),
    ] {
        let expected_url = format!("https://cdn.example/song.{extension}");
        let response = serde_json::json!({
            "status": "complete",
            "download_url": expected_url
        })
        .to_string();
        let server = MockServer::json(&response).await;

        let url = server
            .client()
            .prepared_download_url(
                "clip-a",
                format,
                super::PollingOptions {
                    timeout: Duration::from_secs(1),
                    interval: Duration::from_millis(1),
                },
            )
            .await
            .expect("prepared download URL");

        assert_eq!(url, expected_url);
        let request = server.captured().await;
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.path,
            format!("/api/download/clip/clip-a?format={extension}")
        );
    }
}

#[tokio::test]
async fn prepared_download_without_a_url_returns_a_typed_unavailable_error() {
    let server = MockServer::json(r#"{"status":"complete","download_url":null}"#).await;

    let error = server
        .client()
        .prepared_download_url(
            "clip-a",
            super::download::PreparedDownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a terminal prepared response without a URL is unavailable");

    assert_eq!(error.error_code(), "prepared_download_unavailable");
    let details = error.details().expect("prepared unavailable details");
    assert_eq!(details["clip_id"], "clip-a");
    assert_eq!(details["format"], "wav");
    assert_eq!(details["download_started"], false);
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn prepared_download_rejects_a_blank_url_as_unavailable() {
    let server = MockServer::json(r#"{"status":"complete","download_url":"  "}"#).await;

    let error = server
        .client()
        .prepared_download_url(
            "clip-a",
            super::download::PreparedDownloadFormat::Mp4,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a blank prepared URL is not a usable file location");

    assert_eq!(error.error_code(), "prepared_download_unavailable");
    assert_eq!(error.details().expect("details")["format"], "mp4");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_posts_the_exact_clip_contract_once() {
    let server = MockServer::json(
        r#"{"ok":true,"reason":"subscription","message":"Unlocked","credit_deducted":true,"future_field":"kept"}"#,
    )
    .await;
    let client = server.client();

    let authorization = client
        .authorize_download("clip-a")
        .await
        .expect("download authorization");

    assert_eq!(authorization.ok, Some(true));
    assert_eq!(authorization.reason.as_deref(), Some("subscription"));
    assert_eq!(authorization.message.as_deref(), Some("Unlocked"));
    assert_eq!(authorization.credit_deducted, Some(true));
    assert_eq!(authorization.extra["future_field"], "kept");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/download/authorize");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("authorization body"),
        serde_json::json!({"item_id": "clip-a", "item_type": "clip"})
    );
}

#[tokio::test]
async fn download_authorize_never_follows_a_redirect_with_a_second_post() {
    let target_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind redirect target");
    let target_addr = target_listener
        .local_addr()
        .expect("redirect target address");
    let redirect_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind redirect source");
    let redirect_addr = redirect_listener
        .local_addr()
        .expect("redirect source address");

    let redirect_task = tokio::spawn(async move {
        let (mut stream, _) = redirect_listener
            .accept()
            .await
            .expect("accept redirect POST");
        let request = read_request(&mut stream).await;
        let response = format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{target_addr}/api/download/authorize\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write redirect response");
        request
    });
    let target_task = tokio::spawn(async move {
        let Ok(Ok((stream, _))) =
            timeout(Duration::from_millis(250), target_listener.accept()).await
        else {
            return None;
        };
        Some(capture_request(stream, r#"{"ok":true}"#).await)
    });

    let client = SunoClient::new_for_tests(
        format!("http://{redirect_addr}"),
        AuthState {
            jwt: Some(test_jwt_with_subject("user-1")),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let result = client.authorize_download("clip-a").await;
    let redirect_request = redirect_task.await.expect("redirect source task");
    let redirected_request = target_task.await.expect("redirect target task");

    assert_eq!(redirect_request.method, "POST");
    assert_eq!(redirect_request.path, "/api/download/authorize");
    let error = result.expect_err("a redirect cannot prove the authorization outcome");
    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("redirect ambiguity details");
    assert_eq!(details["stage"], "response_status");
    assert!(
        details["cause"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("307"))
    );
    assert!(
        redirected_request.is_none(),
        "307/308 handling must never replay download authorization"
    );
}

#[tokio::test]
async fn download_authorize_preserves_an_explicit_business_rejection() {
    let server = MockServer::json(
        r#"{"ok":false,"reason":"quota_exhausted","message":"No downloads remaining","credit_deducted":false}"#,
    )
    .await;

    let response = server
        .client()
        .authorize_download("clip-a")
        .await
        .expect("an explicit business rejection is a reliable response");

    assert_eq!(response.ok, Some(false));
    assert_eq!(response.reason.as_deref(), Some("quota_exhausted"));
    assert_eq!(response.message.as_deref(), Some("No downloads remaining"));
    assert_eq!(response.credit_deducted, Some(false));
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_server_error_is_ambiguous_and_never_replayed() {
    let server = MockServer::response_sequence_with_idle_timeout(
        vec![
            (500, r#"{"detail":"authorization outcome unknown"}"#.into()),
            (200, r#"{"ok":true}"#.into()),
        ],
        Duration::from_millis(50),
    )
    .await;

    let error = server
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("5xx cannot prove authorization was rejected");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("authorization ambiguity details");
    assert_eq!(details["operation"], "download_authorize");
    assert_eq!(details["clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_status");
    assert_eq!(details["recovery"]["resumable"], false);
    assert_eq!(
        details["recovery"]["inspection_commands"],
        serde_json::json!(["sunox clip info clip-a --json", "sunox credits --json"])
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_malformed_success_body_is_ambiguous() {
    let server = MockServer::json("{").await;

    let error = server
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("a malformed 2xx body cannot prove authorization outcome");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("authorization ambiguity details");
    assert_eq!(details["operation"], "download_authorize");
    assert_eq!(details["clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_body");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_missing_ok_is_ambiguous_schema_drift() {
    let server = MockServer::json(r#"{"message":"outcome omitted"}"#).await;

    let error = server
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("a 2xx response without ok cannot prove authorization outcome");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("authorization ambiguity details");
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_invalid_ok_type_is_ambiguous_schema_drift() {
    let server = MockServer::json(r#"{"ok":"yes"}"#).await;

    let error = server
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("a non-boolean ok field cannot prove authorization outcome");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("authorization ambiguity details");
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_explicit_client_rejections_are_not_replayed_or_ambiguous() {
    let unauthorized = MockServer::response_sequence_with_idle_timeout(
        vec![(401, String::new()), (200, r#"{"ok":true}"#.into())],
        Duration::from_millis(50),
    )
    .await;
    let error = unauthorized
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("an explicit 401 must be returned without replay");
    assert!(matches!(error, CliError::AuthExpired));
    assert_eq!(unauthorized.captured_all().await.len(), 1);

    let quota_rejection = MockServer::json_status_sequence(&[(
        403,
        r#"{"detail":"No downloads remaining","reason":"quota_exhausted","message":"Upgrade or add downloads"}"#,
    )])
    .await;
    let error = quota_rejection
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("an explicit quota rejection is not ambiguous");
    assert_eq!(error.error_code(), "forbidden");
    let details = error.details().expect("structured rejection details");
    assert_eq!(details["reason"], "quota_exhausted");
    assert_eq!(details["message"], "Upgrade or add downloads");
    assert_eq!(quota_rejection.captured_all().await.len(), 1);
}

#[tokio::test]
async fn download_authorize_send_reset_is_ambiguous_and_not_replayed() {
    let server = MockServer::resets_until_idle(2, Duration::from_millis(50)).await;

    let error = server
        .client()
        .authorize_download("clip-a")
        .await
        .expect_err("a reset after submit leaves authorization outcome unknown");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("authorization ambiguity details");
    assert_eq!(details["stage"], "request_send");
    assert_eq!(details["cause"]["code"], "http_error");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn official_download_rejects_a_zero_poll_timeout() {
    let server = MockServer::json(r#"{"status":"processing","download_url":null}"#).await;
    let client = server.client();

    let error = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Mp3,
            super::PollingOptions {
                timeout: Duration::ZERO,
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("zero timeout must stop a processing download");

    assert!(matches!(error, CliError::Config(message) if message.contains("greater than 0")));
}

#[tokio::test]
async fn official_download_does_not_request_again_after_its_deadline() {
    let server =
        MockServer::json_until_idle(r#"{"status":"processing","download_url":null}"#, 4).await;
    let client = server.client();

    let error = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Mp3,
            super::PollingOptions {
                timeout: Duration::from_millis(100),
                interval: Duration::from_secs(1),
            },
        )
        .await
        .expect_err("polling must stop at the configured deadline");

    assert!(matches!(error, CliError::Download(message) if message.contains("timed out")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1, "deadline must prevent a second request");
}

#[tokio::test]
async fn official_download_deadline_bounds_an_in_flight_request() {
    let server = MockServer::delayed_json(
        r#"{"status":"processing","download_url":null}"#,
        Duration::from_millis(200),
    )
    .await;
    let client = server.client();

    let error = timeout(
        Duration::from_millis(50),
        client.download_url(
            "clip-a",
            super::download::DownloadFormat::Mp3,
            super::PollingOptions {
                timeout: Duration::from_millis(10),
                interval: Duration::from_millis(1),
            },
        ),
    )
    .await
    .expect("configured deadline must bound the download request")
    .expect_err("delayed download request must time out");

    assert!(matches!(error, CliError::Download(message) if message.contains("timed out")));
}

#[tokio::test]
async fn official_download_polls_m4a_download_url() {
    let server = MockServer::json_sequence(&[
        r#"{"status":"processing","download_url":null}"#,
        r#"{"status":"complete","download_url":"https://cdn.example/song.m4a"}"#,
    ])
    .await;
    let client = server.client();

    let url = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::M4a,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("m4a url");

    assert_eq!(url, "https://cdn.example/song.m4a");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/download/clip/clip-a?format=m4a");
    assert_eq!(requests[1].path, "/api/download/clip/clip-a?format=m4a");
}

#[tokio::test]
async fn wav_download_uses_existing_file_url_without_conversion() {
    let server = MockServer::json(r#"{"wav_file_url":"https://cdn.example/song.wav"}"#).await;
    let client = server.client();

    let url = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("wav url");

    assert_eq!(url, "https://cdn.example/song.wav");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/wav_file/");
}

#[tokio::test]
async fn wav_download_posts_convert_when_file_url_is_missing() {
    let server = MockServer::json_sequence(&[
        r#"{"wav_file_url":null}"#,
        r#"{"ok":true}"#,
        r#"{"wav_file_url":null}"#,
        r#"{"wav_file_url":"https://cdn.example/song.wav"}"#,
    ])
    .await;
    let client = server.client();

    let url = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("wav url");

    assert_eq!(url, "https://cdn.example/song.wav");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/gen/clip-a/wav_file/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/gen/clip-a/convert_wav/");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/gen/clip-a/wav_file/");
    assert_eq!(requests[3].path, "/api/gen/clip-a/wav_file/");
}

#[tokio::test]
async fn opus_download_uses_existing_file_url_without_conversion() {
    let server = MockServer::json(r#"{"opus_file_url":"https://cdn.example/song.opus"}"#).await;
    let client = server.client();

    let url = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Opus,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("opus url");

    assert_eq!(url, "https://cdn.example/song.opus");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/opus_file/");
}

#[tokio::test]
async fn opus_download_posts_convert_when_file_url_is_missing() {
    let server = MockServer::json_sequence(&[
        r#"{"opus_file_url":null}"#,
        r#"{"ok":true}"#,
        r#"{"opus_file_url":"https://cdn.example/song.opus"}"#,
    ])
    .await;
    let client = server.client();

    let url = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Opus,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("opus url");

    assert_eq!(url, "https://cdn.example/song.opus");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/gen/clip-a/opus_file/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/gen/clip-a/convert_opus");
    assert_eq!(requests[2].path, "/api/gen/clip-a/opus_file/");
}

#[tokio::test]
async fn stems_posts_current_web_contract() {
    let mut billing =
        serde_json::from_str::<serde_json::Value>(&billing_with_features(&["get_stems"]))
            .expect("billing fixture");
    billing["models"] = serde_json::json!([]);
    let billing = billing.to_string();
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","is_trashed":false,"action_config":{"actions":[{"action_type":"get_stems","visible":true,"disabled":false}]}}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"stem-1","title":"Source Song (Vocals)","status":"submitted","model_name":"chirp-stem","created_at":"2026-06-30T00:00:00Z"},{"id":"stem-2","title":"Source Song (Drums)","status":"submitted","model_name":"chirp-stem","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client.stems("clip-a", None).await.expect("stems");

    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].id, "stem-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/c/check");
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("request json");
    assert!(body["token"].is_null());
    assert!(body["token_provider"].is_null());
    assert_eq!(body["task"], "gen_stem");
    assert_eq!(body["mv"], "chirp-v3-0");
    assert_eq!(body["title"], "Source Song");
    assert_eq!(body["tags"], "");
    assert_eq!(body["prompt"], "");
    assert_eq!(body["make_instrumental"], true);
    assert_eq!(body["continue_clip_id"], "clip-a");
    assert_eq!(body["stem_type_id"], 91);
    assert_eq!(body["stem_type_group_name"], "Twelve");
    assert_eq!(body["stem_task"], "twelve");
    assert_eq!(body["metadata"]["create_mode"], "custom");
    assert_eq!(body["metadata"]["is_remix"], true);
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert_eq!(body["metadata"]["is_max_mode"], false);
    assert!(
        !body["metadata"]
            .as_object()
            .expect("metadata object")
            .contains_key("is_mumble")
    );
}

#[tokio::test]
async fn stems_with_challenge_token_posts_generate_without_preflight_contract() {
    let billing = billing_with_features(&["get_stems"]);
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","is_trashed":false,"action_config":{"actions":[{"action_type":"get_stems","visible":true,"disabled":false}]}}"#,
        billing.as_str(),
        r#"{"clips":[{"id":"stem-1","title":"Source Song (Vocals)","status":"submitted","model_name":"chirp-stem","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client
        .stems("clip-a", Some("captcha-token".into()))
        .await
        .expect("stems");

    assert_eq!(clips[0].id, "stem-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["task"], "gen_stem");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert_eq!(body["token"], "captcha-token");
    assert_eq!(body["token_provider"], 1);
}

#[tokio::test]
async fn split_from_mix_posts_the_current_target_stem_contract() {
    let billing = billing_with_features(&["get_stems"]);
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","is_trashed":false,"action_config":{"actions":[{"action_type":"get_stems","visible":true,"disabled":false}]}}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"stem-target","title":"Source Song (Vocals)","status":"submitted","model_name":"chirp-stem","created_at":"2026-06-30T00:00:00Z"},{"id":"stem-rest","title":"Source Song (Instrumental)","status":"submitted","model_name":"chirp-stem","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let request = client
        .prepare_split_stems_request("clip-a", "Vocals", "Lead Vocal", None)
        .await
        .expect("split request");
    let result = client.generate(&request).await.expect("split from mix");

    assert_eq!(result.len(), 2);
    let requests = server.captured_all().await;
    let body = serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("request json");
    assert_eq!(body["task"], "gen_stem");
    assert_eq!(body["stem_type_id"], 91);
    assert_eq!(body["stem_type_group_name"], "Vocals");
    assert_eq!(body["stem_name"], "Lead Vocal");
    assert_eq!(body["stem_task"], "extract");
}

#[tokio::test]
async fn existing_stem_results_use_current_pages_and_page_routes() {
    let server = MockServer::json_sequence(&[
        r#"{"pages":2}"#,
        r#"{"stems":[{"id":"stem-1"}]}"#,
        r#"{"stems":[{"id":"stem-2"}]}"#,
        r#"{"clips":[{"id":"stem-2","title":"Drums","status":"complete","model_name":"chirp-stem","created_at":"2026-08-24T00:00:00Z","metadata":{"stem_type_group_name":"Drums"}},{"id":"stem-1","title":"Vocals","status":"complete","model_name":"chirp-stem","created_at":"2026-08-24T00:00:00Z","metadata":{"stem_type_group_name":"Vocals"}}],"has_more":false}"#,
    ])
    .await;
    let client = server.client();

    let results = client.get_all_stem_results("clip-a").await.expect("stems");

    assert_eq!(results.pages, 2);
    assert_eq!(results.banks.len(), 2);
    assert_eq!(results.banks[0].page, 0);
    assert_eq!(results.banks[0].stems[0].id, "stem-1");
    assert_eq!(results.banks[1].page, 1);
    assert_eq!(results.banks[1].stems[0].id, "stem-2");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a/stems/pages");
    assert_eq!(requests[1].path, "/api/clip/clip-a/stems?page=0");
    assert_eq!(requests[2].path, "/api/clip/clip-a/stems?page=1");
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/api/feed/v3");
    let body =
        serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("stem hydration body");
    assert_eq!(
        body["filters"]["ids"]["clipIds"],
        serde_json::json!(["stem-1", "stem-2"])
    );
}

#[tokio::test]
async fn empty_or_null_stem_results_match_current_web_fallbacks() {
    let null_pages = MockServer::json(r#"{"pages":null}"#).await;
    let client = null_pages.client();
    let results = client
        .get_all_stem_results("clip-empty")
        .await
        .expect("null page count is empty");
    assert_eq!(results.pages, 0);
    assert!(results.banks.is_empty());
    assert_eq!(null_pages.captured_all().await.len(), 1);

    let null_stems = MockServer::json(r#"{"stems":null}"#).await;
    let client = null_stems.client();
    let bank = client
        .get_stem_result_page("clip-empty", 0)
        .await
        .expect("null stems are empty");
    assert!(bank.stems.is_empty());
    assert!(bank.missing_clip_ids.is_empty());
    assert_eq!(null_stems.captured_all().await.len(), 1);
}

#[tokio::test]
async fn stem_hydration_preserves_missing_reference_ids() {
    let server = MockServer::json_sequence(&[
        r#"{"pages":1}"#,
        r#"{"stems":[{"id":"stem-present"},{"id":"stem-missing"}]}"#,
        r#"{"clips":[{"id":"stem-present","title":"Vocals","status":"complete","model_name":"chirp-stem","created_at":"2026-08-24T00:00:00Z"}],"has_more":false}"#,
    ])
    .await;
    let client = server.client();

    let results = client
        .get_all_stem_results("clip-a")
        .await
        .expect("partial hydration remains inspectable");

    assert_eq!(results.banks[0].stems[0].id, "stem-present");
    assert_eq!(results.banks[0].missing_clip_ids, ["stem-missing"]);
}

#[tokio::test]
async fn extend_fetches_source_clip_and_posts_string_title_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"prompt":"[Verse]\nOriginal words"}}"#,
        r#"{"clips":[{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"tags":"source chamber folk","negative_tags":"vocals, narration","prompt":"[Verse]\nOriginal words","make_instrumental":true}}]}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"extend-1","title":"Source Song","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let clips = client
        .extend(ExtendClipOptions {
            clip_id: "clip-a",
            continue_at: 118.0,
            tags: None,
            negative_tags: None,
            lyrics: None,
            title: None,
            instrumental: None,
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect("extend");

    assert_eq!(clips[0].id, "extend-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/feed/v3");
    let feed_body =
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("feed request json");
    assert_eq!(feed_body["filters"]["searchText"], "Source Song");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/billing/info/");
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/api/c/check");
    assert_eq!(requests[4].method, "POST");
    assert_eq!(requests[4].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[4].body).expect("request json");
    assert_eq!(body["task"], "extend");
    assert_eq!(body["title"], "Source Song");
    assert_eq!(body["prompt"], "");
    assert_eq!(body["continued_aligned_prompt"], "[Verse]\nOriginal words");
    assert_eq!(body["tags"], "source chamber folk");
    assert_eq!(body["negative_tags"], "vocals, narration");
    assert_eq!(body["continue_clip_id"], "clip-a");
    assert_eq!(body["continue_at"], 118.0);
    assert_eq!(body["make_instrumental"], true);
    assert_eq!(body["metadata"]["create_mode"], "custom");
    assert_eq!(body["metadata"]["is_remix"], true);
    assert_eq!(body["metadata"]["lyrics_updated"], false);
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
}

#[tokio::test]
async fn underpaint_uses_owned_vocal_source_and_current_v2_web_contract() {
    let mut billing: serde_json::Value =
        serde_json::from_str(&billing_info_response("tier-pro")).expect("billing fixture");
    billing["accessible_features"] = serde_json::json!(["edit_mode"]);
    billing["models"][0]["allowed_condition_combinations"] = serde_json::json!([["underpaint"]]);
    let billing = billing.to_string();
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"id":"clip-vocal","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-09-11T00:00:00Z","is_trashed":false,"user_id":"user-1","metadata":{"prompt":"[Verse]\nOriginal words","tags":"source pop","negative_tags":"metal","stem_type_group_name":"Vocals"}}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"underpaint-1","title":"Source Song (Add Instrumental)","status":"submitted","model_name":"chirp-fenix","created_at":"2026-09-11T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let request = client
        .prepare_paint_request(PaintOptions {
            clip_id: "clip-vocal",
            title: None,
            lyrics: None,
            tags: None,
            negative_tags: None,
            model: "auto",
            mode: PaintMode::Underpaint,
        })
        .await
        .expect("prepare underpaint");
    let clips = client.generate(&request).await.expect("underpaint");

    assert_eq!(clips[0].id, "underpaint-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/clip/clip-vocal");
    assert_eq!(requests[2].path, "/api/billing/info/");
    assert_eq!(requests[3].path, "/api/c/check");
    assert_eq!(requests[4].path, "/api/generate/v2-web/");
    let body: serde_json::Value =
        serde_json::from_str(&requests[4].body).expect("generation request");
    assert_eq!(body["task"], "underpainting");
    assert_eq!(body["underpainting_clip_id"], "clip-vocal");
    assert!(body.get("overpainting_clip_id").is_none());
    assert_eq!(body["metadata"]["is_remix"], true);
    assert_eq!(body["prompt"], "[Verse]\nOriginal words");
    assert_eq!(body["tags"], "source pop");
    assert_eq!(body["negative_tags"], "metal");
}

#[tokio::test]
async fn paint_fails_before_submit_without_edit_mode_or_ownership() {
    let no_access = MockServer::json(&billing_info_response("tier-pro")).await;
    let error = no_access
        .client()
        .prepare_paint_request(PaintOptions {
            clip_id: "clip-vocal",
            title: None,
            lyrics: None,
            tags: None,
            negative_tags: None,
            model: "auto",
            mode: PaintMode::Underpaint,
        })
        .await
        .expect_err("edit mode entitlement is required");
    assert!(error.to_string().contains("edit_mode"));
    assert_eq!(no_access.captured_all().await.len(), 1);

    let mut billing: serde_json::Value =
        serde_json::from_str(&billing_info_response("tier-pro")).expect("billing fixture");
    billing["accessible_features"] = serde_json::json!(["edit_mode"]);
    let billing = billing.to_string();
    let wrong_owner = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"id":"clip-vocal","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-09-11T00:00:00Z","is_trashed":false,"user_id":"user-other","metadata":{"stem_type_group_name":"Vocals"}}"#,
    ])
    .await;
    let error = wrong_owner
        .client()
        .prepare_paint_request(PaintOptions {
            clip_id: "clip-vocal",
            title: None,
            lyrics: None,
            tags: None,
            negative_tags: None,
            model: "auto",
            mode: PaintMode::Underpaint,
        })
        .await
        .expect_err("ownership is required");
    assert!(error.to_string().contains("owned"));
    assert_eq!(wrong_owner.captured_all().await.len(), 2);
}

#[tokio::test]
async fn extend_uses_upload_extend_task_for_uploaded_audio() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"id":"upload-a","title":"Uploaded Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"type":"upload","tags":"ambient pop","negative_tags":"metal","prompt":"[Verse]\nOriginal words","make_instrumental":false}}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"extend-1","title":"Uploaded Song","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    client
        .extend(ExtendClipOptions {
            clip_id: "upload-a",
            continue_at: 45.0,
            tags: None,
            negative_tags: None,
            lyrics: None,
            title: None,
            instrumental: None,
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect("extend uploaded audio");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[3].body)
        .expect("generation request json");
    assert_eq!(body["task"], "upload_extend");
    assert_eq!(body["continue_clip_id"], "upload-a");
}

#[tokio::test]
async fn extend_propagates_rate_limit_from_metadata_enrichment_without_submitting() {
    let server = MockServer::json_status_sequence(&[
        (
            200,
            r#"{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"prompt":"[Verse]\nOriginal words"}}"#,
        ),
        (429, ""),
    ])
    .await;
    let client = server.client();

    let error = client
        .extend(ExtendClipOptions {
            clip_id: "clip-a",
            continue_at: 118.0,
            tags: None,
            negative_tags: None,
            lyrics: None,
            title: None,
            instrumental: None,
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect_err("metadata enrichment rate limit must stop extend");

    assert!(matches!(error, CliError::RateLimited));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].path, "/api/feed/v3");
}

#[tokio::test]
async fn extend_metadata_fallback_does_not_merge_same_title_different_clip() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"prompt":"[Verse]\nOriginal words"}}"#,
        r#"{"clips":[{"id":"clip-other","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"tags":"wrong same-title tags","negative_tags":"wrong negatives","make_instrumental":true}}]}"#,
        billing.as_str(),
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"extend-1","title":"Source Song","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    client
        .extend(ExtendClipOptions {
            clip_id: "clip-a",
            continue_at: 118.0,
            tags: None,
            negative_tags: None,
            lyrics: None,
            title: None,
            instrumental: None,
            challenge_token: None,
            model: "auto",
        })
        .await
        .expect("extend");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "/api/clip/clip-a");
    assert_eq!(requests[1].path, "/api/feed/v3");
    assert_eq!(requests[2].path, "/api/billing/info/");
    assert_eq!(requests[3].path, "/api/c/check");
    assert_eq!(requests[4].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[4].body).expect("request json");
    assert_eq!(body["tags"], "");
    assert_eq!(body["negative_tags"], "");
    assert_eq!(body["make_instrumental"], false);
}

#[tokio::test]
async fn lyrics_generation_uses_current_cowrite_submit_contract() {
    let server = MockServer::json_sequence(&[
        r#"[{"id":"lyrics-v2","display_name":"Lyrics v2","family":"remi","supports_thinking":true}]"#,
        r#"{"edited_lyrics":"[Verse]\nHello","lyrics_request_id":"request-1","lyrics_id":"lyrics-1","variants":null,"artist_to_tag_mapping":{"A":"pop"},"next_prompts":["add a chorus"],"generation_trace":"trace-1"}"#,
    ])
    .await;
    let client = server.client();

    let result = client
        .generate_lyrics(CowriteLyricsOptions {
            prompt: "write a pop hook",
            model: Some("Lyrics v2"),
            enable_thinking: true,
        })
        .await
        .expect("lyrics");

    assert_eq!(result.edited_lyrics, "[Verse]\nHello");
    assert_eq!(result.lyrics_request_id.as_deref(), Some("request-1"));
    assert_eq!(result.lyrics_id.as_deref(), Some("lyrics-1"));
    assert!(result.variants.is_none());
    assert_eq!(result.artist_to_tag_mapping.as_ref().unwrap()["A"], "pop");
    assert_eq!(
        result.next_prompts.as_ref().expect("next prompts"),
        &["add a chorus".to_string()]
    );
    assert_eq!(result.extra["generation_trace"], "trace-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/generate/cowrite-lyrics/models/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/generate/cowrite-lyrics/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json"),
        serde_json::json!({
            "selected": "",
            "context_before": "",
            "context_after": "",
            "instruction": "write a pop hook",
            "title": "",
            "style": "",
            "mode": "apply_user_request",
            "references": [],
            "num_variants": null,
            "lyricist_id": null,
            "metadata": {
                "lyrics_model": "lyrics-v2",
                "enable_thinking": true
            },
            "create_session_token": null,
            "lyrics_project_id": null
        })
    );
}

#[tokio::test]
async fn lyrics_generation_preserves_the_web_default_model_when_none_is_selected() {
    let server = MockServer::json_sequence(&[
        r#"[{"id":"lyrics-v2","display_name":"Lyrics v2","family":"remi","supports_thinking":true}]"#,
        r#"{"edited_lyrics":"[Verse]\nHello"}"#,
    ])
    .await;
    let client = server.client();

    client
        .generate_lyrics(CowriteLyricsOptions {
            prompt: "write a pop hook",
            model: None,
            enable_thinking: false,
        })
        .await
        .expect("default Cowrite lyrics");

    let requests = server.captured_all().await;
    let body =
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("Cowrite request json");
    assert_eq!(body["metadata"]["lyrics_model"], "default");
    assert_eq!(body["metadata"]["enable_thinking"], false);
}

#[tokio::test]
async fn lyrics_generation_rejects_thinking_when_the_discovered_model_does_not_support_it() {
    let server = MockServer::json(
        r#"[{"id":"lyrics-fast","display_name":"Lyrics Fast","family":"remi","supports_thinking":false}]"#,
    )
    .await;
    let client = server.client();

    let error = client
        .generate_lyrics(CowriteLyricsOptions {
            prompt: "write a pop hook",
            model: Some("Lyrics Fast"),
            enable_thinking: true,
        })
        .await
        .expect_err("unsupported thinking must stop before Cowrite submit");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("does not support thinking"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/generate/cowrite-lyrics/models/");
}

#[tokio::test]
async fn aligned_lyrics_starts_current_v3_contract_and_uses_immediate_alignment() {
    let server = MockServer::json(
        r#"{"alignment":[{"word":"Hello","start_s":0.0,"end_s":0.5,"p_align":0.99,"phoneme":"həˈloʊ"}]}"#,
    )
    .await;
    let client = server.client();

    let words = client
        .aligned_lyrics(
            "clip-a",
            Some("Hello"),
            true,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("aligned lyrics");

    assert_eq!(words[0].word, "Hello");
    assert_eq!(words[0].extra["phoneme"], "həˈloʊ");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/clip-a/aligned_lyrics/v3");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "lyrics": "Hello",
            "enable_augmentation": true
        })
    );
}

#[tokio::test]
async fn aligned_lyrics_polls_current_v3_contract_until_alignment_is_ready() {
    let server = MockServer::json_sequence(&[
        r#"{"state":"running"}"#,
        r#"{"detail":"Lyrics alignment not available, try again later."}"#,
        r#"{"alignment":[{"word":"Hello","start_s":0.0,"end_s":0.5}]}"#,
    ])
    .await;
    let client = server.client();

    let words = client
        .aligned_lyrics(
            "clip-a",
            Some("Hello"),
            false,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("aligned lyrics");

    assert_eq!(words[0].word, "Hello");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/gen/clip-a/aligned_lyrics/v3");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/gen/clip-a/aligned_lyrics/v3");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/gen/clip-a/aligned_lyrics/v3");
}

#[tokio::test]
async fn aligned_lyrics_uses_v2_only_as_v3_compatibility_fallback() {
    let server = MockServer::json_sequence(&[
        r#"{"state":"error","error_message":"v3 unavailable"}"#,
        r#"{"aligned_words":[{"word":"Hello","start_s":0.0,"end_s":0.5,"success":true,"p_align":0.99}]}"#,
    ])
    .await;
    let client = server.client();

    let words = client
        .aligned_lyrics(
            "clip-a",
            Some("Hello"),
            true,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("v2 compatibility fallback");

    assert_eq!(words[0].word, "Hello");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/gen/clip-a/aligned_lyrics/v3");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/gen/clip-a/aligned_lyrics/v2");
}

#[tokio::test]
async fn aligned_lyrics_does_not_hide_v3_schema_drift_behind_v2() {
    let server = MockServer::json_sequence(&[r#"{}"#, r#"{"unexpected":true}"#]).await;
    let client = server.client();

    let error = client
        .aligned_lyrics(
            "clip-a",
            Some("Hello"),
            true,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("v3 schema drift must remain visible");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/api/gen/clip-a/aligned_lyrics/v3");
    assert_eq!(requests[1].path, "/api/gen/clip-a/aligned_lyrics/v3");
}

#[tokio::test]
async fn aligned_lyrics_without_source_lyrics_uses_v2_compatibility_path() {
    let server = MockServer::json(
        r#"{"aligned_words":[{"word":"Hello","start_s":0.0,"end_s":0.5,"success":true}]}"#,
    )
    .await;
    let client = server.client();

    let words = client
        .aligned_lyrics(
            "clip-a",
            None,
            true,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("v2 compatibility path");

    assert_eq!(words[0].word, "Hello");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/aligned_lyrics/v2");
}
#[tokio::test]
async fn playlist_reaction_posts_current_web_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_playlist_reaction("playlist-1", Some(PlaylistReaction::Like))
        .await
        .expect("set playlist reaction");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        "/api/playlist_reaction/playlist-1/update_reaction_type/"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "reaction": "LIKE" })
    );
}

#[tokio::test]
async fn list_playlists_gets_me_page_contract() {
    let server = MockServer::json(
        r#"{"playlists":[{"id":"playlist-1","name":"Road Trip","description":null,"is_public":false,"is_trashed":false,"song_count":3,"num_total_results":1,"current_page":2,"playlist_clips":[],"entity_type":"playlist","play_count":42}],"num_total_results":1,"current_page":2}"#,
    )
    .await;
    let client = server.client();

    let response = client.list_playlists(2).await.expect("list playlists");

    assert_eq!(response.current_page, 2);
    assert_eq!(response.num_total_results, 1);
    assert_eq!(response.playlists[0].id, "playlist-1");
    assert_eq!(response.playlists[0].name, "Road Trip");
    assert_eq!(response.playlists[0].song_count, Some(3));
    assert_eq!(response.playlists[0].extra["entity_type"], "playlist");
    assert_eq!(response.playlists[0].extra["play_count"], 42);
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/playlist/me?page=2");
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn playlist_detail_reads_v2_cover_metadata_contract() {
    let server = MockServer::json(
        r#"{"metadata":{"id":"playlist-1","name":"Road Trip","description":"Drive set","cover_url":"https://cdn2.suno.ai/image_upload-1.jpeg","cover_image_s3_id":"image_upload-1","cover_is_user_set":true,"is_public":true,"owner":{"handle":"owner"}},"relationship":{"is_trashed":false,"can_edit":true},"stats":{"track_count":3,"save_count":2},"deferred_fields":[]}"#,
    )
    .await;
    let client = server.client();

    let playlist = client.get_playlist("playlist-1").await.expect("playlist");

    assert_eq!(playlist.name, "Road Trip");
    assert_eq!(playlist.description.as_deref(), Some("Drive set"));
    assert_eq!(
        playlist.image_url.as_deref(),
        Some("https://cdn2.suno.ai/image_upload-1.jpeg")
    );
    assert_eq!(
        playlist.cover_url.as_deref(),
        Some("https://cdn2.suno.ai/image_upload-1.jpeg")
    );
    assert_eq!(
        playlist.cover_image_s3_id.as_deref(),
        Some("image_upload-1")
    );
    assert_eq!(playlist.cover_is_user_set, Some(true));
    assert_eq!(playlist.is_public, Some(true));
    assert_eq!(playlist.song_count, Some(3));
    assert_eq!(
        playlist.metadata.as_ref().unwrap()["owner"]["handle"],
        "owner"
    );
    assert_eq!(playlist.relationship.as_ref().unwrap()["can_edit"], true);
    assert_eq!(playlist.stats.as_ref().unwrap()["save_count"], 2);
    assert_eq!(playlist.extra["deferred_fields"], serde_json::json!([]));
}

#[tokio::test]
async fn create_playlist_posts_name_only_contract() {
    let server = MockServer::json(r#"{"id":"playlist-1","name":"Road Trip"}"#).await;
    let client = server.client();

    let playlist = client
        .create_playlist("Road Trip")
        .await
        .expect("create playlist");

    assert_eq!(playlist.id, "playlist-1");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/playlist/create/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("create json"),
        serde_json::json!({ "name": "Road Trip" })
    );
}

#[tokio::test]
async fn create_playlist_treats_accepted_schema_loss_as_ambiguous() {
    let server = MockServer::json(r#"{"status":"created"}"#).await;
    let client = server.client();

    let error = client
        .create_playlist("Road Trip")
        .await
        .expect_err("an accepted response without playlist identity is ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity details")["stage"],
        "response_schema"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn set_playlist_uploaded_cover_patches_v2_metadata_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_playlist_uploaded_cover("playlist-1", "upload-1")
        .await
        .expect("set cover");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(requests[0].path, "/api/playlist/v2/playlist-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("cover json"),
        serde_json::json!({
            "metadata": {
                "cover_image_s3_id": "image_upload-1"
            }
        })
    );
}

#[tokio::test]
async fn playlist_create_workflow_reports_completed_mutations_when_cover_update_fails() {
    let server = MockServer::response_sequence(vec![
        (
            200,
            r#"{"id":"playlist-1","metadata":{"name":"Road Trip"}}"#.to_string(),
        ),
        (200, "{}".to_string()),
        (429, r#"{"detail":"rate limited"}"#.to_string()),
    ])
    .await;
    let client = server.client();

    let error = crate::workflow::playlist::create(
        &client,
        crate::workflow::playlist::CreatePlaylistInput {
            name: "Road Trip",
            description: Some("Drive set"),
            external_image_url: None,
            cover: Some(crate::workflow::playlist::CoverReference::existing(
                "existing-upload-1",
                "https://cdn2.suno.ai/image_existing-upload-1.jpeg",
            )),
        },
    )
    .await
    .expect_err("cover failure must retain earlier playlist mutations");

    assert_eq!(error.error_code(), "partial_mutation");
    assert_eq!(
        error.details().expect("playlist workflow checkpoint"),
        &serde_json::json!({
            "operation": "playlist_create",
            "playlist_id": "playlist-1",
            "cover": {
                "upload_id": "existing-upload-1",
                "image_url": "https://cdn2.suno.ai/image_existing-upload-1.jpeg",
                "uploaded_here": false
            },
            "completed_steps": ["playlist_created", "metadata_updated"],
            "failed": {
                "step": "cover_update",
                "code": "rate_limited",
                "message": "Rate limited by Suno — wait and retry"
            },
            "recovery": {
                "resumable": true,
                "command": "sunox playlist set",
                "arguments": {
                    "playlist_id": "playlist-1",
                    "image_url": "https://cdn2.suno.ai/image_existing-upload-1.jpeg"
                }
            }
        })
    );
    assert_eq!(server.captured_all().await.len(), 3);
}

#[tokio::test]
async fn playlist_create_failure_after_local_cover_upload_is_resumable_by_image_url() {
    let server = MockServer::json_status_sequence(&[(429, r#"{"detail":"rate limited"}"#)]).await;
    let client = server.client();

    let error = crate::workflow::playlist::create(
        &client,
        crate::workflow::playlist::CreatePlaylistInput {
            name: "Road Trip",
            description: None,
            external_image_url: None,
            cover: Some(crate::workflow::playlist::CoverReference::uploaded(
                "upload-1",
                "https://cdn2.suno.ai/image_upload-1.jpeg",
            )),
        },
    )
    .await
    .expect_err("created cover must survive playlist create failure");

    let details = error.details().expect("playlist create recovery");
    assert_eq!(details["cover"]["upload_id"], "upload-1");
    assert_eq!(details["cover"]["uploaded_here"], true);
    assert_eq!(
        details["completed_steps"],
        serde_json::json!(["cover_uploaded"])
    );
    assert_eq!(details["recovery"]["resumable"], true);
    assert_eq!(details["recovery"]["command"], "sunox playlist create");
    assert_eq!(
        details["recovery"]["omit_original_arguments"],
        serde_json::json!(["image_file"])
    );
    assert_eq!(
        details["recovery"]["arguments"]["image_url"],
        "https://cdn2.suno.ai/image_upload-1.jpeg"
    );
}

#[tokio::test]
async fn set_playlist_metadata_patches_current_v2_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_playlist_metadata("playlist-1", Some("Road Trip"), Some("Drive set"), None)
        .await
        .expect("set metadata");

    let request = server.captured().await;
    assert_eq!(request.method, "PATCH");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("metadata json"),
        serde_json::json!({
            "metadata": { "name": "Road Trip" },
            "bio": { "description": "Drive set" }
        })
    );
}

#[tokio::test]
async fn set_playlist_external_image_url_uses_legacy_compatibility_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_playlist_metadata(
            "playlist-1",
            Some("Road Trip"),
            Some("Drive set"),
            Some("https://cdn.example/cover.jpg"),
        )
        .await
        .expect("set metadata");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/playlist/set_metadata");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("metadata json"),
        serde_json::json!({
            "playlist_id": "playlist-1",
            "name": "Road Trip",
            "description": "Drive set",
            "image_url": "https://cdn.example/cover.jpg"
        })
    );
}

#[tokio::test]
async fn add_clips_to_playlist_posts_v2_tracks_add_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .add_clips_to_playlist("playlist-1", &["clip-a".to_string(), "clip-b".to_string()])
        .await
        .expect("add clips");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1/tracks/add");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "clip_ids": ["clip-a", "clip-b"] })
    );
}

#[tokio::test]
async fn remove_clips_from_playlist_posts_v2_tracks_remove_contract() {
    let server = MockServer::json_until_idle("{}", 2).await;
    let client = server.client();

    let report = client
        .remove_clips_from_playlist("playlist-1", &["clip-a".to_string(), "clip-b".to_string()])
        .await
        .expect("remove clips");

    assert_eq!(report.succeeded_clip_ids, vec!["clip-a", "clip-b"]);
    assert!(report.failed.is_empty());
    assert!(report.not_attempted_clip_ids.is_empty());

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/playlist/v2/playlist-1/tracks/remove");
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json"),
        serde_json::json!({ "clip_ids": ["clip-a"] })
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json"),
        serde_json::json!({ "clip_ids": ["clip-b"] })
    );
}

#[tokio::test]
async fn remove_clips_from_playlist_reports_partial_failure() {
    let server = MockServer::json_status_sequence(&[
        (200, "{}"),
        (
            500,
            r#"{"status_code":500,"detail":"An unexpected error occurred."}"#,
        ),
    ])
    .await;
    let client = server.client();

    let report = client
        .remove_clips_from_playlist(
            "playlist-1",
            &[
                "clip-a".to_string(),
                "clip-b".to_string(),
                "clip-c".to_string(),
            ],
        )
        .await
        .expect("partial report");

    assert_eq!(report.succeeded_clip_ids, vec!["clip-a"]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].clip_id, "clip-b");
    assert_eq!(report.failed[0].error_code, "ambiguous_mutation");
    let failure_details = report.failed[0]
        .details
        .as_ref()
        .expect("ambiguity details");
    assert_eq!(failure_details["stage"], "response_status");
    assert!(
        failure_details["cause"]["message"]
            .as_str()
            .expect("cause message")
            .contains("HTTP 500")
    );
    assert_eq!(report.not_attempted_clip_ids, vec!["clip-c"]);

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn remove_clips_from_playlist_propagates_first_failure() {
    let server = MockServer::json_status_sequence(&[(
        500,
        r#"{"status_code":500,"detail":"An unexpected error occurred."}"#,
    )])
    .await;
    let client = server.client();

    let error = client
        .remove_clips_from_playlist(
            "playlist-1",
            &[
                "clip-a".to_string(),
                "clip-b".to_string(),
                "clip-c".to_string(),
            ],
        )
        .await
        .expect_err("first failure should not become partial mutation");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn remove_clips_from_playlist_propagates_first_rate_limit() {
    let server = MockServer::json_status_sequence(&[(429, "")]).await;
    let client = server.client();

    let error = client
        .remove_clips_from_playlist("playlist-1", &["clip-a".to_string(), "clip-b".to_string()])
        .await
        .expect_err("first rate limit should not become partial mutation");

    assert!(matches!(error, CliError::RateLimited));

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn reorder_playlist_clip_posts_positions_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .reorder_playlist_clip("playlist-1", "clip-a", 3)
        .await
        .expect("reorder clip");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        "/api/playlist/v2/playlist-1/tracks/reorder-by-index"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "positions": [{ "clip_id": "clip-a", "index": 3 }] })
    );
}

#[tokio::test]
async fn set_playlist_visibility_patches_v2_metadata_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"playlist-1","name":"One","is_public":false}"#,
    ])
    .await;
    let client = server.client();

    client
        .set_playlist_visibility("playlist-1", false)
        .await
        .expect("set visibility");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let request = &requests[0];
    assert_eq!(request.method, "PATCH");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "metadata": { "is_public": false } })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/playlist/v2/playlist-1");
}

#[tokio::test]
async fn playlist_visibility_readback_requires_the_state_field() {
    let server = MockServer::json_sequence(&["{}", r#"{"id":"playlist-1","name":"One"}"#]).await;
    let client = server.client();

    let error = client
        .set_playlist_visibility("playlist-1", false)
        .await
        .expect_err("a missing visibility field cannot confirm private state");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity details")["stage"],
        "readback_state"
    );
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn trash_playlist_posts_undo_false_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"playlist-1","name":"One","is_trashed":true}"#,
    ])
    .await;
    let client = server.client();

    client
        .trash_playlist("playlist-1")
        .await
        .expect("trash playlist");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "undo": false })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/playlist/v2/playlist-1");
}

#[tokio::test]
async fn restore_playlist_posts_undo_true_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"playlist-1","name":"One","is_trashed":false}"#,
    ])
    .await;
    let client = server.client();

    client
        .restore_playlist("playlist-1")
        .await
        .expect("restore playlist");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "undo": true })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/playlist/v2/playlist-1");
}

#[tokio::test]
async fn playlist_restore_readback_requires_the_trash_state_field() {
    let server = MockServer::json_sequence(&["{}", r#"{"id":"playlist-1","name":"One"}"#]).await;
    let client = server.client();

    let error = client
        .restore_playlist("playlist-1")
        .await
        .expect_err("a missing trash field cannot confirm restored state");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity details")["stage"],
        "readback_state"
    );
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn save_and_unsave_playlist_use_v2_save_contract() {
    let save_server = MockServer::json("{}").await;
    let save_client = save_server.client();

    save_client
        .save_playlist("playlist-1")
        .await
        .expect("save playlist");

    let save_request = save_server.captured().await;
    assert_eq!(save_request.method, "POST");
    assert_eq!(save_request.path, "/api/playlist/v2/playlist-1/save");
    assert_eq!(save_request.body, "");

    let unsave_server = MockServer::json("{}").await;
    let unsave_client = unsave_server.client();

    unsave_client
        .unsave_playlist("playlist-1")
        .await
        .expect("unsave playlist");

    let unsave_request = unsave_server.captured().await;
    assert_eq!(unsave_request.method, "DELETE");
    assert_eq!(unsave_request.path, "/api/playlist/v2/playlist-1/save");
    assert_eq!(unsave_request.body, "");
}
#[tokio::test]
async fn create_persona_posts_current_web_contract() {
    let server = MockServer::json(r#"{"id":"persona-1","name":"Lead Voice"}"#).await;
    let client = server.client();

    let persona = client
        .create_persona(&CreatePersonaRequest {
            root_clip_id: Some("clip-a".into()),
            name: Some("Lead Voice".into()),
            description: Some("Warm".into()),
            image_s3_id: None,
            is_public: Some(false),
            is_suno_persona: None,
            persona_type: None,
            vox_audio_id: None,
            vocal_start_s: None,
            vocal_end_s: None,
            user_input_styles: None,
            source: None,
            singer_skill_level: None,
            clips: None,
            is_voice_recording: None,
            voice_recording_id: None,
            verification_id: None,
        })
        .await
        .expect("create persona");

    assert_eq!(persona.id, "persona-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/persona/create/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "root_clip_id": "clip-a",
            "name": "Lead Voice",
            "description": "Warm",
            "is_public": false
        })
    );
}

#[tokio::test]
async fn create_persona_treats_accepted_schema_loss_as_ambiguous() {
    let server = MockServer::json(r#"{"status":"created"}"#).await;
    let client = server.client();

    let error = client
        .create_persona(&CreatePersonaRequest {
            root_clip_id: Some("clip-a".into()),
            name: Some("Lead Voice".into()),
            description: None,
            image_s3_id: None,
            is_public: Some(false),
            is_suno_persona: None,
            persona_type: None,
            vox_audio_id: None,
            vocal_start_s: None,
            vocal_end_s: None,
            user_input_styles: None,
            source: None,
            singer_skill_level: None,
            clips: None,
            is_voice_recording: None,
            voice_recording_id: None,
            verification_id: None,
        })
        .await
        .expect_err("an accepted response without persona identity is ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity details")["stage"],
        "response_schema"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn voice_phrase_uses_the_current_language_query_contract() {
    let server = MockServer::json(r#"{"phrase_id":"phrase-1","phrase_text":"Read this"}"#).await;
    let client = server.client();

    let phrase = client.get_voice_phrase("zh").await.expect("voice phrase");

    assert_eq!(phrase.phrase_id, "phrase-1");
    assert_eq!(phrase.phrase_text, "Read this");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/voice-verification/phrase/?language=zh");
}

#[tokio::test]
async fn voice_status_reads_use_current_processed_and_verification_routes() {
    let server = MockServer::json_sequence(&[
        r#"{"id":"processed-1","status":"completed","voice_recording_id":"recording-1"}"#,
        r#"{"id":"verification-1","status":"approved"}"#,
    ])
    .await;
    let client = server.client();

    let processed = client
        .get_processed_voice_status("processed-1")
        .await
        .expect("processed status");
    let verification = client
        .get_voice_verification("verification-1")
        .await
        .expect("verification status");

    assert_eq!(processed.status, "completed");
    assert_eq!(verification.status, "approved");
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/processed_clip/processed-1");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/voice-verification/verification-1");
}

#[tokio::test]
async fn voice_status_reads_reject_mismatched_response_identities() {
    let server = MockServer::json_sequence(&[
        r#"{"id":"processed-other","status":"completed"}"#,
        r#"{"id":"verification-other","status":"approved"}"#,
    ])
    .await;
    let client = server.client();

    let processed_error = client
        .get_processed_voice_status("processed-expected")
        .await
        .expect_err("processed status identity must match its path");
    let verification_error = client
        .get_voice_verification("verification-expected")
        .await
        .expect_err("verification identity must match its path");

    assert!(matches!(
        processed_error,
        CliError::Api {
            code: "schema_drift",
            ..
        }
    ));
    assert!(matches!(
        verification_error,
        CliError::Api {
            code: "schema_drift",
            ..
        }
    ));
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn voice_processing_posts_distinct_main_and_verification_bodies() {
    let server = MockServer::json_sequence(&[
        r#"{"id":"processed-1","voice_recording_id":"recording-main"}"#,
        r#"{"id":"processed-verify","voice_recording_id":"recording-verify"}"#,
    ])
    .await;
    let client = server.client();

    client
        .process_voice_sample(
            "workflow-1",
            &ProcessVoiceSampleRequest {
                upload_id: "upload-main".into(),
                vocal_start_s: 0.0,
                vocal_end_s: 42.35,
            },
        )
        .await
        .expect("process main sample");
    client
        .process_voice_verification_recording(
            "workflow-1",
            &ProcessVoiceVerificationRecordingRequest::new("upload-verify".into()),
        )
        .await
        .expect("process verification recording");

    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/processed_clip/voice-vox-stem");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("main body"),
        serde_json::json!({
            "upload_id": "upload-main",
            "vocal_start_s": 0.0,
            "vocal_end_s": 42.35
        })
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/processed_clip/voice-vox-stem");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body)
            .expect("verification recording body"),
        serde_json::json!({
            "upload_id": "upload-verify",
            "recording_type": "verification"
        })
    );
}

#[tokio::test]
async fn voice_verification_posts_all_current_server_identities() {
    let server = MockServer::json(r#"{"id":"verification-1","status":"pending"}"#).await;
    let client = server.client();

    let verification = client
        .create_voice_verification(
            "workflow-1",
            &CreateVoiceVerificationRequest {
                voice_recording_id: "recording-main".into(),
                verification_recording_id: "recording-verify".into(),
                phrase_id: "phrase-1".into(),
            },
        )
        .await
        .expect("create voice verification");

    assert_eq!(verification.id, "verification-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/voice-verification/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request body"),
        serde_json::json!({
            "voice_recording_id": "recording-main",
            "verification_recording_id": "recording-verify",
            "phrase_id": "phrase-1"
        })
    );
}

#[tokio::test]
async fn verified_voice_persona_posts_the_exact_private_vox_body() {
    let server = MockServer::json(
        r#"{"id":"persona-1","name":"My Voice","is_public":false,"persona_type":"vox"}"#,
    )
    .await;
    let client = server.client();

    let persona = client
        .create_verified_voice_persona(
            "workflow-1",
            &CreatePersonaRequest {
                root_clip_id: None,
                name: Some("My Voice".into()),
                description: Some(String::new()),
                image_s3_id: None,
                is_public: Some(false),
                is_suno_persona: None,
                persona_type: Some("vox".into()),
                vox_audio_id: Some("processed-1".into()),
                vocal_start_s: Some(0.0),
                vocal_end_s: Some(42.35),
                user_input_styles: Some("warm soul".into()),
                source: Some("random_song".into()),
                singer_skill_level: Some("Advanced".into()),
                clips: None,
                is_voice_recording: Some(true),
                voice_recording_id: Some("recording-main".into()),
                verification_id: Some("verification-1".into()),
            },
        )
        .await
        .expect("create verified Voice persona");

    assert_eq!(persona.id, "persona-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/persona/create/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request body"),
        serde_json::json!({
            "is_voice_recording": true,
            "voice_recording_id": "recording-main",
            "name": "My Voice",
            "description": "",
            "is_public": false,
            "persona_type": "vox",
            "source": "random_song",
            "user_input_styles": "warm soul",
            "singer_skill_level": "Advanced",
            "verification_id": "verification-1",
            "vox_audio_id": "processed-1",
            "vocal_start_s": 0.0,
            "vocal_end_s": 42.35
        })
    );
}

#[tokio::test]
async fn verified_voice_persona_conflict_is_not_replayed_and_preserves_recovery_identity() {
    let server =
        MockServer::json_status_sequence(&[(409, r#"{"detail":"voice persona already exists"}"#)])
            .await;
    let client = server.client();

    let error = client
        .create_verified_voice_persona(
            "workflow-1",
            &CreatePersonaRequest {
                root_clip_id: None,
                name: Some("My Voice".into()),
                description: Some(String::new()),
                image_s3_id: None,
                is_public: Some(false),
                is_suno_persona: None,
                persona_type: Some("vox".into()),
                vox_audio_id: Some("processed-1".into()),
                vocal_start_s: Some(0.0),
                vocal_end_s: Some(42.35),
                user_input_styles: Some("warm soul".into()),
                source: Some("random_song".into()),
                singer_skill_level: Some("Advanced".into()),
                clips: None,
                is_voice_recording: Some(true),
                voice_recording_id: Some("recording-main".into()),
                verification_id: Some("verification-1".into()),
            },
        )
        .await
        .expect_err("HTTP 409 must stay ambiguous and must not replay");

    let details = error.details().expect("ambiguity details");
    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(details["stage"], "persona_create_conflict");
    assert_eq!(details["voice_recording_id"], "recording-main");
    assert_eq!(details["verification_id"], "verification-1");
    assert_eq!(details["recovery"]["resumable"], false);
    assert_eq!(
        details["recovery"]["inspection_commands"],
        serde_json::json!(["sunox persona list --json"])
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn voice_mutation_schema_loss_is_ambiguous_and_is_not_replayed() {
    let server = MockServer::json_status_sequence(&[(200, "not-json")]).await;
    let client = server.client();

    let error = client
        .create_voice_verification(
            "workflow-1",
            &CreateVoiceVerificationRequest {
                voice_recording_id: "recording-main".into(),
                verification_recording_id: "recording-verify".into(),
                phrase_id: "phrase-1".into(),
            },
        )
        .await
        .expect_err("successful write with unreadable schema is ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "voice_create");
    assert_eq!(details["operation_id"], "workflow-1");
    assert_eq!(details["stage"], "verification_create_response_schema");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn voice_mutation_server_error_is_ambiguous_and_is_not_replayed() {
    let server =
        MockServer::json_status_sequence(&[(500, r#"{"detail":"unknown accepted state"}"#)]).await;
    let client = server.client();

    let error = client
        .create_voice_verification(
            "workflow-1",
            &CreateVoiceVerificationRequest {
                voice_recording_id: "recording-main".into(),
                verification_recording_id: "recording-verify".into(),
                phrase_id: "phrase-1".into(),
            },
        )
        .await
        .expect_err("5xx cannot prove the Voice write was rejected");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "verification_create_response_status"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn voice_mutation_auth_failure_is_not_refreshed_or_replayed() {
    let server =
        MockServer::json_status_sequence(&[(401, r#"{"detail":"Token validation failed."}"#)])
            .await;
    let client = server.client();

    let error = client
        .create_voice_verification(
            "workflow-1",
            &CreateVoiceVerificationRequest {
                voice_recording_id: "recording-main".into(),
                verification_recording_id: "recording-verify".into(),
                phrase_id: "phrase-1".into(),
            },
        )
        .await
        .expect_err("Voice writes must not enter the auth-refresh replay path");

    assert!(matches!(error, CliError::AuthExpired));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/voice-verification/");
}

#[tokio::test]
async fn set_persona_love_fetches_detail_then_toggles_when_needed() {
    let server = MockServer::json_sequence(&[
        r#"{"id":"persona-1","name":"Lead Voice","is_loved":false}"#,
        r#"{"loved":true}"#,
    ])
    .await;
    let client = server.client();

    let response = client
        .set_persona_love("persona-1", true)
        .await
        .expect("set persona love");

    assert!(response.loved);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/persona/get-persona/persona-1/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/persona/persona-1/toggle_love/");
    assert_eq!(requests[1].body, "");
}

#[tokio::test]
async fn set_persona_love_skips_toggle_when_state_already_matches() {
    let server =
        MockServer::json(r#"{"id":"persona-1","name":"Lead Voice","is_loved":true}"#).await;
    let client = server.client();

    let response = client
        .set_persona_love("persona-1", true)
        .await
        .expect("set persona love");

    assert!(response.loved);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/persona/get-persona/persona-1/");
}

#[tokio::test]
async fn set_persona_visibility_puts_current_web_contract() {
    let server =
        MockServer::json(r#"{"id":"persona-1","name":"Lead Voice","is_public":true}"#).await;
    let client = server.client();

    let persona = client
        .set_persona_visibility("persona-1", true)
        .await
        .expect("set persona visibility");

    assert_eq!(persona.is_public, Some(true));
    let request = server.captured().await;
    assert_eq!(request.method, "PUT");
    assert_eq!(
        request.path,
        "/api/persona/set_visibility/persona-1/?is_public=true"
    );
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn edit_persona_puts_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"persona-1","name":"Lead Voice","description":"Warm","is_public":false}"#,
    )
    .await;
    let client = server.client();

    let persona = client
        .edit_persona(&EditPersonaRequest {
            persona_id: "persona-1".into(),
            name: Some("Lead Voice".into()),
            description: Some("Warm".into()),
            image_s3_id: Some("image-1".into()),
            is_public: Some(false),
            persona_type: Some("vox".into()),
            user_input_styles: Some("soul".into()),
            vox_audio_id: Some("processed-1".into()),
            vocal_start_s: Some(0.43),
            vocal_end_s: Some(22.56),
        })
        .await
        .expect("edit persona");

    assert_eq!(persona.description.as_deref(), Some("Warm"));
    let request = server.captured().await;
    assert_eq!(request.method, "PUT");
    assert_eq!(request.path, "/api/persona/edit-persona/persona-1/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "persona_id": "persona-1",
            "name": "Lead Voice",
            "description": "Warm",
            "image_s3_id": "image-1",
            "is_public": false,
            "persona_type": "vox",
            "user_input_styles": "soul",
            "vox_audio_id": "processed-1",
            "vocal_start_s": 0.43,
            "vocal_end_s": 22.56
        })
    );
}

#[tokio::test]
async fn get_persona_clips_uses_current_web_paginated_contract() {
    let server = MockServer::json(
        r#"{"persona":{"id":"persona-1","name":"Lead Voice","persona_clips":[{"clip":{"id":"clip-1","title":"Song","status":"complete","model_name":"chirp","created_at":"2026-06-30T00:00:00Z"}}]},"total_results":1,"current_page":2,"is_following":false}"#,
    )
    .await;
    let client = server.client();

    let response = client
        .get_persona_clips("persona-1", 2)
        .await
        .expect("get persona clips");

    assert_eq!(response.persona.persona_clips[0].clip.id, "clip-1");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path,
        "/api/persona/get-persona-paginated/persona-1/?page=2"
    );
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn trash_personas_puts_current_web_per_id_trash_contract() {
    let server = MockServer::json(
        r#"{"updated_persona_ids":["persona-1"],"voice_persona_count":4,"max_voice_personas":1000}"#,
    )
    .await;
    let client = server.client();

    let response = client
        .trash_personas(&["persona-1".to_string()])
        .await
        .expect("trash persona");

    assert_eq!(response.updated_persona_ids, vec!["persona-1"]);
    let request = server.captured().await;
    assert_eq!(request.method, "PUT");
    assert_eq!(
        request.path,
        "/api/persona/trash-persona/persona-1/?undo=false&hide=false"
    );
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn restore_personas_puts_current_web_per_id_restore_contract() {
    let server = MockServer::json(
        r#"{"updated_persona_ids":["persona-1"],"voice_persona_count":5,"max_voice_personas":1000}"#,
    )
    .await;
    let client = server.client();

    client
        .restore_personas(&["persona-1".to_string()])
        .await
        .expect("restore persona");

    let request = server.captured().await;
    assert_eq!(request.method, "PUT");
    assert_eq!(
        request.path,
        "/api/persona/trash-persona/persona-1/?undo=true&hide=false"
    );
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn purge_personas_puts_current_web_per_id_delete_contract() {
    let server = MockServer::json(
        r#"{"updated_persona_ids":["persona-1"],"voice_persona_count":4,"max_voice_personas":1000}"#,
    )
    .await;
    let client = server.client();

    client
        .purge_personas(&["persona-1".to_string()])
        .await
        .expect("purge persona");

    let request = server.captured().await;
    assert_eq!(request.method, "PUT");
    assert_eq!(
        request.path,
        "/api/persona/trash-persona/persona-1/?undo=false&hide=true"
    );
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn persona_per_id_mutation_reports_partial_progress() {
    let server = MockServer::response_sequence(vec![
        (
            200,
            r#"{"updated_persona_ids":["persona-a"],"voice_persona_count":4,"max_voice_personas":1000}"#
                .into(),
        ),
        (500, r#"{"detail":"persona mutation failed"}"#.into()),
    ])
    .await;
    let client = server.client();
    let ids = vec!["persona-a".into(), "persona-b".into(), "persona-c".into()];

    let error = client
        .trash_personas(&ids)
        .await
        .expect_err("later failure must expose partial progress");

    assert_eq!(error.error_code(), "partial_mutation");
    let details = error.details().expect("partial details");
    assert_eq!(details["operation"], "trash_personas");
    assert_eq!(
        details["succeeded_persona_ids"],
        serde_json::json!(["persona-a"])
    );
    assert_eq!(details["failed"]["persona_id"], "persona-b");
    assert_eq!(details["failed"]["code"], "ambiguous_mutation");
    assert_eq!(details["failed"]["details"]["stage"], "response_status");
    assert_eq!(
        details["not_attempted_persona_ids"],
        serde_json::json!(["persona-c"])
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn persona_per_id_mutation_rejects_non_json_success_body() {
    let server = MockServer::json("not-json").await;
    let client = server.client();

    let error = client
        .restore_personas(&["persona-a".into()])
        .await
        .expect_err("unknown success schema must not be ignored");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(error.details().expect("details")["stage"], "response_body");
}

#[tokio::test]
async fn persona_per_id_mutation_requires_the_requested_id_in_success_body() {
    let server = MockServer::json(
        r#"{"updated_persona_ids":["persona-other"],"voice_persona_count":4,"max_voice_personas":1000}"#,
    )
    .await;
    let client = server.client();

    let error = client
        .trash_personas(&["persona-a".into()])
        .await
        .expect_err("a response for another persona cannot confirm this write");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity details")["stage"],
        "response_state"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn list_personas_uses_scope_page_and_continuation_query() {
    let server = MockServer::json(r#"{"personas":[],"total_results":0,"current_page":2}"#).await;
    let client = server.client();

    client
        .list_personas(PersonaListScope::Mine, 2, Some("next-token"))
        .await
        .expect("list personas");

    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path,
        "/api/persona/get-personas/?page=2&continuation_token=next-token"
    );
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn loved_personas_reject_current_web_unsupported_continuation_tokens() {
    let server = MockServer::json(r#"{"personas":[]}"#).await;
    let client = server.client();

    let error = client
        .list_personas(PersonaListScope::Loved, 2, Some("next-token"))
        .await
        .expect_err("loved continuation token must stop locally");

    assert!(matches!(error, CliError::Config(message) if message.contains("page numbers only")));
    assert!(server.captured_all().await.is_empty());
}

#[tokio::test]
async fn create_audio_upload_posts_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"upload-1","url":"https://s3.example/upload","fields":{"key":"audio/upload-1","policy":"policy-1"}}"#,
    )
    .await;
    let client = server.client();

    let upload = client
        .create_audio_upload(&CreateAudioUploadRequest {
            spec: CreateAudioUploadSpec {
                extension: "mp3".into(),
                is_stem_mix: false,
                upload_type: "file_upload".into(),
            },
        })
        .await
        .expect("create audio upload");

    assert_eq!(upload.id, "upload-1");
    assert_eq!(
        upload.fields.get("key").map(String::as_str),
        Some("audio/upload-1")
    );
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/uploads/audio/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "extension": "mp3",
            "is_stem_mix": false,
            "upload_type": "file_upload"
        })
    );
}

#[tokio::test]
async fn finish_audio_upload_posts_current_web_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .finish_audio_upload(
            "upload-1",
            &FinishAudioUploadRequest {
                upload_type: "file_upload".into(),
                upload_filename: "demo.mp3".into(),
                agreed_to_vip_upload_terms: false,
            },
        )
        .await
        .expect("finish audio upload");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/uploads/audio/upload-1/upload-finish/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "upload_type": "file_upload",
            "upload_filename": "demo.mp3",
            "agreed_to_vip_upload_terms": false
        })
    );
}

#[tokio::test]
async fn get_audio_upload_fetches_current_status_contract() {
    let server = MockServer::json(
        r#"{"id":"upload-1","status":"complete","title":"Demo","image_url":"https://cdn.example/cover.jpg","has_vocal":true,"copyright_muted":false}"#,
    )
    .await;
    let client = server.client();

    let status = client
        .get_audio_upload("upload-1")
        .await
        .expect("get audio upload");

    assert_eq!(status.id.as_deref(), Some("upload-1"));
    assert_eq!(status.status.as_deref(), Some("complete"));
    assert_eq!(status.has_vocal, Some(true));
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/uploads/audio/upload-1/");
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn initialize_audio_clip_posts_current_web_contract() {
    let server = MockServer::json(r#"{"clip_id":"clip-1"}"#).await;
    let client = server.client();

    let response = client
        .initialize_audio_clip(
            "upload-1",
            &InitializeAudioClipRequest {
                downbeats: Some(vec![0.0, 1.25]),
                user_reviewed_tags: None,
            },
        )
        .await
        .expect("initialize audio clip");

    assert_eq!(response.clip_id.as_deref(), Some("clip-1"));
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/uploads/audio/upload-1/initialize-clip/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "downbeats": [0.0, 1.25] })
    );
}

#[tokio::test]
async fn create_image_upload_posts_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"image-upload-1","url":"https://s3.example/upload","fields":{"key":"raw_uploads/image-upload-1.png","Content-Type":"image/png","policy":"policy-1"}}"#,
    )
    .await;
    let client = server.client();

    let upload = client
        .create_image_upload(&CreateImageUploadRequest {
            extension: "png".into(),
        })
        .await
        .expect("create image upload");

    assert_eq!(upload.id, "image-upload-1");
    assert_eq!(
        upload.fields.get("Content-Type").map(String::as_str),
        Some("image/png")
    );
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/uploads/image/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "extension": "png" })
    );
}

#[tokio::test]
async fn finish_image_upload_posts_current_web_contract() {
    let server = MockServer::json(r#"{"moderation_status":"approved"}"#).await;
    let client = server.client();

    let response = client
        .finish_image_upload("image-upload-1")
        .await
        .expect("finish image upload");

    assert_eq!(response.moderation_status.as_deref(), Some("approved"));
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        "/api/uploads/image/image-upload-1/upload-finish/"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({})
    );
}

#[tokio::test]
async fn image_upload_workflow_preserves_upload_identity_when_finish_fails() {
    let s3 = MockServer::json("{}").await;
    let create_response = serde_json::json!({
        "id": "image-upload-1",
        "url": format!("{}/s3-upload", s3.base_url),
        "fields": {
            "key": "raw_uploads/image-upload-1.png",
            "Content-Type": "image/png"
        }
    })
    .to_string();
    let api = MockServer::response_sequence(vec![
        (200, create_response),
        (500, r#"{"detail":"finish failed"}"#.to_string()),
    ])
    .await;
    let client = api.client();
    let dir = tempfile::tempdir().expect("image upload tempdir");
    let path = dir.path().join("cover.png");
    std::fs::write(&path, b"image-bytes").expect("write image fixture");

    let error = crate::workflow::image_upload::run(&client, &path)
        .await
        .expect_err("finish failure must expose the created image upload");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("image upload checkpoint");
    assert_eq!(details["operation"], "image_upload_finish");
    assert_eq!(details["resource_id"], "image-upload-1");
    assert_eq!(
        details["completed_steps"],
        serde_json::json!(["upload_created", "file_uploaded"])
    );
    assert_eq!(details["failed_step"], "upload_finish");
    assert_eq!(details["stage"], "response_status");
    assert_eq!(details["recovery"]["resumable"], false);
    assert_eq!(s3.captured().await.path, "/s3-upload");
    assert_eq!(api.captured_all().await.len(), 2);
}

#[tokio::test]
async fn audio_upload_workflow_returns_clip_after_metadata_update() {
    let s3 = MockServer::json("{}").await;
    let create_response = serde_json::json!({
        "id": "audio-upload-1",
        "url": format!("{}/s3-upload", s3.base_url),
        "fields": { "key": "raw_uploads/audio-upload-1.mp3" }
    })
    .to_string();
    let stale_clip = serde_json::json!({
        "id": "clip-1",
        "title": "Original title",
        "status": "complete",
        "model_name": "upload",
        "audio_url": "https://cdn.example/original.mp3",
        "video_url": null,
        "image_url": null,
        "created_at": "2026-07-10T00:00:00Z",
        "metadata": {}
    });
    let final_clip = serde_json::json!({
        "id": "clip-1",
        "title": "Final title",
        "status": "complete",
        "model_name": "upload",
        "audio_url": "https://cdn.example/original.mp3",
        "video_url": null,
        "image_url": null,
        "created_at": "2026-07-10T00:00:00Z",
        "metadata": { "prompt": "Final lyrics" }
    });
    let api = MockServer::response_sequence(vec![
        (200, create_response),
        (200, "{}".to_string()),
        (
            200,
            r#"{"id":"audio-upload-1","status":"complete"}"#.to_string(),
        ),
        (
            200,
            serde_json::json!({ "clip_id": "clip-1", "clip": stale_clip.clone() }).to_string(),
        ),
        (200, "{}".to_string()),
        (200, stale_clip.to_string()),
        (200, final_clip.to_string()),
    ])
    .await;
    let client = api.client();
    let dir = tempfile::tempdir().expect("audio upload tempdir");
    let path = dir.path().join("demo.mp3");
    std::fs::write(&path, b"audio-bytes").expect("write audio fixture");

    let result = crate::workflow::upload::run(
        &client,
        crate::workflow::upload::UploadWorkflowInput {
            file: &path,
            upload_type: "file_upload",
            is_stem_mix: false,
            title: Some("Final title".into()),
            lyrics: Some("Final lyrics".into()),
            timeout: Duration::from_secs(3),
            poll_interval: Duration::from_millis(1),
        },
    )
    .await
    .expect("audio upload workflow");

    let clip = result.clip.expect("final clip");
    assert_eq!(clip.title, "Final title");
    assert_eq!(clip.metadata.prompt.as_deref(), Some("Final lyrics"));
    assert_eq!(s3.captured().await.path, "/s3-upload");
    assert_eq!(api.captured_all().await.len(), 7);
}

#[tokio::test]
async fn upload_presigned_audio_form_posts_s3_multipart_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();
    let dir = tempfile::tempdir().expect("audio upload tempdir");
    let path = dir.path().join("demo.mp3");
    std::fs::write(&path, b"audio-bytes").expect("write audio fixture");
    let file = tokio::fs::File::open(&path)
        .await
        .expect("open audio fixture");

    client
        .upload_presigned_audio_file(
            &format!("{}/s3-upload", server.base_url),
            &[
                ("key".into(), "audio/upload-1".into()),
                ("policy".into(), "p".into()),
            ]
            .into_iter()
            .collect(),
            "demo.mp3",
            file,
            11,
        )
        .await
        .expect("upload presigned form");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/s3-upload");
    assert!(request.headers.contains("multipart/form-data"));
    assert!(request.body.contains("name=\"key\""));
    assert!(request.body.contains("audio/upload-1"));
    assert!(request.body.contains("name=\"file\""));
    assert!(request.body.contains("filename=\"demo.mp3\""));
    assert!(request.body.contains("audio-bytes"));
}

#[tokio::test]
async fn generation_submit_reports_an_ambiguous_accepted_response_body() {
    let server = MockServer::json("{").await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-fenix", "custom");
    request.metadata.user_tier = "tier-pro".into();
    request.set_challenge_token(Some("captcha-token".into()));
    let transaction_uuid = request.transaction_uuid.clone();

    let error = client
        .submit_prepared_generation_after_challenge(&request)
        .await
        .expect_err("an unreadable accepted response must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "generation_submit");
    assert_eq!(details["transaction_uuid"], transaction_uuid);
    assert_eq!(details["stage"], "response_body");
    assert_eq!(details["recovery"]["resumable"], false);
    assert!(error.suggestion().contains("Do not blindly retry"));
    assert_eq!(server.captured().await.path, "/api/generate/v2-web/");
}

#[tokio::test]
async fn generation_submit_reports_ambiguous_missing_clips_after_success() {
    let server = MockServer::json(r#"{"status":"submitted"}"#).await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-fenix", "custom");
    request.metadata.user_tier = "tier-pro".into();
    request.set_challenge_token(Some("captcha-token".into()));

    let error = client
        .submit_prepared_generation_after_challenge(&request)
        .await
        .expect_err("a successful response without clips must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["transaction_uuid"], request.transaction_uuid);
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(details["recovery"]["resumable"], false);
}

#[tokio::test]
async fn generation_submit_rejects_a_blank_clip_id_as_ambiguous_schema_drift() {
    let server = MockServer::json(
        r#"{"clips":[{"id":"   ","title":"Untethered","status":"submitted","model_name":"chirp-fenix","created_at":"2026-08-24T00:00:00Z"}]}"#,
    )
    .await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-fenix", "custom");
    request.metadata.user_tier = "tier-pro".into();
    request.set_challenge_token(Some("captcha-token".into()));

    let error = client
        .submit_prepared_generation_after_challenge(&request)
        .await
        .expect_err("a generated clip without a recovery ID must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["transaction_uuid"], request.transaction_uuid);
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(details["recovery"]["resumable"], false);
}

#[tokio::test]
async fn generation_submit_reports_an_ambiguous_send_failure() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused loopback address");
    let addr = listener.local_addr().expect("unused loopback address");
    drop(listener);
    let client = SunoClient::new_for_tests(
        format!("http://{addr}"),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");
    let mut request = GenerateRequest::new("chirp-fenix", "custom");
    request.metadata.user_tier = "tier-pro".into();
    request.set_challenge_token(Some("captcha-token".into()));

    let error = client
        .submit_prepared_generation_after_challenge(&request)
        .await
        .expect_err("send failure must be treated as ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["transaction_uuid"], request.transaction_uuid);
    assert_eq!(details["stage"], "request_send");
}

#[tokio::test]
async fn generation_submit_treats_server_error_as_ambiguous() {
    let server = MockServer::json_status_sequence(&[(
        500,
        r#"{"detail":"generation rejected","retryable":false}"#,
    )])
    .await;
    let client = server.client();
    let mut request = GenerateRequest::new("chirp-fenix", "custom");
    request.metadata.user_tier = "tier-pro".into();
    request.set_challenge_token(Some("captcha-token".into()));

    let error = client
        .submit_prepared_generation_after_challenge(&request)
        .await
        .expect_err("5xx cannot prove generation was rejected before commit");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn crop_poll_transport_failure_keeps_the_returned_action_id() {
    let server = MockServer::json_sequence(&[r#"{"action_clip_id":"crop-1"}"#, "{"]).await;
    let client = server.client();

    let error = client
        .crop_clip(
            "clip-a",
            1.0,
            2.0,
            false,
            "Crop",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("post-submit poll failure must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "crop");
    assert_eq!(details["action_clip_id"], "crop-1");
    assert_eq!(details["stage"], "action_poll");
    assert_eq!(details["recovery"]["resumable"], true);
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn crop_submit_response_body_loss_is_ambiguous_without_replay() {
    let server = MockServer::json("{").await;
    let client = server.client();

    let error = client
        .crop_clip(
            "clip-a",
            1.0,
            2.0,
            false,
            "Crop",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("an unreadable accepted crop response must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "crop");
    assert_eq!(details["source_clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_body");
    assert_eq!(details["recovery"]["resumable"], false);
    assert!(
        details["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn crop_submit_rejects_a_blank_action_id_as_non_resumable_ambiguity() {
    let server = MockServer::json(r#"{"action_clip_id":"   "}"#).await;
    let client = server.client();

    let error = client
        .crop_clip(
            "clip-a",
            1.0,
            2.0,
            false,
            "Crop",
            super::PollingOptions {
                timeout: Duration::from_millis(20),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a blank server action id cannot support safe polling recovery");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "crop");
    assert_eq!(details["stage"], "response_body");
    assert_eq!(details["recovery"]["resumable"], false);
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn reverse_submit_send_failure_has_a_local_non_resumable_operation_id() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused loopback address");
    let addr = listener.local_addr().expect("unused loopback address");
    drop(listener);
    let client = SunoClient::new_for_tests(
        format!("http://{addr}"),
        AuthState {
            jwt: Some("test-jwt".into()),
            ..AuthState::default()
        },
    )
    .expect("test client");

    let error = client
        .reverse_clip("clip-a", "Reversed")
        .await
        .expect_err("an uncertain reverse send must not invite replay");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "reverse");
    assert_eq!(details["source_clip_id"], "clip-a");
    assert_eq!(details["stage"], "request_send");
    assert_eq!(details["recovery"]["resumable"], false);
    assert!(
        details["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
}

#[tokio::test]
async fn reverse_submit_rejects_a_blank_clip_id_as_non_resumable_ambiguity() {
    let server = MockServer::json(
        r#"{"id":"   ","title":"Reversed","status":"processing","model_name":"chirp-fenix","created_at":"2026-08-24T00:00:00Z"}"#,
    )
    .await;
    let client = server.client();

    let error = client
        .reverse_clip("clip-a", "Reversed")
        .await
        .expect_err("a reverse result without a recovery ID must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "reverse");
    assert_eq!(details["source_clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(details["recovery"]["resumable"], false);
    assert!(
        details["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn fade_poll_api_error_keeps_the_returned_action_id() {
    let server = MockServer::json_status_sequence(&[
        (200, r#"{"action_clip_id":"fade-1"}"#),
        (500, r#"{"detail":"poll unavailable"}"#),
    ])
    .await;
    let client = server.client();

    let error = client
        .fade_clip(
            "clip-a",
            Some(1.0),
            None,
            "Fade In",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a poll API error must preserve the submitted action identity");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "fade");
    assert_eq!(details["action_clip_id"], "fade-1");
    assert_eq!(details["stage"], "action_poll");
    assert_eq!(details["recovery"]["resumable"], true);
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn wav_no_convert_fails_closed_after_reading_the_existing_url() {
    let server = MockServer::json(r#"{"wav_file_url":null}"#).await;
    let client = server.client();

    let error = client
        .download_url_with_conversion_policy(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
            false,
        )
        .await
        .expect_err("no-convert must refuse a conversion POST");

    assert!(matches!(error, CliError::Download(message) if message.contains("refused")));
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/wav_file/");
}

#[tokio::test]
async fn opus_no_convert_fails_closed_after_reading_the_existing_url() {
    let server = MockServer::json(r#"{"opus_file_url":null}"#).await;
    let client = server.client();

    let error = client
        .download_url_with_conversion_policy(
            "clip-a",
            super::download::DownloadFormat::Opus,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
            false,
        )
        .await
        .expect_err("no-convert must refuse a conversion POST");

    assert!(matches!(error, CliError::Download(message) if message.contains("refused")));
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/opus_file/");
}

#[tokio::test]
async fn conversion_poll_transport_failure_is_ambiguous_after_submit() {
    let server = MockServer::json_sequence(&[r#"{"wav_file_url":null}"#, "{}", "{"]).await;
    let client = server.client();

    let error = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("post-submit conversion poll failure must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "convert_wav");
    assert_eq!(details["clip_id"], "clip-a");
    assert_eq!(details["stage"], "file_poll");
    assert_eq!(server.captured_all().await.len(), 3);
}

#[tokio::test]
async fn conversion_poll_api_error_is_ambiguous_after_submit() {
    let server = MockServer::json_status_sequence(&[
        (200, r#"{"wav_file_url":null}"#),
        (200, "{}"),
        (500, r#"{"detail":"conversion poll unavailable"}"#),
    ])
    .await;
    let client = server.client();

    let error = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a post-submit poll API error must remain ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "convert_wav");
    assert_eq!(details["clip_id"], "clip-a");
    assert_eq!(details["stage"], "file_poll");
    assert_eq!(details["recovery"]["resumable"], true);
    assert_eq!(server.captured_all().await.len(), 3);
}

#[tokio::test]
async fn conversion_poll_timeout_is_ambiguous_after_submit() {
    let server =
        MockServer::json_sequence(&[r#"{"wav_file_url":null}"#, "{}", r#"{"wav_file_url":null}"#])
            .await;
    let client = server.client();

    let error = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_millis(50),
                interval: Duration::from_secs(1),
            },
        )
        .await
        .expect_err("post-submit conversion timeout must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "convert_wav");
    assert_eq!(details["stage"], "file_poll");
    assert_eq!(details["cause"]["code"], "download_error");
    assert_eq!(server.captured_all().await.len(), 3);
}

#[tokio::test]
async fn conversion_treats_server_error_as_ambiguous() {
    let server = MockServer::json_status_sequence(&[
        (200, r#"{"wav_file_url":null}"#),
        (500, r#"{"detail":"conversion rejected"}"#),
    ])
    .await;
    let client = server.client();

    let error = client
        .download_url(
            "clip-a",
            super::download::DownloadFormat::Wav,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("5xx cannot prove conversion was rejected before commit");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn existing_aligned_lyrics_is_get_only() {
    let server = MockServer::json(
        r#"{"aligned_words":[{"word":"Hello","start_s":0.0,"end_s":0.5,"success":true}]}"#,
    )
    .await;
    let client = server.client();

    let words = client
        .existing_aligned_lyrics(
            "clip-a",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("existing aligned lyrics");

    assert_eq!(words[0].word, "Hello");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/aligned_lyrics/v2");
}

#[tokio::test]
async fn remaster_submit_reports_an_ambiguous_accepted_response_body() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-08-24T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let server = MockServer::json_sequence(&[source, "{"]).await;
    let client = server.client();

    let error = client
        .remaster(
            "clip-a",
            "chirp-flounder",
            Some(crate::api::types::RemasterVariation::High),
        )
        .await
        .expect_err("an unreadable accepted response must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "remaster");
    assert_eq!(details["source_clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_body");
    assert_eq!(details["recovery"]["resumable"], false);
    assert!(
        details["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn remaster_submit_reports_ambiguous_valid_json_schema_drift() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-08-24T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let server = MockServer::json_sequence(&[source, r#"{"clips":[{"id":17}]}"#]).await;
    let client = server.client();

    let error = client
        .remaster("clip-a", "chirp-flounder", None)
        .await
        .expect_err("an accepted response with an unusable clip schema must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "remaster");
    assert_eq!(details["source_clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "json_error");
    assert_eq!(details["recovery"]["resumable"], false);
}

#[tokio::test]
async fn remaster_submit_rejects_an_empty_clip_list_as_ambiguous_schema_drift() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-08-24T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let server = MockServer::json_sequence(&[source, r#"{"clips":[]}"#]).await;
    let client = server.client();

    let error = client
        .remaster("clip-a", "chirp-flounder", None)
        .await
        .expect_err("an accepted Remaster response without a recovery clip must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "remaster");
    assert_eq!(details["source_clip_id"], "clip-a");
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(details["recovery"]["resumable"], false);
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn remaster_submit_rejects_a_blank_clip_id_as_ambiguous_schema_drift() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-08-24T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let result = r#"{"clips":[{"id":"  ","title":"Remaster","status":"submitted","model_name":"chirp-flounder","created_at":"2026-08-24T00:00:00Z"}]}"#;
    let server = MockServer::json_sequence(&[source, result]).await;
    let client = server.client();

    let error = client
        .remaster("clip-a", "chirp-flounder", None)
        .await
        .expect_err("an accepted Remaster clip without a recovery ID must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "remaster");
    assert_eq!(details["stage"], "response_schema");
    assert_eq!(details["cause"]["code"], "schema_drift");
    assert_eq!(details["recovery"]["resumable"], false);
    assert!(
        details["operation_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn remaster_submit_treats_server_error_as_ambiguous() {
    let source = r#"{"id":"clip-a","title":"Source","status":"complete","model_name":"chirp-carp","created_at":"2026-08-24T00:00:00Z","is_trashed":false,"metadata":{"duration":180.0},"action_config":{"actions":[{"action_type":"remaster","visible":true,"disabled":false}]}}"#;
    let server = MockServer::json_status_sequence(&[
        (200, source),
        (500, r#"{"detail":"remaster rejected","retryable":false}"#),
    ])
    .await;
    let client = server.client();

    let error = client
        .remaster("clip-a", "chirp-flounder", None)
        .await
        .expect_err("5xx cannot prove Remaster was rejected before commit");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert_eq!(server.captured_all().await.len(), 2);
}

fn complete_clip_fixture(id: &str, action: Option<&str>, image_url: Option<&str>) -> String {
    let actions = action
        .map(|action| {
            serde_json::json!([{
                "action_type": action,
                "visible": true,
                "disabled": false,
            }])
        })
        .unwrap_or_else(|| serde_json::json!([]));
    serde_json::json!({
        "id": id,
        "user_id": "user-1",
        "title": "Verified source",
        "status": "complete",
        "model_name": "chirp-fenix",
        "created_at": "2026-08-24T00:00:00Z",
        "is_trashed": false,
        "image_url": image_url,
        "action_config": {"actions": actions}
    })
    .to_string()
}

fn billing_with_features(features: &[&str]) -> String {
    let mut billing = serde_json::from_str::<serde_json::Value>(&billing_info_response("tier-pro"))
        .expect("billing fixture");
    billing["accessible_features"] = serde_json::json!(features);
    billing.to_string()
}

fn lyrics_project_fixture(id: &str, title: &str, lyrics: &str, updated_at: &str) -> String {
    serde_json::json!({
        "id": id,
        "title": title,
        "lyrics": lyrics,
        "created_at": "2026-08-24T00:00:00Z",
        "updated_at": updated_at,
        "future_project_field": {"preserved": true}
    })
    .to_string()
}

fn parsed_lyrics_project_generation_request(project_id: &str) -> GenerateRequest {
    let cli = Cli::try_parse_from([
        "sunox",
        "create",
        "--lyrics",
        "[Verse]\nhello",
        "--lyrics-project-id",
        project_id,
        "--model",
        "chirp-v4-5",
    ])
    .expect("lyrics project generation CLI");
    let Some(Commands::Create(args)) = cli.command else {
        panic!("expected create command");
    };
    let args = crate::commands::create::build_generate_args_from_create(args);
    crate::commands::create::build_generate_request(&args, &AppConfig::default())
        .expect("generation request")
}

#[tokio::test]
async fn custom_model_create_preflights_every_source_then_posts_the_exact_contract() {
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();
    let mut responses = vec![(200, billing_with_features(&["custom_models"]))];
    responses.extend(
        clip_ids
            .iter()
            .map(|clip_id| (200, complete_clip_fixture(clip_id, None, None))),
    );
    responses.push((200, r#"{"id":"custom-1","training_eta_s":120}"#.to_string()));
    responses.push((
        200,
        r#"{"has_pending":true,"pending_models":[{"id":"custom-1","name":"My Sound"}]}"#
            .to_string(),
    ));
    responses.push((200, billing_info_response("tier-pro")));
    let server = MockServer::response_sequence(responses).await;
    let client = server.client();

    let created = client
        .create_custom_model(&clip_ids, "My Sound", true)
        .await
        .expect("create custom model");

    assert_eq!(created.id, "custom-1");
    assert_eq!(created.extra["training_eta_s"], 120);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 10);
    assert_eq!(requests[0].path, "/api/billing/info/");
    for (index, clip_id) in clip_ids.iter().enumerate() {
        assert_eq!(requests[index + 1].method, "GET");
        assert_eq!(requests[index + 1].path, format!("/api/clip/{clip_id}"));
    }
    assert_eq!(requests[7].method, "POST");
    assert_eq!(requests[7].path, "/api/custom-model/create/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[7].body).expect("create body"),
        serde_json::json!({"clip_ids": clip_ids, "name": "My Sound"})
    );
    assert_eq!(requests[8].path, "/api/custom-model/pending/");
    assert_eq!(requests[9].path, "/api/billing/info/");
}

#[tokio::test]
async fn custom_model_create_rejects_an_unusable_source_before_training_submit() {
    let source = serde_json::json!({
        "id": "clip-1",
        "user_id": "user-1",
        "title": "Still rendering",
        "status": "processing",
        "model_name": "chirp-fenix",
        "created_at": "2026-08-24T00:00:00Z",
        "is_trashed": false
    })
    .to_string();
    let server = MockServer::json(&source).await;
    let client = server.client();
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();

    let error = client
        .create_custom_model_after_ui_gate(
            &clip_ids,
            "My Sound",
            super::PollingOptions {
                timeout: Duration::from_millis(200),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("processing source must stop training");

    assert!(matches!(error, CliError::Config(message) if message.contains("must be complete")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
}

#[tokio::test]
async fn custom_model_create_requires_every_source_to_match_the_authenticated_owner() {
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();
    let mut mismatched =
        serde_json::from_str::<serde_json::Value>(&complete_clip_fixture("clip-1", None, None))
            .expect("clip fixture");
    mismatched["user_id"] = serde_json::json!("user-other");
    let server = MockServer::json(&mismatched.to_string()).await;
    let client = server.client();

    let error = client
        .create_custom_model_after_ui_gate(
            &clip_ids,
            "My Sound",
            super::PollingOptions {
                timeout: Duration::from_millis(200),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a source owned by another account must stop training");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("owned by the authenticated account"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
}

#[tokio::test]
async fn custom_model_create_api_rejects_duplicate_sources_before_network_access() {
    let server = MockServer::json("{}").await;
    let client = server.client();
    let clip_ids = vec!["clip-1".to_string(); 6];

    let error = client
        .create_custom_model(&clip_ids, "My Sound", true)
        .await
        .expect_err("duplicate sources must fail at the API boundary");

    assert!(matches!(error, CliError::Config(message) if message.contains("duplicated")));
    assert!(server.captured_all().await.is_empty());
}

#[tokio::test]
async fn custom_model_create_requires_the_account_feature_before_clip_reads() {
    let server = MockServer::json(&billing_with_features(&[])).await;
    let client = server.client();
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();

    let error = client
        .create_custom_model(&clip_ids, "My Sound", true)
        .await
        .expect_err("missing Custom Model entitlement must fail closed");

    assert!(matches!(error, CliError::Config(message) if message.contains("custom_models")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn custom_model_create_requires_explicit_ui_availability_confirmation() {
    let billing = billing_with_features(&["custom_models"]);
    let server = MockServer::json(&billing).await;
    let client = server.client();
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();

    let error = client
        .create_custom_model(&clip_ids, "My Sound", false)
        .await
        .expect_err("missing UI availability confirmation must fail closed");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("custom-model-ui") && message.contains("--confirm-ui-available"))
    );
    let requests = server.captured_all().await;
    assert_eq!(
        requests.len(),
        1,
        "gate uncertainty must stop before clip reads"
    );
    assert_eq!(requests[0].path, "/api/billing/info/");
}

#[tokio::test]
async fn custom_model_create_reports_ambiguous_missing_id_without_replay() {
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();
    let mut responses = Vec::new();
    responses.extend(
        clip_ids
            .iter()
            .map(|clip_id| (200, complete_clip_fixture(clip_id, None, None))),
    );
    responses.push((200, r#"{"name":"missing recovery id"}"#.to_string()));
    let server = MockServer::response_sequence(responses).await;
    let client = server.client();

    let error = client
        .create_custom_model_after_ui_gate(
            &clip_ids,
            "My Sound",
            super::PollingOptions {
                timeout: Duration::from_millis(200),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("missing create ID must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["operation"], "custom_model_create");
    assert_eq!(details["recovery"]["resumable"], false);
    assert_eq!(server.captured_all().await.len(), 7);
}

#[tokio::test]
async fn custom_model_create_treats_a_server_error_after_submit_as_ambiguous() {
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();
    let mut responses = clip_ids
        .iter()
        .map(|clip_id| (200, complete_clip_fixture(clip_id, None, None)))
        .collect::<Vec<_>>();
    responses.push((500, r#"{"error":"unknown accepted state"}"#.into()));
    let server = MockServer::response_sequence(responses).await;
    let client = server.client();

    let error = client
        .create_custom_model_after_ui_gate(
            &clip_ids,
            "My Sound",
            super::PollingOptions {
                timeout: Duration::from_millis(200),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("5xx after training submit has an unknown accepted state");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert_eq!(server.captured_all().await.len(), 7);
}

#[tokio::test]
async fn custom_model_create_readback_converges_without_replaying_training() {
    let clip_ids = (1..=6)
        .map(|index| format!("clip-{index}"))
        .collect::<Vec<_>>();
    let mut responses = Vec::new();
    responses.extend(
        clip_ids
            .iter()
            .map(|clip_id| (200, complete_clip_fixture(clip_id, None, None))),
    );
    responses.push((200, r#"{"id":"custom-1"}"#.to_string()));
    responses.push((
        200,
        r#"{"has_pending":false,"pending_models":[]}"#.to_string(),
    ));
    responses.push((200, billing_info_response("tier-pro")));
    responses.push((
        200,
        r#"{"has_pending":true,"pending_models":[{"id":"custom-1","name":"My Sound"}]}"#
            .to_string(),
    ));
    responses.push((200, billing_info_response("tier-pro")));
    let server = MockServer::response_sequence(responses).await;
    let client = server.client();

    let created = client
        .create_custom_model_after_ui_gate(
            &clip_ids,
            "My Sound",
            super::PollingOptions {
                timeout: Duration::from_millis(200),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("eventually visible training should be confirmed");

    assert_eq!(created.id, "custom-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 11);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1,
        "readback convergence must not replay training"
    );
}

#[tokio::test]
async fn custom_model_archive_posts_then_proves_absence_in_pending_and_billing() {
    let before_pending =
        r#"{"has_pending":true,"pending_models":[{"id":"custom-1","name":"My Sound"}]}"#;
    let before_models = serde_json::json!([{
        "name": "My Sound",
        "external_key": "custom-1",
        "can_use": true,
        "is_default_model": false,
        "description": "trained model",
        "badges": ["custom"],
        "max_lengths": {}
    }]);
    let before_billing = billing_info_with_models("tier-pro", before_models);
    let after_billing = billing_info_with_models("tier-pro", serde_json::json!([]));
    let server = MockServer::response_sequence(vec![
        (200, before_pending.to_string()),
        (200, before_billing),
        (200, "{}".into()),
        (200, r#"{"has_pending":false,"pending_models":[]}"#.into()),
        (200, after_billing),
    ])
    .await;
    let client = server.client();

    client
        .archive_custom_model("custom-1")
        .await
        .expect("archive with readback");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "/api/custom-model/pending/");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/custom-model/archive/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("archive body"),
        serde_json::json!({"id": "custom-1"})
    );
    assert_eq!(requests[3].path, "/api/custom-model/pending/");
    assert_eq!(requests[4].path, "/api/billing/info/");
}

#[tokio::test]
async fn custom_model_archive_treats_a_server_error_after_submit_as_ambiguous() {
    let pending = r#"{"has_pending":true,"pending_models":[{"id":"custom-1","name":"My Sound"}]}"#;
    let server = MockServer::response_sequence(vec![
        (200, pending.into()),
        (200, billing_info_response("tier-pro")),
        (500, r#"{"error":"unknown accepted state"}"#.into()),
    ])
    .await;
    let client = server.client();

    let error = client
        .archive_custom_model("custom-1")
        .await
        .expect_err("5xx after archive submit has an unknown accepted state");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert_eq!(server.captured_all().await.len(), 3);
}

#[tokio::test]
async fn ready_only_custom_model_archive_converges_without_replaying_the_write() {
    let pending = r#"{"has_pending":false,"pending_models":[]}"#;
    let ready_models = serde_json::json!([{
        "id": "custom-1",
        "name": "My Sound",
        "external_key": "chirp-custom-1",
        "can_use": true,
        "is_default_model": false,
        "description": "trained model",
        "badges": ["custom"],
        "max_lengths": {}
    }]);
    let ready_billing = billing_info_with_models("tier-pro", ready_models);
    let absent_billing = billing_info_with_models("tier-pro", serde_json::json!([]));
    let server = MockServer::response_sequence(vec![
        (200, pending.into()),
        (200, ready_billing.clone()),
        (200, "{}".into()),
        (200, pending.into()),
        (200, ready_billing),
        (200, pending.into()),
        (200, absent_billing),
    ])
    .await;
    let client = server.client();

    client
        .archive_custom_model_with_readback(
            "custom-1",
            super::PollingOptions {
                timeout: Duration::from_millis(200),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("ready-only archive should converge through read-only polling");

    let requests = server.captured_all().await;
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1,
        "archive must never be replayed during convergence"
    );
    assert_eq!(requests.len(), 7);
}

#[tokio::test]
async fn custom_model_archive_does_not_treat_a_base_model_custom_badge_as_identity() {
    let pending = r#"{"has_pending":false,"pending_models":[]}"#;
    let base_models = serde_json::json!([{
        "id": "base-model-1",
        "name": "v5.5",
        "external_key": "chirp-fenix",
        "can_use": true,
        "is_default_model": true,
        "description": "base model that supports custom prompting",
        "badges": ["custom"],
        "max_lengths": {}
    }]);
    let billing = billing_info_with_models("tier-pro", base_models);
    let server = MockServer::json_sequence(&[pending, &billing]).await;
    let client = server.client();

    let error = client
        .archive_custom_model("base-model-1")
        .await
        .expect_err("base model must fail closed before archive submit");

    assert!(matches!(error, CliError::NotFound(_)));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn lyrics_projects_list_follows_the_exact_cursor_contract() {
    let first = serde_json::json!({
        "projects": [serde_json::from_str::<serde_json::Value>(&lyrics_project_fixture(
            "project-1", "First", "A", "2026-08-24T01:00:00Z"
        )).expect("project fixture")],
        "next_cursor": "cursor-2"
    })
    .to_string();
    let second = serde_json::json!({
        "projects": [serde_json::from_str::<serde_json::Value>(&lyrics_project_fixture(
            "project-2", "Second", "B", "2026-08-24T00:00:00Z"
        )).expect("project fixture")],
        "next_cursor": null
    })
    .to_string();
    let server = MockServer::json_sequence(&[&first, &second]).await;
    let client = server.client();

    let projects = client.lyrics_projects().await.expect("project list");

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].extra["future_project_field"]["preserved"], true);
    let requests = server.captured_all().await;
    assert_eq!(
        requests[0].path,
        "/api/lyrics-projects?limit=50&sort=updated_at"
    );
    assert_eq!(
        requests[1].path,
        "/api/lyrics-projects?limit=50&sort=updated_at&cursor=cursor-2"
    );
}

#[tokio::test]
async fn lyrics_projects_list_rejects_a_missing_projects_field() {
    let server = MockServer::json(r#"{"next_cursor":null}"#).await;
    let client = server.client();

    let error = client
        .lyrics_projects()
        .await
        .expect_err("the list envelope must contain projects");

    assert_eq!(error.error_code(), "schema_drift");

    let empty = MockServer::json(r#"{"projects":[],"next_cursor":null}"#).await;
    let client = empty.client();
    assert!(
        client
            .lyrics_projects()
            .await
            .expect("a present empty projects array is valid")
            .is_empty()
    );
}

#[tokio::test]
async fn lyrics_project_get_rejects_a_mismatched_response_identity() {
    let response =
        lyrics_project_fixture("project-other", "Draft", "lyrics", "2026-08-24T00:00:00Z");
    let server = MockServer::json(&response).await;
    let client = server.client();

    let error = client
        .lyrics_project("project-1")
        .await
        .expect_err("a project GET must preserve the requested identity");

    assert_eq!(error.error_code(), "schema_drift");
    assert!(error.to_string().contains("project-other"));
}

#[tokio::test]
async fn lyrics_project_get_requires_title_and_lyrics_fields_even_when_they_may_be_empty() {
    for response in [
        r#"{"id":"project-1","lyrics":"","created_at":"2026-08-24T00:00:00Z","updated_at":"2026-08-24T00:00:00Z"}"#,
        r#"{"id":"project-1","title":"","created_at":"2026-08-24T00:00:00Z","updated_at":"2026-08-24T00:00:00Z"}"#,
    ] {
        let server = MockServer::json(response).await;
        let client = server.client();

        let error = client
            .lyrics_project("project-1")
            .await
            .expect_err("consumed project fields must be present");

        assert_eq!(error.error_code(), "schema_drift");
    }

    let empty = lyrics_project_fixture("project-1", "", "", "2026-08-24T00:00:00Z");
    let server = MockServer::json(&empty).await;
    let client = server.client();
    client
        .lyrics_project("project-1")
        .await
        .expect("present empty values remain valid");
}

#[tokio::test]
async fn generation_with_lyrics_project_preflights_identity_then_posts_the_exact_reference() {
    let project = lyrics_project_fixture(
        "project-1",
        "Draft",
        "[Verse]\nhello",
        "2026-08-24T00:00:00Z",
    );
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        &project,
        &billing,
        r#"{"required":false}"#,
        r#"{"clips":[{"id":"clip-1","title":"Draft","status":"submitted","model_name":"chirp-v4-5","created_at":"2026-08-24T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();
    let request = parsed_lyrics_project_generation_request("project-1");

    crate::commands::create::validate_lyrics_project_reference(&request, &client)
        .await
        .expect("matching project preflight");
    client
        .generate(&request)
        .await
        .expect("generation submit after project preflight");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/lyrics-projects/project-1");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/c/check");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[2].body)
            .expect("challenge request JSON"),
        serde_json::json!({"ctype": "generation"})
    );
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[3].body)
        .expect("generation request JSON");
    assert_eq!(body["prompt"], "[Verse]\nhello");
    assert_eq!(body["lyrics_project_id"], "project-1");
}

#[tokio::test]
async fn generation_with_lyrics_project_identity_mismatch_never_submits() {
    let mismatched = lyrics_project_fixture(
        "project-other",
        "Draft",
        "[Verse]\nhello",
        "2026-08-24T00:00:00Z",
    );
    let server = MockServer::json(&mismatched).await;
    let client = server.client();
    let request = parsed_lyrics_project_generation_request("project-1");

    let error = crate::commands::create::validate_lyrics_project_reference(&request, &client)
        .await
        .expect_err("mismatched project identity must stop before submit");

    assert_eq!(error.error_code(), "schema_drift");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/lyrics-projects/project-1");
}

#[tokio::test]
async fn lyrics_project_create_posts_then_reads_back_the_created_project() {
    let created = lyrics_project_fixture("project-1", "Draft", "", "2026-08-24T00:00:00Z");
    let readback = lyrics_project_fixture("project-1", "Draft", "", "2026-08-24T00:00:01Z");
    let server = MockServer::json_sequence(&[&created, &readback]).await;
    let client = server.client();

    let project = client
        .create_lyrics_project("Draft")
        .await
        .expect("create with readback");

    assert_eq!(project.updated_at.as_deref(), Some("2026-08-24T00:00:01Z"));
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/lyrics-projects");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("create body"),
        serde_json::json!({"title": "Draft"})
    );
    assert_eq!(requests[1].path, "/api/lyrics-projects/project-1");
}

#[tokio::test]
async fn lyrics_project_rename_and_flush_use_exact_bodies_and_business_readback() {
    let original = lyrics_project_fixture("project-1", "Old", "old", "2026-08-24T00:00:00Z");
    let renamed = lyrics_project_fixture("project-1", "New", "old", "2026-08-24T00:00:01Z");
    let flushed = lyrics_project_fixture("project-1", "New", "new lyrics", "2026-08-24T00:00:02Z");
    let server = MockServer::json_sequence(&[
        &original,
        &renamed,
        &renamed,
        &renamed,
        r#"{"updated_at":"2026-08-24T00:00:02Z"}"#,
        &flushed,
    ])
    .await;
    let client = server.client();

    client
        .rename_lyrics_project("project-1", "New")
        .await
        .expect("rename with readback");
    let flush = client
        .flush_lyrics_project("project-1", "new lyrics")
        .await
        .expect("flush with readback");
    assert_eq!(flush.updated_at, "2026-08-24T00:00:02Z");

    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[1].method, "PATCH");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("rename body"),
        serde_json::json!({"title": "New"})
    );
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[3].method, "GET");
    assert_eq!(requests[4].method, "POST");
    assert_eq!(requests[4].path, "/api/lyrics-projects/project-1/flush");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[4].body).expect("flush body"),
        serde_json::json!({"lyrics": "new lyrics"})
    );
    assert_eq!(requests[5].method, "GET");
}

#[tokio::test]
async fn lyrics_project_delete_accepts_204_then_requires_not_found_readback() {
    let original = lyrics_project_fixture("project-1", "Draft", "lyrics", "2026-08-24T00:00:00Z");
    let server = MockServer::response_sequence(vec![
        (200, original),
        (204, String::new()),
        (404, String::new()),
    ])
    .await;
    let client = server.client();

    client
        .delete_lyrics_project("project-1")
        .await
        .expect("delete with absence readback");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[1].method, "DELETE");
    assert_eq!(requests[1].path, "/api/lyrics-projects/project-1");
    assert_eq!(requests[1].body, "");
    assert_eq!(requests[2].method, "GET");
}

#[tokio::test]
async fn lyrics_project_create_missing_id_is_ambiguous_and_not_replayed() {
    let server = MockServer::json(r#"{"title":"Draft"}"#).await;
    let client = server.client();

    let error = client
        .create_lyrics_project("Draft")
        .await
        .expect_err("missing project ID must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("ambiguity")["recovery"]["resumable"],
        false
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn lyrics_project_write_server_error_is_ambiguous_and_not_replayed() {
    let server =
        MockServer::json_status_sequence(&[(500, r#"{"detail":"unknown accepted state"}"#)]).await;
    let client = server.client();

    let error = client
        .create_lyrics_project("Draft")
        .await
        .expect_err("5xx cannot prove the project write was rejected");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("details")["stage"],
        "response_status"
    );
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn prompt_image_workflow_preflights_generates_applies_and_reads_back() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let readback = complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        Some("https://cdn.example/generated.jpeg"),
    );
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        r#"{"image_url":"https://cdn.example/generated.jpeg","asset_id":"asset-1"}"#,
        "{}",
        &readback,
    ])
    .await;
    let client = server.client();

    let result =
        crate::workflow::visual::generate_and_apply_clip_image(&client, "clip-1", "neon rain")
            .await
            .expect("generated image with readback");

    assert_eq!(result.image_url, "https://cdn.example/generated.jpeg");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/clip/clip-1");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/gen/prompt_image/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("prompt body"),
        serde_json::json!({"prompt": "neon rain"})
    );
    assert_eq!(requests[3].path, "/api/gen/clip-1/set_metadata/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("metadata body"),
        serde_json::json!({"image_url": "https://cdn.example/generated.jpeg"})
    );
    assert_eq!(requests[4].path, "/api/clip/clip-1");
}

#[tokio::test]
async fn prompt_image_readback_converges_without_replaying_either_write() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let stale = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let converged = complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        Some("https://cdn.example/generated.jpeg"),
    );
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        r#"{"image_url":"https://cdn.example/generated.jpeg"}"#,
        "{}",
        &stale,
        &converged,
    ])
    .await;
    let client = server.client();

    let result = crate::workflow::visual::generate_and_apply_clip_image_with_readback(
        &client,
        "clip-1",
        "neon rain",
        super::PollingOptions {
            timeout: Duration::from_millis(200),
            interval: Duration::from_millis(1),
        },
    )
    .await
    .expect("stale clip image should converge through bounded GET polling");

    assert_eq!(result.image_url, "https://cdn.example/generated.jpeg");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        2,
        "readback convergence must not replay image generation or metadata"
    );
}

#[tokio::test]
async fn image_generation_uses_the_cover_action_not_download_eligibility() {
    let billing = billing_with_features(&["generate_song_image"]);
    let mut source = serde_json::from_str::<serde_json::Value>(&complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        None,
    ))
    .expect("source fixture");
    source["download_disabled_reason"] = serde_json::json!("rights_restricted");
    let source = source.to_string();
    let readback = complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        Some("https://cdn.example/generated.jpeg"),
    );
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        r#"{"image_url":"https://cdn.example/generated.jpeg"}"#,
        "{}",
        &readback,
    ])
    .await;
    let client = server.client();

    let generated =
        crate::workflow::visual::generate_and_apply_clip_image(&client, "clip-1", "neon rain")
            .await
            .expect("an enabled cover action remains authoritative for image generation");

    assert_eq!(generated.image_url, "https://cdn.example/generated.jpeg");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        2
    );
}

#[tokio::test]
async fn image_generation_requires_the_exact_authenticated_clip_owner_before_submit() {
    let billing = billing_with_features(&["generate_song_image"]);
    let mut source = serde_json::from_str::<serde_json::Value>(&complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        None,
    ))
    .expect("source fixture");
    source["user_id"] = serde_json::json!("user-other");
    let server = MockServer::json_sequence(&[&billing, &source.to_string()]).await;
    let client = server.client();

    let error =
        crate::workflow::visual::generate_and_apply_clip_image(&client, "clip-1", "neon rain")
            .await
            .expect_err("a different clip owner must stop before credit-bearing submit");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("owned by the authenticated account"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn prompt_image_unusable_2xx_is_ambiguous_without_replay() {
    let server = MockServer::json(r#"{"image_url":" "}"#).await;
    let client = server.client();

    let error = client
        .generate_prompt_image("neon rain")
        .await
        .expect_err("blank generated URL must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn prompt_image_lost_2xx_body_is_ambiguous_without_replay() {
    let server =
        MockServer::truncated_json_once(r#"{"image_url":"https://cdn.example/generated.jpeg"}"#)
            .await;
    let client = server.client();

    let error = client
        .generate_prompt_image("neon rain")
        .await
        .expect_err("lost accepted response body must be ambiguous");

    assert_eq!(error.error_code(), "ambiguous_mutation");
    let details = error.details().expect("ambiguity details");
    assert_eq!(details["stage"], "response_body");
    assert_eq!(details["recovery"]["resumable"], false);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1, "prompt image submit must not replay");
    assert_eq!(requests[0].method, "POST");
}

#[tokio::test]
async fn legacy_video_generation_fails_closed_without_a_proven_clip_eligibility_seam() {
    let billing = billing_with_features(&["generate_song_video"]);
    let source = complete_clip_fixture("clip-1", None, None);
    let server = MockServer::json_sequence(&[&billing, &source]).await;
    let client = server.client();

    let error = crate::workflow::visual::generate_video_and_wait(
        &client,
        "clip-1",
        super::PollingOptions {
            timeout: Duration::from_secs(1),
            interval: Duration::from_millis(1),
        },
    )
    .await
    .expect_err("unproven ownership/download eligibility must stop submit");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("ownership") && message.contains("eligibility") && message.contains("status"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn existing_video_status_errors_and_terminal_failures_remain_explicit() {
    let server =
        MockServer::response_sequence(vec![(400, r#"{"detail":"status unavailable"}"#.into())])
            .await;
    let client = server.client();
    let error = crate::workflow::visual::wait_for_existing_video(
        &client,
        "clip-1",
        super::PollingOptions {
            timeout: Duration::from_secs(1),
            interval: Duration::from_millis(1),
        },
    )
    .await
    .expect_err("read-only status errors must remain explicit");
    assert_ne!(error.error_code(), "ambiguous_mutation");
    assert_eq!(server.captured_all().await.len(), 1);

    let terminal = MockServer::json(r#"{"status":"failed","error_message":"render failed"}"#).await;
    let terminal_client = terminal.client();
    let error = crate::workflow::visual::wait_for_existing_video(
        &terminal_client,
        "clip-1",
        super::PollingOptions {
            timeout: Duration::from_secs(1),
            interval: Duration::from_millis(1),
        },
    )
    .await
    .expect_err("terminal failure stays explicit");
    assert!(matches!(error, CliError::GenerationFailed(_)));
}

#[tokio::test]
async fn cover_art_discovers_models_and_costs_from_current_server_contract() {
    let configs = r#"{
        "image_model_categories":[{"category":"image-v1","display_name":"Image V1","description":"still"}],
        "video_model_categories":[{"category":"video-v1","display_name":"Video V1","description":"motion","image":"supported","allowed_durations":[5,10],"allowed_durations_with_image":[5]}]
    }"#;
    let server = MockServer::json_sequence(&[
        configs,
        r#"{"cost":8,"remaining_gens":4}"#,
        r#"{"cost":20,"remaining_gens":2}"#,
    ])
    .await;
    let client = server.client();

    let discovered = client
        .cover_art_model_configs()
        .await
        .expect("dynamic model configs");
    assert_eq!(discovered.image_model_categories[0].category, "image-v1");
    assert_eq!(
        discovered.video_model_categories[0].allowed_durations,
        vec![5, 10]
    );
    let image_cost = client
        .cover_art_image_cost("image-v1")
        .await
        .expect("image cost");
    let video_cost = client
        .cover_art_video_cost("video-v1", 5)
        .await
        .expect("video cost");
    assert_eq!(image_cost.cost, 8.0);
    assert_eq!(video_cost.remaining_gens, Some(2));

    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/video_gen/model-configs");
    assert_eq!(requests[1].path, "/api/video_gen/cost/image");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("image cost body"),
        serde_json::json!({"image_gen_category":"image-v1","prompt":""})
    );
    assert_eq!(requests[2].path, "/api/video_gen/cost/video");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("video cost body"),
        serde_json::json!({"video_gen_category":"video-v1","duration":5})
    );
}

#[tokio::test]
async fn cover_art_image_and_video_submit_exact_batch_contracts_once() {
    use crate::api::types::{
        CoverArtImageGenerateRequest, CoverArtPromptImage, CoverArtVideoGenerateRequest,
    };

    let server = MockServer::json_sequence(&[
        r#"{"batch_id":"batch-image","image_ids":["image-1","image-2"]}"#,
        r#"{"batch_id":"batch-video","video_ids":["video-1","video-2"]}"#,
    ])
    .await;
    let client = server.client();

    let image = client
        .submit_cover_art_image(&CoverArtImageGenerateRequest {
            generated_text_id: None,
            prompt: "neon rain".into(),
            clip_id: "clip-1".into(),
            quantity: 2,
            image_gen_category: Some("image-v1".into()),
            prompt_images: vec![CoverArtPromptImage::uploaded("upload-1")],
            aspect_ratio: Some("1:1".into()),
        })
        .await
        .expect("image batch submit");
    assert_eq!(image.batch_id, "batch-image");
    assert_eq!(image.image_ids, vec!["image-1", "image-2"]);

    let video = client
        .submit_cover_art_video(&CoverArtVideoGenerateRequest {
            generated_text_id: None,
            prompt_start_image: Some(CoverArtPromptImage::generated("image-1")),
            clip_id: Some("clip-1".into()),
            prompt: "slow camera push".into(),
            quantity: 2,
            video_gen_category: Some("video-v1".into()),
            duration: 5,
            clip_start_time: None,
            clip_end_time: None,
            aspect_ratio: Some("1:1".into()),
        })
        .await
        .expect("video batch submit");
    assert_eq!(video.batch_id, "batch-video");
    assert_eq!(video.video_ids, vec!["video-1", "video-2"]);

    let requests = server.captured_all().await;
    assert_eq!(requests[0].path, "/api/video_gen/image/generate");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("image body"),
        serde_json::json!({
            "prompt":"neon rain",
            "clip_id":"clip-1",
            "quantity":2,
            "image_gen_category":"image-v1",
            "prompt_images":[{"id":"upload-1","type":"uploaded"}],
            "aspect_ratio":"1:1"
        })
    );
    assert_eq!(requests[1].path, "/api/video_gen/video/generate");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("video body"),
        serde_json::json!({
            "prompt_start_image":{"id":"image-1","type":"generated"},
            "clip_id":"clip-1",
            "prompt":"slow camera push",
            "quantity":2,
            "video_gen_category":"video-v1",
            "duration":5,
            "aspect_ratio":"1:1"
        })
    );
}

#[tokio::test]
async fn cover_art_recovery_poll_and_apply_use_current_batch_identities() {
    use crate::api::types::{CoverArtBatchDescriptor, CoverArtHistoryRequest};

    let server = MockServer::json_sequence(&[
        r#"{"batch_ids":[{"id":"batch-1","type":"image"}]}"#,
        r#"{"history":[{"batch_id":"batch-1","type":"image","prompt":"neon","items":[]}]}"#,
        r#"{"batches":{"batch-1":[{"id":"image-1","clip_id":"clip-1","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg","gen_category":"image-v1"}]}}"#,
        r#"{"image_url":"https://cdn.example/image-1.jpeg"}"#,
        r#"{"image_url":"https://cdn.example/frame.jpeg","video_cover_url":"https://cdn.example/video.mp4","preview_url":"https://cdn.example/preview.mp4"}"#,
    ])
    .await;
    let client = server.client();

    let pending = client
        .pending_cover_art_batches()
        .await
        .expect("pending batches");
    assert_eq!(pending.batch_ids[0].id, "batch-1");
    let history = client
        .cover_art_history(&CoverArtHistoryRequest {
            clip_id: None,
            created_at_offset: None,
            favorites_only: false,
            media_type: Some("image".into()),
            limit: 20,
        })
        .await
        .expect("history");
    assert_eq!(history.history[0].batch_id, "batch-1");
    let polled = client
        .poll_cover_art_batches(&[CoverArtBatchDescriptor::new("batch-1", "image")])
        .await
        .expect("poll batch");
    assert_eq!(polled.batches["batch-1"][0].status, "complete");
    client
        .apply_generated_cover_image("clip-1", "image-1", "session-1")
        .await
        .expect("apply generated image");
    client
        .apply_generated_cover_video("clip-1", "video-upload-1", "session-2")
        .await
        .expect("apply generated video");

    let requests = server.captured_all().await;
    assert_eq!(requests[0].path, "/api/video_gen/pending_batches");
    assert_eq!(requests[0].body, "{}");
    assert_eq!(requests[1].path, "/api/video_gen/history");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("history body"),
        serde_json::json!({
            "clip_id":null,
            "created_at_offset":null,
            "favorites_only":false,
            "media_type":"image",
            "limit":20
        })
    );
    assert_eq!(requests[2].path, "/api/video_gen/poll_batches");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("poll body"),
        serde_json::json!({"batch_ids":[{"id":"batch-1","type":"image"}]})
    );
    assert_eq!(requests[3].path, "/api/gen/clip-1/set_metadata/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("image apply body"),
        serde_json::json!({
            "cover_image":{"id":"image-1","type":"generated"},
            "cover_art_session_id":"session-1"
        })
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[4].body).expect("video apply body"),
        serde_json::json!({
            "video_cover_upload_id":"video-upload-1",
            "cover_art_session_id":"session-2"
        })
    );
}

#[tokio::test]
async fn cover_art_image_workflow_preflights_costs_and_preserves_batch_ids_without_auto_apply() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let configs = r#"{
        "image_model_categories":[{"category":"image-v1","display_name":"Image V1"}],
        "video_model_categories":[]
    }"#;
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        configs,
        r#"{"cost":8,"remaining_gens":4}"#,
        r#"{"batch_id":"batch-image","image_ids":["image-1","image-2"]}"#,
    ])
    .await;
    let client = server.client();

    let result = crate::workflow::visual::generate_cover_art_image_batch(
        &client,
        "clip-1",
        "neon rain",
        Some("Image V1"),
        None,
        None,
    )
    .await
    .expect("safe no-wait image submit");

    assert_eq!(result.model_category, "image-v1");
    assert_eq!(result.submission.batch_id, "batch-image");
    assert!(result.batch.is_none());
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].path, "/api/clip/clip-1");
    assert_eq!(requests[2].path, "/api/video_gen/model-configs");
    assert_eq!(requests[3].path, "/api/video_gen/cost/image");
    assert_eq!(requests[4].path, "/api/video_gen/image/generate");
    assert!(
        requests
            .iter()
            .all(|request| !request.body.contains("cover_image"))
    );
}

#[tokio::test]
async fn cover_art_video_workflow_uses_dynamic_duration_and_bounded_batch_polling() {
    let billing = billing_with_features(&["generate_song_video"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let configs = r#"{
        "image_model_categories":[],
        "video_model_categories":[{"category":"video-v1","allowed_durations":[5,10],"allowed_durations_with_image":[5]}]
    }"#;
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        configs,
        r#"{"cost":20,"remaining_gens":2}"#,
        r#"{"batch_id":"batch-video","video_ids":["video-1","video-2"]}"#,
        r#"{"batches":{"batch-video":[{"id":"video-1","type":"video","status":"processing"},{"id":"video-2","type":"video","status":"processing"}]}}"#,
        r#"{"batches":{"batch-video":[{"id":"video-1","type":"video","status":"complete","url":"https://cdn.example/video-1.mp4","video_upload_id":"upload-1"},{"id":"video-2","type":"video","status":"complete","url":"https://cdn.example/video-2.mp4","video_upload_id":"upload-2"}]}}"#,
    ])
    .await;
    let client = server.client();

    let result = crate::workflow::visual::generate_cover_art_video_batch(
        &client,
        "clip-1",
        "slow camera push",
        Some("video-v1"),
        Some(10),
        None,
        Some(super::PollingOptions {
            timeout: Duration::from_millis(200),
            interval: Duration::from_millis(1),
        }),
    )
    .await
    .expect("video batch converges");

    assert_eq!(result.submission.batch_id, "batch-video");
    assert!(result.batch.is_some());
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 7);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("cost body"),
        serde_json::json!({"video_gen_category":"video-v1","duration":10})
    );
    assert_eq!(requests[5].path, "/api/video_gen/poll_batches");
    assert_eq!(requests[6].path, "/api/video_gen/poll_batches");
}

#[tokio::test]
async fn cover_art_submit_waits_for_every_returned_media_id_not_a_partial_poll_page() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let configs =
        r#"{"image_model_categories":[{"category":"image-v1"}],"video_model_categories":[]}"#;
    let server = MockServer::response_sequence_with_idle_timeout(
        vec![
            (200, billing),
            (200, source),
            (200, configs.into()),
            (200, r#"{"cost":8,"remaining_gens":4}"#.into()),
            (200, r#"{"batch_id":"batch-image","image_ids":["image-1","image-2"]}"#.into()),
            (200, r#"{"batches":{"batch-image":[{"id":"image-1","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg"}]}}"#.into()),
            (200, r#"{"batches":{"batch-image":[{"id":"image-1","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg"},{"id":"image-2","type":"image","status":"complete","url":"https://cdn.example/image-2.jpeg"}]}}"#.into()),
        ],
        Duration::from_millis(100),
    )
    .await;
    let client = server.client();

    let result = crate::workflow::visual::generate_cover_art_image_batch(
        &client,
        "clip-1",
        "neon rain",
        None,
        None,
        Some(super::PollingOptions {
            timeout: Duration::from_millis(300),
            interval: Duration::from_millis(1),
        }),
    )
    .await
    .expect("all returned image IDs must complete");

    assert_eq!(
        result.batch.expect("terminal batch").batches["batch-image"].len(),
        2
    );
    assert_eq!(server.captured_all().await.len(), 7);
}

#[tokio::test]
async fn cover_art_recovery_waits_for_both_fixed_quantity_results() {
    let server = MockServer::response_sequence(vec![
        (200, r#"{"batches":{"batch-image":[{"id":"image-1","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg"}]}}"#.into()),
        (200, r#"{"batches":{"batch-image":[{"id":"image-1","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg"},{"id":"image-2","type":"image","status":"complete","url":"https://cdn.example/image-2.jpeg"}]}}"#.into()),
    ])
    .await;
    let client = server.client();

    let result = crate::workflow::visual::wait_for_cover_art_batch(
        &client,
        &crate::api::types::CoverArtBatchDescriptor::new("batch-image", "image"),
        super::PollingOptions {
            timeout: Duration::from_millis(200),
            interval: Duration::from_millis(1),
        },
    )
    .await
    .expect("recovery must wait for both fixed-quantity results");

    assert_eq!(result.batches["batch-image"].len(), 2);
    assert_eq!(server.captured_all().await.len(), 2);
}

#[tokio::test]
async fn cover_art_apply_workflow_requires_owner_and_exact_clip_readback() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let readback = complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        Some("https://cdn.example/image-1.jpeg"),
    );
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        r#"{"batches":{"batch-image":[{"id":"image-1","clip_id":"clip-1","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg"}]}}"#,
        r#"{"image_url":"https://cdn.example/image-1.jpeg"}"#,
        &readback,
    ])
    .await;
    let client = server.client();

    let applied =
        crate::workflow::visual::apply_cover_art_image(&client, "clip-1", "batch-image", "image-1")
            .await
            .expect("applied image readback");

    assert_eq!(
        applied.clip.image_url,
        Some("https://cdn.example/image-1.jpeg".into())
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[2].path, "/api/video_gen/poll_batches");
    assert_eq!(requests[3].path, "/api/gen/clip-1/set_metadata/");
    assert_eq!(requests[4].path, "/api/clip/clip-1");
}

#[tokio::test]
async fn cover_art_apply_rejects_a_completed_result_from_another_clip() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let foreign_batch = r#"{"batches":{"batch-image":[{"id":"image-1","clip_id":"clip-2","type":"image","status":"complete","url":"https://cdn.example/image-1.jpeg"}]}}"#;
    let server = MockServer::json_sequence(&[&billing, &source, foreign_batch]).await;
    let client = server.client();

    let error =
        crate::workflow::visual::apply_cover_art_image(&client, "clip-1", "batch-image", "image-1")
            .await
            .expect_err("a result from another clip must never be applied");

    assert!(matches!(error, CliError::Config(message) if message.contains("does not prove")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2].path, "/api/video_gen/poll_batches");
}

#[tokio::test]
async fn cover_art_video_apply_proves_upload_provenance_and_reads_flattened_cover_url() {
    let billing = billing_with_features(&["generate_song_video"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let batch = r#"{"batches":{"batch-video":[{"id":"video-1","clip_id":"clip-1","type":"video","status":"complete","url":"https://cdn.example/video-1.mp4","video_upload_id":"upload-1"}]}}"#;
    let mut readback: serde_json::Value = serde_json::from_str(&complete_clip_fixture(
        "clip-1",
        Some("generate_cover_art"),
        None,
    ))
    .expect("clip fixture");
    readback["video_cover_url"] = serde_json::json!("https://cdn.example/video-1.mp4");
    let readback = serde_json::to_string(&readback).expect("clip JSON");
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        batch,
        r#"{"image_url":"https://cdn.example/fallback.jpeg","video_cover_url":"https://cdn.example/video-1.mp4"}"#,
        &readback,
    ])
    .await;
    let client = server.client();

    let applied = crate::workflow::visual::apply_cover_art_video(
        &client,
        "clip-1",
        "batch-video",
        "upload-1",
    )
    .await
    .expect("video apply readback");

    assert_eq!(applied.media_id, "upload-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[2].path, "/api/video_gen/poll_batches");
    assert_eq!(requests[3].path, "/api/gen/clip-1/set_metadata/");
}

#[tokio::test]
async fn cover_art_video_rejects_an_image_for_a_model_that_does_not_support_it() {
    let billing = billing_with_features(&["generate_song_video"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let configs = r#"{
        "image_model_categories":[],
        "video_model_categories":[{"category":"video-v1","image":"not_supported","allowed_durations":[5],"allowed_durations_with_image":[]}]
    }"#;
    let server = MockServer::json_sequence(&[&billing, &source, configs]).await;
    let client = server.client();

    let error = crate::workflow::visual::generate_cover_art_video_batch(
        &client,
        "clip-1",
        "",
        Some("video-v1"),
        None,
        Some(crate::api::types::CoverArtPromptImage::generated("image-1")),
        None,
    )
    .await
    .expect_err("unsupported image-to-video model must stop before cost or submit");

    assert!(
        matches!(error, CliError::Config(message) if message.contains("does not support a start image"))
    );
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.method == "GET"));
}

#[tokio::test]
async fn cover_art_cost_with_no_remaining_generations_stops_before_submit() {
    let billing = billing_with_features(&["generate_song_image"]);
    let source = complete_clip_fixture("clip-1", Some("generate_cover_art"), None);
    let configs = r#"{
        "image_model_categories":[{"category":"image-v1"}],
        "video_model_categories":[]
    }"#;
    let server = MockServer::json_sequence(&[
        &billing,
        &source,
        configs,
        r#"{"cost":8,"remaining_gens":0}"#,
    ])
    .await;
    let client = server.client();

    let error = crate::workflow::visual::generate_cover_art_image_batch(
        &client,
        "clip-1",
        "neon rain",
        None,
        None,
        None,
    )
    .await
    .expect_err("zero remaining generations must stop submit");

    assert!(matches!(error, CliError::Config(message) if message.contains("no remaining image")));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[3].path, "/api/video_gen/cost/image");
}

#[tokio::test]
async fn cover_art_credit_and_moderation_gates_remain_structured_and_non_ambiguous() {
    let creation_limit = MockServer::json_status_sequence(&[(
        402,
        r#"{"error_type":"creation_limit_reached","detail":"daily creation limit reached"}"#,
    )])
    .await;
    let error = creation_limit
        .client()
        .cover_art_image_cost("image-v1")
        .await
        .expect_err("creation limit must remain explicit");
    assert_eq!(error.error_code(), "creation_limit_reached");
    assert_ne!(error.error_code(), "ambiguous_mutation");
    assert_eq!(creation_limit.captured_all().await.len(), 1);

    let insufficient = MockServer::json_status_sequence(&[(
        402,
        r#"{"error_type":"insufficient_credits","detail":"not enough credits"}"#,
    )])
    .await;
    let request = crate::api::types::CoverArtImageGenerateRequest {
        generated_text_id: None,
        prompt: "neon rain".into(),
        clip_id: "clip-1".into(),
        quantity: 2,
        image_gen_category: Some("image-v1".into()),
        prompt_images: Vec::new(),
        aspect_ratio: Some("1:1".into()),
    };
    let error = insufficient
        .client()
        .submit_cover_art_image(&request)
        .await
        .expect_err("insufficient credits must remain explicit");
    assert_eq!(error.error_code(), "insufficient_credits");
    assert_ne!(error.error_code(), "ambiguous_mutation");
    assert_eq!(insufficient.captured_all().await.len(), 1);

    let moderation = MockServer::json_status_sequence(&[(
        400,
        r#"{"error_type":"moderation_error","detail":"prompt rejected","reasons":["policy"]}"#,
    )])
    .await;
    let error = moderation
        .client()
        .submit_cover_art_image(&request)
        .await
        .expect_err("moderation must remain explicit");
    assert_eq!(error.error_code(), "moderation_error");
    assert_eq!(moderation.captured_all().await.len(), 1);
}

#[tokio::test]
async fn cover_art_submit_and_apply_treat_server_errors_as_ambiguous_without_replay() {
    let request = crate::api::types::CoverArtImageGenerateRequest {
        generated_text_id: None,
        prompt: "neon rain".into(),
        clip_id: "clip-1".into(),
        quantity: 2,
        image_gen_category: Some("image-v1".into()),
        prompt_images: Vec::new(),
        aspect_ratio: Some("1:1".into()),
    };
    let submit = MockServer::json_status_sequence(&[(
        500,
        r#"{"error_type":"server_error","detail":"upstream lost response"}"#,
    )])
    .await;
    let error = submit
        .client()
        .submit_cover_art_image(&request)
        .await
        .expect_err("server error cannot prove the paid submit was rejected");
    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("submit ambiguity")["recovery"]["resumable"],
        false
    );
    assert_eq!(submit.captured_all().await.len(), 1);

    let apply = MockServer::json_status_sequence(&[(
        500,
        r#"{"error_type":"server_error","detail":"metadata outcome unknown"}"#,
    )])
    .await;
    let error = apply
        .client()
        .apply_generated_cover_image("clip-1", "image-1", "session-1")
        .await
        .expect_err("server error cannot prove metadata was not applied");
    assert_eq!(error.error_code(), "ambiguous_mutation");
    assert_eq!(
        error.details().expect("apply ambiguity")["recovery"]["resumable"],
        false
    );
    assert_eq!(apply.captured_all().await.len(), 1);
}

#[tokio::test]
async fn new_mutation_submit_does_not_retry_an_explicit_401() {
    let server = MockServer::response_sequence_with_idle_timeout(
        vec![(401, String::new())],
        Duration::from_millis(50),
    )
    .await;
    let client = server.client();

    let error = client
        .generate_prompt_image("neon rain")
        .await
        .expect_err("explicit auth rejection must be returned");

    assert!(matches!(error, CliError::AuthExpired));
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 1, "mutation submit must not auth-retry");
    assert_eq!(requests[0].method, "POST");
}

#[tokio::test]
async fn lyrics_rewrite_posts_the_current_selection_body_once() {
    let server = MockServer::json(
        r#"{"generated_lyrics":"new chorus","lyrics_request_id":"request-1","lyrics_id":"lyrics-1","variant":"current"}"#,
    )
    .await;
    let client = server.client();

    let result = client
        .rewrite_lyrics(super::lyrics_editor::LyricsRewriteOptions {
            prompt: "make it brighter",
            prefix: "verse\n",
            edit: "old chorus",
            suffix: "\noutro",
            title: "Draft",
            create_session_token: "session-1",
        })
        .await
        .expect("lyrics rewrite");

    assert_eq!(result.generated_lyrics, "new chorus");
    assert_eq!(result.lyrics_request_id.as_deref(), Some("request-1"));
    assert_eq!(result.lyrics_id.as_deref(), Some("lyrics-1"));
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/generate/lyrics-infill/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("rewrite request JSON"),
        serde_json::json!({
            "prompt": "make it brighter",
            "context_lyrics_prefix": "verse\n",
            "context_lyrics_edit": "old chorus",
            "context_lyrics_suffix": "\noutro",
            "create_session_token": "session-1",
            "title": "Draft"
        })
    );
}

#[tokio::test]
async fn lyrics_rewrite_has_a_fixed_thirty_second_bound_and_never_auth_replays() {
    assert_eq!(
        super::lyrics_editor::LYRICS_REWRITE_TIMEOUT,
        Duration::from_secs(30)
    );

    let server = MockServer::response_sequence_with_idle_timeout(
        vec![
            (401, String::new()),
            (200, r#"{"generated_lyrics":"duplicate"}"#.into()),
        ],
        Duration::from_millis(50),
    )
    .await;
    let client = server.client();
    let error = client
        .rewrite_lyrics(super::lyrics_editor::LyricsRewriteOptions {
            prompt: "rewrite",
            prefix: "",
            edit: "old",
            suffix: "",
            title: "",
            create_session_token: "session-1",
        })
        .await
        .expect_err("mutation must return explicit auth rejection");
    assert!(matches!(error, CliError::AuthExpired));
    assert_eq!(server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn lyrics_editor_submits_treat_server_errors_as_ambiguous_without_replay() {
    let rewrite_server =
        MockServer::json_status_sequence(&[(500, r#"{"detail":"unknown accepted state"}"#)]).await;
    let rewrite_error = rewrite_server
        .client()
        .rewrite_lyrics(super::lyrics_editor::LyricsRewriteOptions {
            prompt: "rewrite",
            prefix: "",
            edit: "old",
            suffix: "",
            title: "",
            create_session_token: "session-1",
        })
        .await
        .expect_err("rewrite 5xx is ambiguous");
    assert_eq!(rewrite_error.error_code(), "ambiguous_mutation");
    assert_eq!(rewrite_server.captured_all().await.len(), 1);

    let mashup_server =
        MockServer::json_status_sequence(&[(500, r#"{"detail":"unknown accepted state"}"#)]).await;
    let mashup_error = mashup_server
        .client()
        .start_lyrics_mashup(super::lyrics_editor::LyricsMashupOptions {
            lyrics_a: "first",
            lyrics_b: "second",
            create_session_token: "session-2",
        })
        .await
        .expect_err("mashup 5xx is ambiguous");
    assert_eq!(mashup_error.error_code(), "ambiguous_mutation");
    assert_eq!(mashup_server.captured_all().await.len(), 1);
}

#[tokio::test]
async fn lyrics_mashup_posts_exact_sources_then_polls_the_returned_id() {
    let server = MockServer::json_sequence(&[
        r#"{"lyrics_request_id":"request-2","mashup_id":"mashup-2"}"#,
        r#"{"status":"pending"}"#,
        r#"{"status":"complete","text":"combined lyrics","title":"A x B (Mashup)","id":"mashup-2"}"#,
    ])
    .await;
    let client = server.client();

    let submission = client
        .start_lyrics_mashup(super::lyrics_editor::LyricsMashupOptions {
            lyrics_a: "first lyrics",
            lyrics_b: "second lyrics",
            create_session_token: "session-2",
        })
        .await
        .expect("mashup submit");
    assert_eq!(submission.mashup_id, "mashup-2");
    let result = client
        .wait_for_submitted_lyrics_mashup(
            &submission.mashup_id,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("mashup completes");
    assert_eq!(result.status, "complete");
    assert_eq!(result.text.as_deref(), Some("combined lyrics"));

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/generate/lyrics-mashup");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("mashup request JSON"),
        serde_json::json!({
            "lyrics_a": "first lyrics",
            "lyrics_b": "second lyrics",
            "create_session_token": "session-2",
            "source": "create_ui"
        })
    );
    assert_eq!(requests[1].path, "/api/generate/lyrics/mashup-2");
    assert_eq!(requests[2].path, "/api/generate/lyrics/mashup-2");
}

#[tokio::test]
async fn lyrics_mashup_poll_failure_preserves_the_known_id_without_replaying_submit() {
    let server = MockServer::response_sequence(vec![
        (
            200,
            r#"{"lyrics_request_id":"request-3","mashup_id":"mashup-3"}"#.into(),
        ),
        (500, r#"{"detail":"poll unavailable"}"#.into()),
    ])
    .await;
    let client = server.client();
    let submission = client
        .start_lyrics_mashup(super::lyrics_editor::LyricsMashupOptions {
            lyrics_a: "first lyrics",
            lyrics_b: "second lyrics",
            create_session_token: "session-3",
        })
        .await
        .expect("mashup submit");
    let error = client
        .wait_for_submitted_lyrics_mashup(
            &submission.mashup_id,
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("poll failure must retain the accepted submit handle");

    assert_eq!(error.error_code(), "partial_mutation");
    let details = error.details().expect("partial mutation details");
    assert_eq!(details["mashup_id"], "mashup-3");
    assert_eq!(details["recovery"]["resumable"], true);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2, "submit must not be replayed");
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[1].method, "GET");
}

#[tokio::test]
async fn read_only_lyrics_mashup_wait_keeps_poll_errors_explicit() {
    let server =
        MockServer::response_sequence(vec![(500, r#"{"detail":"poll unavailable"}"#.into())]).await;
    let client = server.client();

    let error = client
        .wait_for_existing_lyrics_mashup(
            "mashup-existing",
            super::PollingOptions {
                timeout: Duration::from_secs(1),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("a read-only poll failure is not a partial mutation");

    assert_eq!(error.error_code(), "api_error");
    assert_eq!(server.captured_all().await.len(), 1);
}

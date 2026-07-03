use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::{Duration, timeout};

use super::SunoClient;
use super::extend::ExtendClipOptions;
use super::types::{
    Clip, ClipReaction, CreateAudioUploadRequest, CreateAudioUploadSpec, CreateImageUploadRequest,
    CreatePersonaRequest, EditPersonaRequest, FeedFilters, FinishAudioUploadRequest,
    GenerateRequest, InitializeAudioClipRequest, PersonaListScope, PlaylistReaction,
    SetMetadataRequest,
};
use crate::auth::{AuthState, BrowserEnvironment};
use crate::core::CliError;

struct CapturedRequest {
    method: String,
    path: String,
    headers: String,
    body: String,
}

struct MockServer {
    base_url: String,
    requests: oneshot::Receiver<Vec<CapturedRequest>>,
}

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
        "models": [],
        "period": "month",
        "renews_on": null,
        "remaster_model_types": []
    })
    .to_string()
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
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("mock server address");
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut captured = Vec::with_capacity(responses.len());
            for (status, response_body) in responses {
                let (stream, _) = listener.accept().await.expect("accept request");
                captured.push(capture_request_with_status(stream, status, &response_body).await);
            }
            let _ = tx.send(captured);
        });

        Self {
            base_url: format!("http://{addr}"),
            requests: rx,
        }
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
        }
    }

    fn client(&self) -> SunoClient {
        self.client_with_auth(AuthState {
            jwt: Some("test-jwt".into()),
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
        self.requests.await.expect("captured requests")
    }
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

async fn capture_request_with_status_inner(
    stream: &mut TcpStream,
    status: u16,
    response_body: &str,
) -> CapturedRequest {
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

    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

#[tokio::test]
async fn delete_clips_posts_current_web_trash_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .delete_clips(&["clip-a".to_string(), "clip-b".to_string()])
        .await
        .expect("delete clips");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "trash": true,
            "clip_ids": ["clip-a", "clip-b"]
        })
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
async fn restore_clips_posts_current_web_trash_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .restore_clips(&["clip-a".to_string()])
        .await
        .expect("restore clips");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/gen/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "trash": false,
            "clip_ids": ["clip-a"]
        })
    );
}

#[tokio::test]
async fn get_clips_batches_feed_ids_by_pairs_contract() {
    let clip_a = r#"{"id":"clip-a","title":"A","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}"#;
    let clip_b = r#"{"id":"clip-b","title":"B","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}"#;
    let clip_c = r#"{"id":"clip-c","title":"C","status":"complete","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}"#;
    let first_response = format!("[{clip_a},{clip_b}]");
    let second_response = format!("[{clip_c}]");
    let server =
        MockServer::json_sequence(&[first_response.as_str(), second_response.as_str()]).await;
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
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/feed/?ids=clip-a,clip-b");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/feed/?ids=clip-c");
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
async fn clip_info_fetches_song_page_supplemental_contract() {
    let server = MockServer::json_sequence(&[
        r#"{"source_clips":[{"clip_id":"source-1","title":"Source Song","image_url":"https://cdn2.suno.ai/image_source-1.jpeg","audio_url":"https://cdn1.suno.ai/source-1.mp3","is_deleted":true,"relationship":"COV","user":{"user_id":"user-1","user_display_name":"Source User","user_handle":"source"}}]}"#,
        r#"{"results":[{"id":"comment-1","clip_id":"clip-a","content":"Nice","num_likes":2}],"allow_comment":true,"total_count":1}"#,
        r#"{"count":3}"#,
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
            audio_url: None,
            video_url: None,
            image_url: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
        })
        .await
        .expect("clip info");

    assert_eq!(info.clip.id, "clip-a");
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
    assert_eq!(info.direct_children_count, 3);
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
    assert_eq!(
        requests[2].path,
        "/api/clips/direct_children_count?clip_id=clip-a"
    );
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
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
        })
        .await
        .expect("clip info should keep base clip when supplemental reads fail");

    assert_eq!(info.clip.id, "clip-a");
    assert_eq!(
        info.clip.audio_url.as_deref(),
        Some("https://cdn1.suno.ai/clip-a.mp3")
    );
    assert!(info.attribution.source_clips.is_empty());
    assert_eq!(info.comments.total_count, 0);
    assert_eq!(info.direct_children_count, 0);
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
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
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
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
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
        serde_json::json!({ "is_public": false })
    );
}

#[tokio::test]
async fn generate_posts_current_web_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        billing.as_str(),
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();

    let mut generate = GenerateRequest::new("chirp-v4-5", "custom");
    generate.set_challenge_token(Some("captcha-token".into()));
    generate.title = Some("Demo".into());
    generate.tags = Some("pop, upbeat".into());
    generate.gpt_description_prompt = Some("first line\nsecond line".into());
    generate.metadata.lyrics_model = Some("default".into());

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
    assert_eq!(body["prompt"], "");
    assert_eq!(body["gpt_description_prompt"], "first line\nsecond line");
    assert_eq!(body["token_provider"], 1);
    assert_eq!(body["metadata"]["create_mode"], "custom");
    assert_eq!(body["metadata"]["lyrics_model"], "default");
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
async fn generate_preserves_existing_user_tier_without_billing_lookup() {
    let server = MockServer::json(
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-v4-5","created_at":"2026-06-30T00:00:00Z"}]}"#,
    )
    .await;
    let client = server.client();
    let mut generate = GenerateRequest::new("chirp-v4-5", "custom");
    generate.set_challenge_token(Some("captcha-token".into()));
    generate.metadata.user_tier = "existing-tier".into();

    let clips = client.generate(&generate).await.expect("generate");

    assert_eq!(clips[0].id, "clip-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&request.body).expect("request json");
    assert_eq!(body["metadata"]["user_tier"], "existing-tier");
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
        .upsample_tags("garage pop", false)
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
            "is_instrumental": false
        })
    );
}

#[tokio::test]
async fn generate_without_token_preflights_then_submits_when_challenge_is_not_required() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"required":false}"#,
        billing.as_str(),
        r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
    ])
    .await;
    let client = server.client();
    let generate = GenerateRequest::new("chirp-fenix", "custom");

    let clips = client.generate(&generate).await.expect("generate");

    assert_eq!(clips[0].id, "clip-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/c/check");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json"),
        serde_json::json!({ "ctype": "generation" })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
}

#[tokio::test]
async fn generate_falls_back_when_billing_info_is_unavailable() {
    let server = MockServer::json_status_sequence(&[
        (200, r#"{"required":false}"#),
        (500, r#"{"detail":"billing unavailable"}"#),
        (
            200,
            r#"{"clips":[{"id":"clip-1","title":"Demo","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]}"#,
        ),
    ])
    .await;
    let client = server.client();
    let generate = GenerateRequest::new("chirp-fenix", "custom");

    let clips = client.generate(&generate).await.expect("generate");

    assert_eq!(clips[0].id, "clip-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].path, "/api/c/check");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["metadata"]["user_tier"], "");
}

#[tokio::test]
async fn generate_without_token_stops_when_challenge_is_required() {
    let server = MockServer::json(r#"{"required":true,"captcha_version":1}"#).await;
    let client = server.client();
    let generate = GenerateRequest::new("chirp-fenix", "custom");

    let err = client
        .generate(&generate)
        .await
        .expect_err("challenge error");

    assert!(err.to_string().contains("generation challenge"));
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/c/check");
}

#[tokio::test]
async fn cover_posts_generate_v2_cover_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"{"required":false}"#,
        billing.as_str(),
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
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/c/check");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/billing/info/");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[2].body).expect("request json");
    assert_eq!(body["mv"], "chirp-fenix");
    assert_eq!(body["tags"], "pop");
    assert_eq!(body["cover_clip_id"], "clip-a");
    assert_eq!(body["metadata"]["create_mode"], "cover");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
}

#[tokio::test]
async fn cover_with_challenge_token_posts_generate_without_preflight_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
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
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/billing/info/");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("request json");
    assert_eq!(body["cover_clip_id"], "clip-a");
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert_eq!(body["token"], "captcha-token");
    assert_eq!(body["token_provider"], 1);
}

#[tokio::test]
async fn remaster_posts_generate_v2_remaster_contract() {
    let server = MockServer::json(
        r#"{"clips":[{"id":"remaster-1","title":"Remaster","status":"submitted","model_name":"chirp-flounder","created_at":"2026-06-30T00:00:00Z"}]}"#,
    )
    .await;
    let client = server.client();

    let clips = client
        .remaster("clip-a", "chirp-flounder")
        .await
        .expect("remaster");

    assert_eq!(clips[0].id, "remaster-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/generate/upsample");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "clip_id": "clip-a",
            "model_name": "chirp-flounder",
            "variation_category": "normal"
        })
    );
}

#[tokio::test]
async fn concat_posts_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"concat-1","title":"Concat","status":"submitted","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}"#,
    )
    .await;
    let client = server.client();

    let clip = client.concat("clip-a").await.expect("concat");

    assert_eq!(clip.id, "concat-1");
    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/generate/concat/v2/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "clip_id": "clip-a" })
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
async fn stems_posts_current_web_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"[{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]"#,
        r#"{"required":false}"#,
        billing.as_str(),
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
    assert_eq!(requests[0].path, "/api/feed/?ids=clip-a");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/c/check");
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/billing/info/");
    assert_eq!(requests[3].method, "POST");
    assert_eq!(requests[3].path, "/api/generate/v2-web/");
    let body = serde_json::from_str::<serde_json::Value>(&requests[3].body).expect("request json");
    assert_eq!(body["token"], serde_json::Value::Null);
    assert_eq!(body["token_provider"], serde_json::Value::Null);
    assert_eq!(body["task"], "gen_stem");
    assert_eq!(body["mv"], "chirp-v3-0");
    assert_eq!(body["title"], "Source Song");
    assert_eq!(body["prompt"], "");
    assert_eq!(body["make_instrumental"], true);
    assert_eq!(body["continue_clip_id"], "clip-a");
    assert_eq!(body["stem_type_id"], 91);
    assert_eq!(body["stem_type_group_name"], "Twelve");
    assert_eq!(body["stem_task"], "twelve");
    assert_eq!(body["metadata"]["create_mode"], "custom");
    assert_eq!(body["metadata"]["is_remix"], true);
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
    assert!(
        !body["metadata"]
            .as_object()
            .expect("metadata object")
            .contains_key("is_max_mode")
    );
    assert!(
        !body["metadata"]
            .as_object()
            .expect("metadata object")
            .contains_key("is_mumble")
    );
}

#[tokio::test]
async fn stems_with_challenge_token_posts_generate_without_preflight_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"[{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z"}]"#,
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
    assert_eq!(requests[0].path, "/api/feed/?ids=clip-a");
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
async fn extend_fetches_source_clip_and_posts_string_title_contract() {
    let billing = billing_info_response("tier-pro");
    let server = MockServer::json_sequence(&[
        r#"[{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"prompt":"[Verse]\nOriginal words"}}]"#,
        r#"{"clips":[{"id":"clip-a","title":"Source Song","status":"complete","model_name":"chirp-fenix","created_at":"2026-06-30T00:00:00Z","metadata":{"tags":"source chamber folk","negative_tags":"vocals, narration","prompt":"[Verse]\nOriginal words","make_instrumental":true}}]}"#,
        r#"{"required":false}"#,
        billing.as_str(),
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
        })
        .await
        .expect("extend");

    assert_eq!(clips[0].id, "extend-1");
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/api/feed/?ids=clip-a");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/feed/v3");
    let feed_body =
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("feed request json");
    assert_eq!(feed_body["filters"]["searchText"], "Source Song");
    assert_eq!(requests[2].method, "POST");
    assert_eq!(requests[2].path, "/api/c/check");
    assert_eq!(requests[3].method, "GET");
    assert_eq!(requests[3].path, "/api/billing/info/");
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
    assert_eq!(body["metadata"]["lyrics_updated"], true);
    assert_eq!(body["metadata"]["user_tier"], "tier-pro");
}

#[tokio::test]
async fn lyrics_generation_posts_and_polls_current_web_contract() {
    let server = MockServer::json_sequence(&[
        r#"{"id":"lyrics-job-1"}"#,
        r#"{"text":"[Verse]\nHello","title":"Demo","status":"complete","tags":["pop"]}"#,
    ])
    .await;
    let client = server.client();

    let result = client
        .generate_lyrics("write a pop hook")
        .await
        .expect("lyrics");

    assert_eq!(result.status, "complete");
    assert_eq!(result.tags, vec!["pop"]);
    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/generate/lyrics/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("request json"),
        serde_json::json!({ "prompt": "write a pop hook" })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/generate/lyrics/lyrics-job-1");
    assert_eq!(requests[1].body, "");
}

#[tokio::test]
async fn aligned_lyrics_gets_current_web_contract() {
    let server = MockServer::json(
        r#"{"aligned_words":[{"word":"Hello","start_s":0.0,"end_s":0.5,"success":true,"p_align":0.99}]}"#,
    )
    .await;
    let client = server.client();

    let words = client
        .aligned_lyrics("clip-a")
        .await
        .expect("aligned lyrics");

    assert_eq!(words[0].word, "Hello");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/gen/clip-a/aligned_lyrics/v2/");
    assert_eq!(request.body, "");
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
        r#"{"playlists":[{"id":"playlist-1","name":"Road Trip"}],"numTotalResults":1,"currentPage":2}"#,
    )
    .await;
    let client = server.client();

    let response = client.list_playlists(2).await.expect("list playlists");

    assert_eq!(response.current_page, 2);
    assert_eq!(response.playlists[0].id, "playlist-1");
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/playlist/me?page=2");
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn playlist_detail_reads_v2_cover_metadata_contract() {
    let server = MockServer::json(
        r#"{"id":"playlist-1","metadata":{"name":"Road Trip","description":"Drive set","cover_url":"https://cdn2.suno.ai/image_upload-1.jpeg","cover_image_s3_id":"image_upload-1","cover_is_user_set":true,"is_public":true}}"#,
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
    assert!(playlist.is_public);
}

#[tokio::test]
async fn create_playlist_with_metadata_uses_create_set_metadata_then_detail_contract() {
    let server = MockServer::json_sequence(&[
        r#"{"id":"playlist-1","name":"Road Trip"}"#,
        "{}",
        r#"{"playlist":{"id":"playlist-1","name":"Road Trip","description":"Drive set","image_url":"https://cdn.example/cover.jpg"}}"#,
    ])
    .await;
    let client = server.client();

    let playlist = client
        .create_playlist(
            "Road Trip",
            Some("Drive set"),
            Some("https://cdn.example/cover.jpg"),
        )
        .await
        .expect("create playlist");

    assert_eq!(playlist.id, "playlist-1");
    assert_eq!(playlist.description.as_deref(), Some("Drive set"));

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/playlist/create/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("create json"),
        serde_json::json!({ "name": "Road Trip" })
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/api/playlist/set_metadata");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[1].body).expect("metadata json"),
        serde_json::json!({
            "playlist_id": "playlist-1",
            "description": "Drive set",
            "image_url": "https://cdn.example/cover.jpg"
        })
    );
    assert_eq!(requests[2].method, "GET");
    assert_eq!(requests[2].path, "/api/playlist/v2/playlist-1");
    assert_eq!(requests[2].body, "");
}

#[tokio::test]
async fn set_playlist_uploaded_cover_patches_v2_metadata_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"playlist-1","metadata":{"name":"Road Trip","cover_url":"https://cdn2.suno.ai/image_upload-1.jpeg","cover_image_s3_id":"image_upload-1","cover_is_user_set":true}}"#,
    ])
    .await;
    let client = server.client();

    let playlist = client
        .set_playlist_uploaded_cover("playlist-1", "upload-1")
        .await
        .expect("set cover");

    assert_eq!(
        playlist.image_url.as_deref(),
        Some("https://cdn2.suno.ai/image_upload-1.jpeg")
    );
    assert_eq!(
        playlist.cover_image_s3_id.as_deref(),
        Some("image_upload-1")
    );
    let requests = server.captured_all().await;
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(requests[0].path, "/api/playlist/v2/playlist-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("cover json"),
        serde_json::json!({
            "metadata": {
                "cover_url": "https://cdn2.suno.ai/image_upload-1.jpeg",
                "cover_image_s3_id": "image_upload-1",
                "cover_is_user_set": true
            }
        })
    );
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/api/playlist/v2/playlist-1");
}

#[tokio::test]
async fn set_playlist_metadata_with_suno_image_url_patches_v2_cover_contract() {
    let server = MockServer::json_sequence(&[
        "{}",
        r#"{"id":"playlist-1","metadata":{"name":"Road Trip","cover_url":"https://cdn2.suno.ai/image_upload-1.jpeg","cover_image_s3_id":"image_upload-1","cover_is_user_set":true}}"#,
    ])
    .await;
    let client = server.client();

    client
        .set_playlist_metadata(
            "playlist-1",
            None,
            None,
            Some("https://cdn2.suno.ai/image_upload-1.jpeg"),
        )
        .await
        .expect("set cover");

    let requests = server.captured_all().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(requests[0].path, "/api/playlist/v2/playlist-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&requests[0].body).expect("cover json"),
        serde_json::json!({
            "metadata": {
                "cover_url": "https://cdn2.suno.ai/image_upload-1.jpeg",
                "cover_image_s3_id": "image_upload-1",
                "cover_is_user_set": true
            }
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
    assert_eq!(report.failed[0].error_code, "api_error");
    assert!(report.failed[0].message.contains("HTTP 500"));
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

    match error {
        CliError::Api { code, message } => {
            assert_eq!(code, "api_error");
            assert!(message.contains("HTTP 500"));
        }
        other => panic!("unexpected error: {other:?}"),
    }

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
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .set_playlist_visibility("playlist-1", false)
        .await
        .expect("set visibility");

    let request = server.captured().await;
    assert_eq!(request.method, "PATCH");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "metadata": { "is_public": false } })
    );
}

#[tokio::test]
async fn trash_playlist_posts_undo_false_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .trash_playlist("playlist-1")
        .await
        .expect("trash playlist");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "undo": false })
    );
}

#[tokio::test]
async fn restore_playlist_posts_undo_true_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .restore_playlist("playlist-1")
        .await
        .expect("restore playlist");

    let request = server.captured().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/playlist/v2/playlist-1/trash");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({ "undo": true })
    );
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
async fn get_processed_clip_uses_current_web_contract() {
    let server = MockServer::json(
        r#"{"id":"processed-1","status":"complete","vocal_start_s":0.0,"vocal_end_s":19.92,"vocal_audio_url":"https://cdn1.suno.ai/processed_vocals.m4a"}"#,
    )
    .await;
    let client = server.client();

    let processed = client
        .get_processed_clip("processed-1")
        .await
        .expect("get processed clip");

    assert_eq!(processed.status, "complete");
    assert_eq!(
        processed.vocal_audio_url.as_deref(),
        Some("https://cdn1.suno.ai/processed_vocals.m4a")
    );
    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/processed_clip/processed-1");
    assert_eq!(request.body, "");
}

#[tokio::test]
async fn trash_personas_puts_current_web_bulk_trash_contract() {
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
    assert_eq!(request.path, "/api/persona/bulk-trash-personas/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "persona_ids": ["persona-1"],
            "undo": false,
            "hide": false
        })
    );
}

#[tokio::test]
async fn restore_personas_puts_current_web_bulk_restore_contract() {
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
    assert_eq!(request.path, "/api/persona/bulk-trash-personas/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "persona_ids": ["persona-1"],
            "undo": true,
            "hide": false
        })
    );
}

#[tokio::test]
async fn purge_personas_puts_current_web_bulk_delete_contract() {
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
    assert_eq!(request.path, "/api/persona/bulk-trash-personas/");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&request.body).expect("request json"),
        serde_json::json!({
            "persona_ids": ["persona-1"],
            "undo": false,
            "hide": true
        })
    );
}

#[tokio::test]
async fn list_personas_uses_scope_page_and_continuation_query() {
    let server = MockServer::json(r#"{"personas":[],"total_results":0,"current_page":2}"#).await;
    let client = server.client();

    client
        .list_personas(PersonaListScope::Loved, 2, Some("next-token"))
        .await
        .expect("list personas");

    let request = server.captured().await;
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path,
        "/api/persona/get-loved-personas/?page=2&continuation_token=next-token"
    );
    assert_eq!(request.body, "");
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
            "spec": {
                "extension": "mp3",
                "is_stem_mix": false,
                "upload_type": "file_upload"
            }
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
            "upload_filename": "demo.mp3"
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
async fn upload_presigned_audio_form_posts_s3_multipart_contract() {
    let server = MockServer::json("{}").await;
    let client = server.client();

    client
        .upload_presigned_audio_form(
            &format!("{}/s3-upload", server.base_url),
            &[
                ("key".into(), "audio/upload-1".into()),
                ("policy".into(), "p".into()),
            ]
            .into_iter()
            .collect(),
            "demo.mp3",
            b"audio-bytes".to_vec(),
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

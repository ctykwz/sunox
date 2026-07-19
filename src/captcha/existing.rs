use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::api::challenge::ChallengeProvider;
use crate::commands::browser_extension;
use crate::core::CliError;

const PORT_START: u16 = 29_764;
const PORT_COUNT: u16 = 8;
const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1_800);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(35);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_BYTES: usize = 24 * 1024;
const PROTOCOL_VERSION: u8 = 1;
const CLAIM_PENDING: u8 = 0;
const CLAIMED: u8 = 1;
const CLAIM_CLOSED: u8 = 2;

#[derive(Debug)]
enum BridgeResult {
    Token(String),
    Error(String),
}

struct BridgeState {
    request_id: String,
    provider: ChallengeProvider,
    secret: String,
    claim_state: AtomicU8,
    claimed_notify: Notify,
    result_sender: Mutex<Option<oneshot::Sender<BridgeResult>>>,
}

#[derive(Deserialize)]
struct ClaimRequest {
    version: u8,
    client_id: String,
    page_url: String,
}

#[derive(Serialize)]
struct ClaimResponse<'a> {
    version: u8,
    request_id: &'a str,
    provider: &'static str,
}

#[derive(Deserialize)]
struct ResultRequest {
    version: u8,
    request_id: String,
    token: Option<String>,
    error: Option<String>,
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: Option<&'static str>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, reason: &'static str, value: impl Serialize) -> Result<Self, CliError> {
        Ok(Self {
            status,
            reason,
            content_type: Some("application/json"),
            body: serde_json::to_vec(&value)?,
        })
    }

    fn empty(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            content_type: None,
            body: Vec::new(),
        }
    }
}

pub(super) async fn try_solve(provider: ChallengeProvider) -> Result<Option<String>, CliError> {
    let Some(secret) = browser_extension::bridge_secret()? else {
        return Ok(None);
    };

    let (listener, port) = bind_bridge_listener().await?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let (result_sender, result_receiver) = oneshot::channel();
    let state = Arc::new(BridgeState {
        request_id,
        provider,
        secret,
        claim_state: AtomicU8::new(CLAIM_PENDING),
        claimed_notify: Notify::new(),
        result_sender: Mutex::new(Some(result_sender)),
    });
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(serve(listener, Arc::clone(&state), cancellation.clone()));

    let claimed = wait_for_claim(&state).await;
    if !claimed {
        cancellation.cancel();
        let _ = server.await;
        return Ok(None);
    }

    eprintln!(
        "Using the connected Suno browser tab for silent challenge verification (bridge port {port})..."
    );
    let result = timeout(COMPLETION_TIMEOUT, result_receiver).await;
    cancellation.cancel();
    let _ = server.await;

    match result {
        Ok(Ok(BridgeResult::Token(token))) => Ok(Some(token)),
        Ok(Ok(BridgeResult::Error(error))) => Err(CliError::Config(format!(
            "existing browser challenge failed: {error}"
        ))),
        Ok(Err(_)) => Err(CliError::Config(
            "existing browser challenge bridge closed before returning a result".into(),
        )),
        Err(_) => Err(CliError::Config(
            "existing browser challenge timed out after 35 seconds".into(),
        )),
    }
}

async fn bind_bridge_listener() -> Result<(TcpListener, u16), CliError> {
    let mut last_error = None;
    for port in PORT_START..PORT_START + PORT_COUNT {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok((listener, port)),
            Err(error) => last_error = Some(error),
        }
    }
    Err(CliError::Config(format!(
        "could not bind the browser bridge on ports {PORT_START}-{}: {}",
        PORT_START + PORT_COUNT - 1,
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no port available".into())
    )))
}

async fn wait_for_claim(state: &BridgeState) -> bool {
    let notified = state.claimed_notify.notified();
    if state.claim_state.load(Ordering::Acquire) == CLAIMED {
        return true;
    }
    let _ = timeout(DISCOVERY_TIMEOUT, notified).await;
    match state.claim_state.compare_exchange(
        CLAIM_PENDING,
        CLAIM_CLOSED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => false,
        Err(CLAIMED) => true,
        Err(_) => false,
    }
}

async fn serve(listener: TcpListener, state: Arc<BridgeState>, cancellation: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    let _ = timeout(CONNECTION_TIMEOUT, handle_connection(stream, state)).await;
                });
            }
        }
    }
}

async fn handle_connection(mut stream: TcpStream, state: Arc<BridgeState>) -> Result<(), CliError> {
    let request = read_request(&mut stream).await?;
    let origin = request.headers.get("origin").cloned().unwrap_or_default();
    let response = route_request(&request, &state)?;
    write_response(
        &mut stream,
        response,
        valid_extension_origin(&origin).then_some(origin.as_str()),
    )
    .await
}

fn route_request(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    let origin = request
        .headers
        .get("origin")
        .map(String::as_str)
        .unwrap_or("");
    if request.method == "OPTIONS" {
        return Ok(if valid_extension_origin(origin) {
            HttpResponse::empty(204, "No Content")
        } else {
            HttpResponse::empty(403, "Forbidden")
        });
    }

    if request.method != "POST"
        || !valid_extension_origin(origin)
        || request.headers.get("x-sunox-extension").map(String::as_str) != Some("1")
        || !authorized(request, &state.secret)
    {
        return Ok(HttpResponse::empty(403, "Forbidden"));
    }

    match request.path.as_str() {
        "/v1/challenge/claim" => claim(request, state),
        "/v1/challenge/result" => receive_result(request, state),
        _ => Ok(HttpResponse::empty(404, "Not Found")),
    }
}

fn claim(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    let claim: ClaimRequest = match serde_json::from_slice(&request.body) {
        Ok(claim) => claim,
        Err(_) => return Ok(HttpResponse::empty(400, "Bad Request")),
    };
    if claim.version != PROTOCOL_VERSION
        || claim.client_id.is_empty()
        || claim.client_id.len() > 128
        || !is_suno_page(&claim.page_url)
    {
        return Ok(HttpResponse::empty(422, "Unprocessable Content"));
    }
    if state
        .claim_state
        .compare_exchange(CLAIM_PENDING, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(HttpResponse::empty(409, "Conflict"));
    }
    state.claimed_notify.notify_waiters();
    HttpResponse::json(
        200,
        "OK",
        ClaimResponse {
            version: PROTOCOL_VERSION,
            request_id: &state.request_id,
            provider: match state.provider {
                ChallengeProvider::HCaptcha => "hcaptcha",
                ChallengeProvider::Turnstile => "turnstile",
            },
        },
    )
}

fn receive_result(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    let result: ResultRequest = match serde_json::from_slice(&request.body) {
        Ok(result) => result,
        Err(_) => return Ok(HttpResponse::empty(400, "Bad Request")),
    };
    if result.version != PROTOCOL_VERSION || result.request_id != state.request_id {
        return Ok(HttpResponse::empty(409, "Conflict"));
    }
    let bridge_result = match (result.token, result.error) {
        (Some(token), None) if (20..=16_384).contains(&token.len()) => BridgeResult::Token(token),
        (None, Some(error)) if !error.is_empty() && error.len() <= 1_000 => {
            BridgeResult::Error(error)
        }
        _ => return Ok(HttpResponse::empty(422, "Unprocessable Content")),
    };
    let Some(sender) = state
        .result_sender
        .lock()
        .expect("bridge result mutex poisoned")
        .take()
    else {
        return Ok(HttpResponse::empty(409, "Conflict"));
    };
    let _ = sender.send(bridge_result);
    Ok(HttpResponse::empty(204, "No Content"))
}

fn authorized(request: &HttpRequest, secret: &str) -> bool {
    let Some(value) = request.headers.get("authorization") else {
        return false;
    };
    let Some(candidate) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(candidate.as_bytes(), secret.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn valid_extension_origin(origin: &str) -> bool {
    let Some(id) = origin.strip_prefix("chrome-extension://") else {
        return false;
    };
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

fn is_suno_page(page_url: &str) -> bool {
    reqwest::Url::parse(page_url).is_ok_and(|url| {
        url.scheme() == "https" && url.host_str() == Some("suno.com") && url.username().is_empty()
    })
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, CliError> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4_096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(CliError::Config(
                "browser bridge received a truncated request".into(),
            ));
        }
        data.extend_from_slice(&buffer[..read]);
        if data.len() > MAX_REQUEST_BYTES {
            return Err(CliError::Config(
                "browser bridge request exceeded 24 KiB".into(),
            ));
        }
        if let Some(position) = find_bytes(&data, b"\r\n\r\n") {
            break position + 4;
        }
    };

    let header_text = std::str::from_utf8(&data[..header_end - 4])
        .map_err(|_| CliError::Config("browser bridge received non-UTF-8 headers".into()))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| CliError::Config("browser bridge request line was missing".into()))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let path = request_parts.next().unwrap_or_default().to_string();
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(CliError::Config("browser bridge requires HTTP/1.1".into()));
    }
    let mut headers = HashMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(CliError::Config(
                "browser bridge received a malformed header".into(),
            ));
        };
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| CliError::Config("browser bridge content-length was invalid".into()))?
        .unwrap_or(0);
    if header_end + content_length > MAX_REQUEST_BYTES {
        return Err(CliError::Config(
            "browser bridge request exceeded 24 KiB".into(),
        ));
    }
    while data.len() < header_end + content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Err(CliError::Config("browser bridge body was truncated".into()));
        }
        data.extend_from_slice(&buffer[..read]);
        if data.len() > MAX_REQUEST_BYTES {
            return Err(CliError::Config(
                "browser bridge request exceeded 24 KiB".into(),
            ));
        }
    }

    Ok(HttpRequest {
        method,
        path,
        headers,
        body: data[header_end..header_end + content_length].to_vec(),
    })
}

async fn write_response(
    stream: &mut TcpStream,
    response: HttpResponse,
    extension_origin: Option<&str>,
) -> Result<(), CliError> {
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        response.status,
        response.reason,
        response.body.len()
    );
    if let Some(content_type) = response.content_type {
        headers.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    if let Some(origin) = extension_origin {
        headers.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-Sunox-Extension\r\nAccess-Control-Allow-Private-Network: true\r\nVary: Origin\r\n"
        ));
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await?;
    Ok(())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{Notify, oneshot};
    use tokio_util::sync::CancellationToken;

    use super::{
        BridgeResult, BridgeState, CLAIM_CLOSED, CLAIM_PENDING, HttpRequest, constant_time_eq,
        is_suno_page, route_request, serve, valid_extension_origin,
    };
    use crate::api::challenge::ChallengeProvider;

    fn state(secret: &str) -> (BridgeState, oneshot::Receiver<BridgeResult>) {
        let (sender, receiver) = oneshot::channel();
        (
            BridgeState {
                request_id: "request-a".into(),
                provider: ChallengeProvider::HCaptcha,
                secret: secret.into(),
                claim_state: CLAIM_PENDING.into(),
                claimed_notify: Notify::new(),
                result_sender: Mutex::new(Some(sender)),
            },
            receiver,
        )
    }

    fn request(path: &str, secret: &str, body: serde_json::Value) -> HttpRequest {
        HttpRequest {
            method: "POST".into(),
            path: path.into(),
            headers: HashMap::from([
                (
                    "origin".into(),
                    "chrome-extension://abcdefghijklmnopabcdefghijklmnop".into(),
                ),
                ("x-sunox-extension".into(), "1".into()),
                ("authorization".into(), format!("Bearer {secret}")),
            ]),
            body: serde_json::to_vec(&body).expect("serialize body"),
        }
    }

    use std::collections::HashMap;

    #[test]
    fn only_chrome_extension_origins_are_trusted() {
        assert!(valid_extension_origin(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop"
        ));
        assert!(!valid_extension_origin(
            "chrome-extension://abcdefghijklmnop"
        ));
        assert!(!valid_extension_origin("https://suno.com"));
        assert!(!valid_extension_origin("chrome-extension://ABC"));
        assert!(!valid_extension_origin("chrome-extension://abc/extra"));
    }

    #[test]
    fn suno_claim_requires_the_exact_https_origin() {
        assert!(is_suno_page("https://suno.com/create"));
        assert!(!is_suno_page("http://suno.com/create"));
        assert!(!is_suno_page("https://evil.suno.com/create"));
        assert!(!is_suno_page("https://suno.com.evil.example/create"));
    }

    #[test]
    fn bearer_secret_comparison_is_exact() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn first_valid_suno_tab_claims_the_challenge() {
        let (state, _receiver) = state("secret-value");
        let claim = request(
            "/v1/challenge/claim",
            "secret-value",
            serde_json::json!({
                "version": 1,
                "client_id": "client-a",
                "page_url": "https://suno.com/create"
            }),
        );

        let first = route_request(&claim, &state).expect("first response");
        let second = route_request(&claim, &state).expect("second response");

        assert_eq!(first.status, 200);
        assert_eq!(second.status, 409);
    }

    #[test]
    fn a_claim_cannot_start_after_discovery_has_closed() {
        let (state, _receiver) = state("secret-value");
        state
            .claim_state
            .store(CLAIM_CLOSED, std::sync::atomic::Ordering::Release);
        let claim = request(
            "/v1/challenge/claim",
            "secret-value",
            serde_json::json!({
                "version": 1,
                "client_id": "client-a",
                "page_url": "https://suno.com/create"
            }),
        );

        assert_eq!(route_request(&claim, &state).expect("response").status, 409);
    }

    #[tokio::test]
    async fn matching_result_returns_the_one_time_token() {
        let (state, receiver) = state("secret-value");
        let result = request(
            "/v1/challenge/result",
            "secret-value",
            serde_json::json!({
                "version": 1,
                "request_id": "request-a",
                "token": "abcdefghijklmnopqrstuvwxyz",
                "error": null
            }),
        );

        let response = route_request(&result, &state).expect("result response");
        let BridgeResult::Token(token) = receiver.await.expect("bridge result") else {
            panic!("expected token");
        };

        assert_eq!(response.status, 204);
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn invalid_origin_or_secret_cannot_claim() {
        let (state, _receiver) = state("secret-value");
        let mut bad_origin = request(
            "/v1/challenge/claim",
            "secret-value",
            serde_json::json!({
                "version": 1,
                "client_id": "client-a",
                "page_url": "https://suno.com/create"
            }),
        );
        bad_origin
            .headers
            .insert("origin".into(), "https://evil.example".into());
        let bad_secret = request(
            "/v1/challenge/claim",
            "wrong-secret",
            serde_json::json!({
                "version": 1,
                "client_id": "client-a",
                "page_url": "https://suno.com/create"
            }),
        );

        assert_eq!(
            route_request(&bad_origin, &state).expect("response").status,
            403
        );
        assert_eq!(
            route_request(&bad_secret, &state).expect("response").status,
            403
        );
    }

    #[tokio::test]
    async fn loopback_server_accepts_an_authenticated_extension_round_trip() {
        let (state, receiver) = state("secret-value");
        let state = std::sync::Arc::new(state);
        let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("listener: {error}"),
        };
        let address = listener.local_addr().expect("listener address");
        let cancellation = CancellationToken::new();
        let server = tokio::spawn(serve(
            listener,
            std::sync::Arc::clone(&state),
            cancellation.clone(),
        ));

        let claim_body = serde_json::json!({
            "version": 1,
            "client_id": "client-a",
            "page_url": "https://suno.com/create"
        })
        .to_string();
        let claim_response =
            raw_request(address, "/v1/challenge/claim", "secret-value", &claim_body).await;
        assert!(claim_response.starts_with("HTTP/1.1 200 OK"));
        assert!(claim_response.contains(
            "Access-Control-Allow-Origin: chrome-extension://abcdefghijklmnopabcdefghijklmnop"
        ));
        assert!(claim_response.contains("\"provider\":\"hcaptcha\""));

        let result_body = serde_json::json!({
            "version": 1,
            "request_id": "request-a",
            "token": "abcdefghijklmnopqrstuvwxyz",
            "error": null
        })
        .to_string();
        let result_response = raw_request(
            address,
            "/v1/challenge/result",
            "secret-value",
            &result_body,
        )
        .await;
        assert!(result_response.starts_with("HTTP/1.1 204 No Content"));
        let BridgeResult::Token(token) = receiver.await.expect("bridge result") else {
            panic!("expected token");
        };
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");

        cancellation.cancel();
        server.await.expect("server task");
    }

    async fn raw_request(
        address: std::net::SocketAddr,
        path: &str,
        secret: &str,
        body: &str,
    ) -> String {
        let mut stream = TcpStream::connect(address).await.expect("connect");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: chrome-extension://abcdefghijklmnopabcdefghijklmnop\r\nX-Sunox-Extension: 1\r\nAuthorization: Bearer {secret}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.expect("write");
        stream.shutdown().await.expect("shutdown write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        String::from_utf8(response).expect("UTF-8 response")
    }
}

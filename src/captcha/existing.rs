use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{StreamExt, future::join_all};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2_10::Sha256;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

use crate::api::challenge::ChallengeProvider;
use crate::captcha::bridge_contract::{
    BROWSER_BRIDGE_RUNTIME_BUILD, LOOPBACK_PORT_COUNT as PORT_COUNT,
    LOOPBACK_PORT_START as PORT_START, PROTOCOL_VERSION,
};
use crate::captcha::{BridgeProbe, BridgeProbeStatus};
use crate::commands::browser_extension;
use crate::core::CliError;

const ACTIVE_TAB_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const BACKGROUND_TAB_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(27);
const BRIDGE_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const COMPLETION_TIMEOUT_MS: u64 = 130_000;
const COMPLETION_TIMEOUT: Duration = Duration::from_millis(COMPLETION_TIMEOUT_MS);
const RESULT_REPLAY_GRACE: Duration = Duration::from_millis(1_500);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);
const RESULT_WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const OCCUPIED_PORT_PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_OCCUPIED_PORT_HELLO_RESPONSE_BYTES: usize = 4 * 1024;
const MAX_REQUEST_BYTES: usize = 24 * 1024;
const CLAIM_PENDING: u8 = 0;
const CLAIMED: u8 = 1;
const CLAIM_CLOSED: u8 = 2;

#[derive(Debug)]
enum BridgeResult {
    Token(String),
    Error(String),
}

#[derive(Clone, Copy)]
enum BridgeOperation {
    Challenge(ChallengeProvider),
    Probe,
}

enum BridgeListenerBinding {
    Bound {
        listener: TcpListener,
        port: u16,
        occupied_ports: Vec<u16>,
    },
    Exhausted {
        occupied_ports: Vec<u16>,
    },
}

struct BridgeState {
    port: u16,
    request_id: String,
    server_nonce: String,
    operation: BridgeOperation,
    secret: String,
    claim_state: AtomicU8,
    claimed_notify: Notify,
    probe_acknowledged: AtomicBool,
    probe_acknowledged_notify: Notify,
    result_delivery: Arc<ResultDeliveryState>,
    claim_session: Mutex<Option<ClaimSession>>,
}

struct ClaimSession {
    client_nonce: String,
    server_nonce: String,
}

#[derive(Deserialize)]
struct HelloRequest {
    version: u8,
    client_nonce: String,
}

#[derive(Serialize)]
struct HelloResponse<'a> {
    version: u8,
    server_nonce: &'a str,
    proof: String,
}

#[derive(Deserialize)]
struct OwnedHelloResponse {
    version: u8,
    server_nonce: String,
    proof: String,
}

#[derive(Deserialize)]
struct ClaimRequest {
    version: u8,
    runtime_build: String,
    client_id: String,
    page_url: String,
    client_nonce: String,
    server_nonce: String,
    proof: String,
}

#[derive(Serialize)]
struct ClaimResponse<'a> {
    version: u8,
    request_id: &'a str,
    provider: &'static str,
}

#[derive(Serialize)]
struct ProbeClaimResponse<'a> {
    version: u8,
    request_id: &'a str,
    probe: bool,
}

#[derive(Deserialize)]
struct ProbeAckRequest {
    version: u8,
    runtime_build: String,
    request_id: String,
    client_nonce: String,
    server_nonce: String,
    proof: String,
}

#[derive(Deserialize)]
struct ResultRequest {
    version: u8,
    request_id: String,
    client_nonce: String,
    server_nonce: String,
    token: Option<String>,
    error: Option<String>,
    proof: String,
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
    result_delivery: Option<ResultDelivery>,
}

enum ResultSlot {
    Pending {
        sender: oneshot::Sender<BridgeResult>,
        fingerprint: Option<String>,
    },
    Writing {
        fingerprint: String,
    },
    Committed {
        fingerprint: String,
    },
    Closed,
}

struct ResultDeliveryState {
    slot: Mutex<ResultSlot>,
    changed: Notify,
}

struct ResultDelivery {
    state: Arc<ResultDeliveryState>,
    fingerprint: String,
    sender: Option<oneshot::Sender<BridgeResult>>,
    result: Option<BridgeResult>,
}

impl ResultDelivery {
    fn acknowledge(mut self) {
        let Some(sender) = self.sender.take() else {
            return;
        };
        let Some(result) = self.result.take() else {
            return;
        };
        let mut state = self
            .state
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = std::mem::replace(&mut *state, ResultSlot::Closed);
        match current {
            ResultSlot::Writing { fingerprint } if fingerprint == self.fingerprint => {
                *state = if sender.send(result).is_ok() {
                    ResultSlot::Committed { fingerprint }
                } else {
                    ResultSlot::Closed
                };
            }
            other => {
                debug_assert!(
                    false,
                    "a Browser Bridge result delivery must exclusively own the writing state"
                );
                *state = other;
            }
        }
        drop(state);
        self.state.changed.notify_one();
    }
}

impl Drop for ResultDelivery {
    fn drop(&mut self) {
        let Some(sender) = self.sender.take() else {
            return;
        };
        let mut state = self
            .state
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = std::mem::replace(&mut *state, ResultSlot::Closed);
        match current {
            ResultSlot::Writing { fingerprint } if fingerprint == self.fingerprint => {
                *state = ResultSlot::Pending {
                    sender,
                    fingerprint: Some(fingerprint),
                };
            }
            other => {
                debug_assert!(
                    false,
                    "an abandoned Browser Bridge result must still own the writing state"
                );
                *state = other;
            }
        }
        drop(state);
        self.state.changed.notify_one();
    }
}

impl HttpResponse {
    fn json(status: u16, reason: &'static str, value: impl Serialize) -> Result<Self, CliError> {
        Ok(Self {
            status,
            reason,
            content_type: Some("application/json"),
            body: serde_json::to_vec(&value)?,
            result_delivery: None,
        })
    }

    fn empty(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            content_type: None,
            body: Vec::new(),
            result_delivery: None,
        }
    }

    fn with_result_delivery(
        mut self,
        state: Arc<ResultDeliveryState>,
        fingerprint: String,
        sender: oneshot::Sender<BridgeResult>,
        result: BridgeResult,
    ) -> Self {
        self.result_delivery = Some(ResultDelivery {
            state,
            fingerprint,
            sender: Some(sender),
            result: Some(result),
        });
        self
    }

    fn acknowledge_result_delivery(&mut self) {
        if let Some(delivery) = self.result_delivery.take() {
            delivery.acknowledge();
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
        port,
        request_id,
        server_nonce: uuid::Uuid::new_v4().to_string(),
        operation: BridgeOperation::Challenge(provider),
        secret,
        claim_state: AtomicU8::new(CLAIM_PENDING),
        claimed_notify: Notify::new(),
        probe_acknowledged: AtomicBool::new(false),
        probe_acknowledged_notify: Notify::new(),
        result_delivery: Arc::new(ResultDeliveryState {
            slot: Mutex::new(ResultSlot::Pending {
                sender: result_sender,
                fingerprint: None,
            }),
            changed: Notify::new(),
        }),
        claim_session: Mutex::new(None),
    });
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(serve(listener, Arc::clone(&state), cancellation.clone()));

    let claimed = wait_for_claim(&state).await;
    if !claimed {
        cancellation.cancel();
        let _ = server.await;
        return Ok(None);
    }
    acknowledge_loaded_runtime(&state.secret);

    eprintln!(
        "Using the Browser Bridge managed Chrome context for silent challenge verification (bridge port {port})..."
    );
    let mut result_receiver = result_receiver;
    let result = match timeout(COMPLETION_TIMEOUT, &mut result_receiver).await {
        Ok(result) => Some(result),
        Err(_) => finish_or_close_timed_out_result(&state, &mut result_receiver).await,
    };
    if matches!(&result, Some(Ok(_))) {
        // Keep the authenticated listener alive long enough for the extension
        // to replay an identical terminal result if the flushed 204 response
        // was lost at the fetch boundary.
        sleep(RESULT_REPLAY_GRACE).await;
    }
    cancellation.cancel();
    let _ = server.await;

    match result {
        Some(Ok(BridgeResult::Token(token))) => Ok(Some(token)),
        Some(Ok(BridgeResult::Error(error))) => Err(CliError::Config(format!(
            "Browser Bridge challenge failed: {error}"
        ))),
        Some(Err(_)) => Err(CliError::Config(
            "Browser Bridge closed before returning a challenge result".into(),
        )),
        None => Err(CliError::Config(format!(
            "Browser Bridge challenge timed out after {} seconds",
            COMPLETION_TIMEOUT.as_secs()
        ))),
    }
}

async fn finish_or_close_timed_out_result(
    state: &BridgeState,
    receiver: &mut oneshot::Receiver<BridgeResult>,
) -> Option<Result<BridgeResult, oneshot::error::RecvError>> {
    loop {
        let changed = state.result_delivery.changed.notified();
        let wait_for_writer = {
            let mut slot = state
                .result_delivery
                .slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match &*slot {
                ResultSlot::Writing { .. } | ResultSlot::Committed { .. } => true,
                ResultSlot::Pending { .. } => {
                    *slot = ResultSlot::Closed;
                    false
                }
                ResultSlot::Closed => false,
            }
        };
        if !wait_for_writer {
            return None;
        }
        tokio::select! {
            result = &mut *receiver => return Some(result),
            _ = changed => {}
        }
    }
}

pub(crate) async fn probe() -> Result<BridgeProbe, CliError> {
    let started = Instant::now();
    let Some(secret) = browser_extension::bridge_secret()? else {
        return Ok(BridgeProbe {
            status: missing_secret_probe_status(browser_extension::bridge_is_configured()?),
            port: None,
            occupied_ports: Vec::new(),
            bridge_occupied_ports: Vec::new(),
            foreign_occupied_ports: Vec::new(),
            latency: started.elapsed(),
        });
    };
    let acknowledgement_secret = secret.clone();
    let probe = probe_with_secret(secret, BRIDGE_PROBE_TIMEOUT, started).await?;
    if probe.status == BridgeProbeStatus::Responsive {
        acknowledge_loaded_runtime(&acknowledgement_secret);
    }
    Ok(probe)
}

fn missing_secret_probe_status(installation_exists: bool) -> BridgeProbeStatus {
    if installation_exists {
        BridgeProbeStatus::Unavailable
    } else {
        BridgeProbeStatus::NotConfigured
    }
}

async fn probe_with_secret(
    secret: String,
    discovery_timeout: Duration,
    started: Instant,
) -> Result<BridgeProbe, CliError> {
    probe_with_secret_in_range(secret, discovery_timeout, started, PORT_START, PORT_COUNT).await
}

async fn probe_with_secret_in_range(
    secret: String,
    discovery_timeout: Duration,
    started: Instant,
    port_start: u16,
    port_count: u16,
) -> Result<BridgeProbe, CliError> {
    let binding = discover_bridge_listener_in_range(port_start, port_count).await?;
    let occupied_ports = match &binding {
        BridgeListenerBinding::Bound { occupied_ports, .. }
        | BridgeListenerBinding::Exhausted { occupied_ports } => occupied_ports.clone(),
    };
    let (bridge_occupied_ports, foreign_occupied_ports) =
        classify_occupied_ports(&occupied_ports, &secret).await?;
    let (listener, port) = match binding {
        BridgeListenerBinding::Bound { listener, port, .. } => (listener, port),
        BridgeListenerBinding::Exhausted { .. } => {
            return Ok(BridgeProbe {
                status: occupied_probe_status(&bridge_occupied_ports, &foreign_occupied_ports),
                port: None,
                occupied_ports,
                bridge_occupied_ports,
                foreign_occupied_ports,
                latency: started.elapsed(),
            });
        }
    };
    let state = Arc::new(BridgeState {
        port,
        request_id: uuid::Uuid::new_v4().to_string(),
        server_nonce: uuid::Uuid::new_v4().to_string(),
        operation: BridgeOperation::Probe,
        secret,
        claim_state: AtomicU8::new(CLAIM_PENDING),
        claimed_notify: Notify::new(),
        probe_acknowledged: AtomicBool::new(false),
        probe_acknowledged_notify: Notify::new(),
        result_delivery: Arc::new(ResultDeliveryState {
            slot: Mutex::new(ResultSlot::Closed),
            changed: Notify::new(),
        }),
        claim_session: Mutex::new(None),
    });
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(serve(listener, Arc::clone(&state), cancellation.clone()));

    let responsive = wait_for_probe_ack_signal(&state, discovery_timeout).await;
    if !responsive {
        let _ = close_discovery(&state);
    }
    cancellation.cancel();
    let _ = server.await;

    Ok(BridgeProbe {
        status: if responsive {
            BridgeProbeStatus::Responsive
        } else {
            occupied_probe_status(&bridge_occupied_ports, &foreign_occupied_ports)
        },
        port: Some(port),
        occupied_ports,
        bridge_occupied_ports,
        foreign_occupied_ports,
        latency: started.elapsed(),
    })
}

pub(super) fn is_configured() -> Result<bool, CliError> {
    browser_extension::bridge_is_configured()
}

fn acknowledge_loaded_runtime(authenticated_secret: &str) {
    if let Err(error) = browser_extension::acknowledge_runtime_build(
        BROWSER_BRIDGE_RUNTIME_BUILD,
        authenticated_secret,
    ) {
        eprintln!(
            "Warning: authenticated Browser Bridge runtime {BROWSER_BRIDGE_RUNTIME_BUILD} could not clear its reload-pending marker: {error}"
        );
    }
}

async fn bind_bridge_listener() -> Result<(TcpListener, u16), CliError> {
    match discover_bridge_listener().await? {
        BridgeListenerBinding::Bound { listener, port, .. } => Ok((listener, port)),
        BridgeListenerBinding::Exhausted { occupied_ports } => Err(CliError::Config(format!(
            "could not bind the browser bridge because all ports {}-{} are occupied ({})",
            PORT_START,
            PORT_START + PORT_COUNT - 1,
            occupied_ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

async fn discover_bridge_listener() -> Result<BridgeListenerBinding, CliError> {
    discover_bridge_listener_in_range(PORT_START, PORT_COUNT).await
}

async fn discover_bridge_listener_in_range(
    port_start: u16,
    port_count: u16,
) -> Result<BridgeListenerBinding, CliError> {
    let port_end = port_start.checked_add(port_count).ok_or_else(|| {
        CliError::Config(format!(
            "invalid browser bridge port range: start {port_start}, count {port_count}"
        ))
    })?;
    let mut last_error = None;
    let mut occupied_ports = Vec::new();
    for port in port_start..port_end {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => {
                return Ok(BridgeListenerBinding::Bound {
                    listener,
                    port,
                    occupied_ports,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                occupied_ports.push(port);
                last_error = Some(error);
            }
            Err(error) => last_error = Some(error),
        }
    }
    if occupied_ports.len() == usize::from(port_count) {
        return Ok(BridgeListenerBinding::Exhausted { occupied_ports });
    }
    Err(CliError::Config(format!(
        "could not bind the browser bridge on ports {port_start}-{}: {}",
        port_end.saturating_sub(1),
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no port available".into())
    )))
}

fn occupied_probe_status(
    bridge_occupied_ports: &[u16],
    foreign_occupied_ports: &[u16],
) -> BridgeProbeStatus {
    if !bridge_occupied_ports.is_empty() {
        BridgeProbeStatus::Busy
    } else if !foreign_occupied_ports.is_empty() {
        BridgeProbeStatus::PortConflict
    } else {
        BridgeProbeStatus::Unavailable
    }
}

async fn classify_occupied_ports(
    occupied_ports: &[u16],
    secret: &str,
) -> Result<(Vec<u16>, Vec<u16>), CliError> {
    if occupied_ports.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let client = occupied_port_probe_client()?;
    let checks = occupied_ports
        .iter()
        .copied()
        .map(|port| occupied_port_is_current_bridge(&client, port, secret));
    let mut bridge = Vec::new();
    let mut foreign = Vec::new();
    for (port, authenticated) in occupied_ports.iter().copied().zip(join_all(checks).await) {
        if authenticated {
            bridge.push(port);
        } else {
            foreign.push(port);
        }
    }
    Ok((bridge, foreign))
}

fn occupied_port_probe_client() -> Result<reqwest::Client, CliError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(OCCUPIED_PORT_PROBE_TIMEOUT)
        .timeout(OCCUPIED_PORT_PROBE_TIMEOUT)
        .build()
        .map_err(|error| {
            CliError::Config(format!(
                "could not build Browser Bridge conflict probe: {error}"
            ))
        })
}

async fn occupied_port_is_current_bridge(
    client: &reqwest::Client,
    port: u16,
    secret: &str,
) -> bool {
    let client_nonce = uuid::Uuid::new_v4().to_string();
    let response = match client
        .post(format!("http://127.0.0.1:{port}/v3/challenge/hello"))
        .header(
            "Origin",
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
        )
        .header("X-Sunox-Extension", "1")
        .json(&serde_json::json!({
            "version": PROTOCOL_VERSION,
            "client_nonce": &client_nonce,
        }))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        _ => return false,
    };
    let content_type_is_json = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if !content_type_is_json
        || response
            .content_length()
            .is_some_and(|length| length > MAX_OCCUPIED_PORT_HELLO_RESPONSE_BYTES as u64)
    {
        return false;
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return false;
        };
        if body.len().saturating_add(chunk.len()) > MAX_OCCUPIED_PORT_HELLO_RESPONSE_BYTES {
            return false;
        }
        body.extend_from_slice(&chunk);
    }
    let hello: OwnedHelloResponse = match serde_json::from_slice(&body) {
        Ok(hello) => hello,
        Err(_) => return false,
    };
    if hello.version != PROTOCOL_VERSION || !valid_nonce(&hello.server_nonce) {
        return false;
    }
    let expected = authentication_proof(
        secret,
        "sunox-bridge-server-v3",
        &[&port.to_string(), &client_nonce, &hello.server_nonce],
    );
    constant_time_eq(hello.proof.as_bytes(), expected.as_bytes())
}

async fn wait_for_claim(state: &BridgeState) -> bool {
    if wait_for_claim_signal(state, ACTIVE_TAB_DISCOVERY_TIMEOUT).await {
        return true;
    }
    eprintln!("Waiting for the Chrome extension's hidden challenge context...");
    let _ = wait_for_claim_signal(state, BACKGROUND_TAB_DISCOVERY_TIMEOUT).await;
    close_discovery(state)
}

async fn wait_for_claim_signal(state: &BridgeState, duration: Duration) -> bool {
    let notified = state.claimed_notify.notified();
    if state.claim_state.load(Ordering::Acquire) == CLAIMED {
        return true;
    }
    let _ = timeout(duration, notified).await;
    state.claim_state.load(Ordering::Acquire) == CLAIMED
}

async fn wait_for_probe_ack_signal(state: &BridgeState, duration: Duration) -> bool {
    let notified = state.probe_acknowledged_notify.notified();
    if state.probe_acknowledged.load(Ordering::Acquire) {
        return true;
    }
    let _ = timeout(duration, notified).await;
    state.probe_acknowledged.load(Ordering::Acquire)
}

fn close_discovery(state: &BridgeState) -> bool {
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
    let mut response = route_request(&request, &state)?;
    let carries_result_delivery = response.result_delivery.is_some();
    let write = write_response(
        &mut stream,
        &mut response,
        valid_extension_origin(&origin).then_some(origin.as_str()),
    );
    if carries_result_delivery {
        timeout(RESULT_WRITE_TIMEOUT, write).await.map_err(|_| {
            CliError::Config("browser bridge result acknowledgement write timed out".into())
        })?
    } else {
        write.await
    }
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
    {
        return Ok(HttpResponse::empty(403, "Forbidden"));
    }

    match request.path.as_str() {
        "/v3/challenge/hello" => hello(request, state),
        "/v3/challenge/claim" => claim(request, state),
        "/v3/challenge/probe-ack" => acknowledge_probe(request, state),
        "/v3/challenge/result" => receive_result(request, state),
        _ => Ok(HttpResponse::empty(404, "Not Found")),
    }
}

fn hello(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    let hello: HelloRequest = match serde_json::from_slice(&request.body) {
        Ok(hello) => hello,
        Err(_) => return Ok(HttpResponse::empty(400, "Bad Request")),
    };
    if hello.version != PROTOCOL_VERSION || !valid_nonce(&hello.client_nonce) {
        return Ok(HttpResponse::empty(422, "Unprocessable Content"));
    }
    let port = state.port.to_string();
    HttpResponse::json(
        200,
        "OK",
        HelloResponse {
            version: PROTOCOL_VERSION,
            server_nonce: &state.server_nonce,
            proof: authentication_proof(
                &state.secret,
                "sunox-bridge-server-v3",
                &[&port, &hello.client_nonce, &state.server_nonce],
            ),
        },
    )
}

fn claim(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    let claim: ClaimRequest = match serde_json::from_slice(&request.body) {
        Ok(claim) => claim,
        Err(_) => return Ok(HttpResponse::empty(400, "Bad Request")),
    };
    if claim.version != PROTOCOL_VERSION
        || claim.runtime_build != BROWSER_BRIDGE_RUNTIME_BUILD
        || claim.client_id.is_empty()
        || claim.client_id.len() > 128
        || !is_suno_page(&claim.page_url)
        || !valid_nonce(&claim.client_nonce)
        || claim.server_nonce != state.server_nonce
    {
        return Ok(HttpResponse::empty(422, "Unprocessable Content"));
    }
    let port = state.port.to_string();
    let expected_proof = authentication_proof(
        &state.secret,
        "sunox-bridge-client-v3",
        &[
            &port,
            &claim.client_nonce,
            &claim.server_nonce,
            &claim.runtime_build,
            &claim.client_id,
            &claim.page_url,
        ],
    );
    if !constant_time_eq(claim.proof.as_bytes(), expected_proof.as_bytes()) {
        return Ok(HttpResponse::empty(403, "Forbidden"));
    }
    if state
        .claim_state
        .compare_exchange(CLAIM_PENDING, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(HttpResponse::empty(409, "Conflict"));
    }
    *state
        .claim_session
        .lock()
        .expect("bridge claim session mutex poisoned") = Some(ClaimSession {
        client_nonce: claim.client_nonce,
        server_nonce: claim.server_nonce,
    });
    state.claimed_notify.notify_waiters();
    match state.operation {
        BridgeOperation::Challenge(provider) => HttpResponse::json(
            200,
            "OK",
            ClaimResponse {
                version: PROTOCOL_VERSION,
                request_id: &state.request_id,
                provider: provider.bridge_name(),
            },
        ),
        // A probe returns a distinct instruction that the transport must
        // acknowledge. It contains no provider, so the offscreen controller
        // cannot create a Suno iframe or execute a challenge.
        BridgeOperation::Probe => HttpResponse::json(
            200,
            "OK",
            ProbeClaimResponse {
                version: PROTOCOL_VERSION,
                request_id: &state.request_id,
                probe: true,
            },
        ),
    }
}

fn acknowledge_probe(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    if !matches!(state.operation, BridgeOperation::Probe) {
        return Ok(HttpResponse::empty(409, "Conflict"));
    }
    let ack: ProbeAckRequest = match serde_json::from_slice(&request.body) {
        Ok(ack) => ack,
        Err(_) => return Ok(HttpResponse::empty(400, "Bad Request")),
    };
    if ack.version != PROTOCOL_VERSION
        || ack.runtime_build != BROWSER_BRIDGE_RUNTIME_BUILD
        || ack.request_id != state.request_id
    {
        return Ok(HttpResponse::empty(409, "Conflict"));
    }
    {
        let session = state
            .claim_session
            .lock()
            .expect("bridge claim session mutex poisoned");
        let Some(session) = session.as_ref() else {
            return Ok(HttpResponse::empty(409, "Conflict"));
        };
        if ack.client_nonce != session.client_nonce || ack.server_nonce != session.server_nonce {
            return Ok(HttpResponse::empty(403, "Forbidden"));
        }
    }
    let port = state.port.to_string();
    let expected_proof = authentication_proof(
        &state.secret,
        "sunox-bridge-probe-ack-v3",
        &[
            &port,
            &ack.client_nonce,
            &ack.server_nonce,
            &ack.request_id,
            &ack.runtime_build,
        ],
    );
    if !constant_time_eq(ack.proof.as_bytes(), expected_proof.as_bytes()) {
        return Ok(HttpResponse::empty(403, "Forbidden"));
    }
    state.probe_acknowledged.store(true, Ordering::Release);
    state.probe_acknowledged_notify.notify_waiters();
    Ok(HttpResponse::empty(204, "No Content"))
}

fn receive_result(request: &HttpRequest, state: &BridgeState) -> Result<HttpResponse, CliError> {
    let result: ResultRequest = match serde_json::from_slice(&request.body) {
        Ok(result) => result,
        Err(_) => return Ok(HttpResponse::empty(400, "Bad Request")),
    };
    if result.version != PROTOCOL_VERSION || result.request_id != state.request_id {
        return Ok(HttpResponse::empty(409, "Conflict"));
    }
    {
        let session = state
            .claim_session
            .lock()
            .expect("bridge claim session mutex poisoned");
        let Some(session) = session.as_ref() else {
            return Ok(HttpResponse::empty(409, "Conflict"));
        };
        if result.client_nonce != session.client_nonce
            || result.server_nonce != session.server_nonce
        {
            return Ok(HttpResponse::empty(403, "Forbidden"));
        }
    }
    let (kind, value, bridge_result) = match (&result.token, &result.error) {
        (Some(token), None) if (20..=16_384).contains(&token.len()) => {
            ("token", token.as_str(), BridgeResult::Token(token.clone()))
        }
        (None, Some(error)) if !error.is_empty() && error.len() <= 1_000 => {
            ("error", error.as_str(), BridgeResult::Error(error.clone()))
        }
        _ => return Ok(HttpResponse::empty(422, "Unprocessable Content")),
    };
    let port = state.port.to_string();
    let expected_proof = authentication_proof(
        &state.secret,
        "sunox-bridge-result-v3",
        &[
            &port,
            &result.client_nonce,
            &result.server_nonce,
            &result.request_id,
            kind,
            value,
        ],
    );
    if !constant_time_eq(result.proof.as_bytes(), expected_proof.as_bytes()) {
        return Ok(HttpResponse::empty(403, "Forbidden"));
    }
    let fingerprint = result.proof;
    let mut slot = state
        .result_delivery
        .slot
        .lock()
        .expect("bridge result delivery mutex poisoned");
    let current = std::mem::replace(&mut *slot, ResultSlot::Closed);
    let sender = match current {
        ResultSlot::Pending {
            sender,
            fingerprint: expected,
        } => {
            if sender.is_closed() {
                *slot = ResultSlot::Closed;
                return Ok(HttpResponse::empty(410, "Gone"));
            }
            if expected
                .as_deref()
                .is_some_and(|value| value != fingerprint)
            {
                *slot = ResultSlot::Pending {
                    sender,
                    fingerprint: expected,
                };
                return Ok(HttpResponse::empty(409, "Conflict"));
            }
            sender
        }
        ResultSlot::Writing {
            fingerprint: expected,
        } => {
            let is_replay = expected == fingerprint;
            *slot = ResultSlot::Writing {
                fingerprint: expected,
            };
            return Ok(if is_replay {
                HttpResponse::empty(425, "Too Early")
            } else {
                HttpResponse::empty(409, "Conflict")
            });
        }
        ResultSlot::Committed {
            fingerprint: expected,
        } => {
            let is_replay = expected == fingerprint;
            *slot = ResultSlot::Committed {
                fingerprint: expected,
            };
            return Ok(if is_replay {
                HttpResponse::empty(204, "No Content")
            } else {
                HttpResponse::empty(409, "Conflict")
            });
        }
        ResultSlot::Closed => {
            *slot = ResultSlot::Closed;
            return Ok(HttpResponse::empty(410, "Gone"));
        }
    };
    *slot = ResultSlot::Writing {
        fingerprint: fingerprint.clone(),
    };
    drop(slot);
    Ok(HttpResponse::empty(204, "No Content").with_result_delivery(
        Arc::clone(&state.result_delivery),
        fingerprint,
        sender,
        bridge_result,
    ))
}

fn valid_nonce(nonce: &str) -> bool {
    (16..=128).contains(&nonce.len())
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn authentication_proof(secret: &str, label: &str, fields: &[&str]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts keys of any non-empty length");
    update_authentication_payload(&mut mac, label, fields);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn update_authentication_payload(mac: &mut Hmac<Sha256>, label: &str, fields: &[&str]) {
    mac.update(label.as_bytes());
    mac.update(&[0]);
    for field in fields {
        let bytes = field.as_bytes();
        mac.update(&(bytes.len() as u32).to_be_bytes());
        mac.update(bytes);
    }
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
    reqwest::Url::parse(page_url).is_ok_and(|url| url.as_str() == "https://suno.com/")
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

async fn write_response<W>(
    stream: &mut W,
    response: &mut HttpResponse,
    extension_origin: Option<&str>,
) -> Result<(), CliError>
where
    W: AsyncWrite + Unpin,
{
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
            "Access-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, X-Sunox-Extension\r\nAccess-Control-Allow-Private-Network: true\r\nVary: Origin\r\n"
        ));
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.flush().await?;
    // Do not wake the challenge caller until its authenticated result response
    // has been flushed. If the write is cancelled or fails before this point,
    // dropping ResultDelivery restores the one-shot sender so the extension
    // can retry the same signed result.
    response.acknowledge_result_delivery();
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
    use std::sync::{Arc, Mutex, atomic::Ordering};
    use std::time::{Duration, Instant};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{Mutex as AsyncMutex, Notify, oneshot};
    use tokio_util::sync::CancellationToken;

    use super::{
        BridgeOperation, BridgeResult, BridgeState, CLAIM_CLOSED, CLAIM_PENDING,
        COMPLETION_TIMEOUT_MS, CONNECTION_TIMEOUT, HttpRequest,
        MAX_OCCUPIED_PORT_HELLO_RESPONSE_BYTES, PORT_COUNT, PROTOCOL_VERSION, RESULT_REPLAY_GRACE,
        RESULT_WRITE_TIMEOUT, ResultDeliveryState, ResultSlot, acknowledge_probe,
        authentication_proof, constant_time_eq, finish_or_close_timed_out_result, is_suno_page,
        missing_secret_probe_status, occupied_port_is_current_bridge, occupied_port_probe_client,
        probe_with_secret_in_range, route_request, serve, valid_extension_origin,
        wait_for_probe_ack_signal, write_response,
    };
    use crate::api::challenge::ChallengeProvider;
    use crate::captcha::{
        BridgeProbeStatus, SUNO_CHALLENGE_SDK_READY_TIMEOUT_MS, SUNO_HCAPTCHA_SILENT_TIMEOUT_MS,
        bridge_contract::BROWSER_BRIDGE_RUNTIME_BUILD,
    };

    static PROBE_PORT_TEST_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

    #[test]
    fn missing_secret_probe_distinguishes_never_installed_from_broken_installation() {
        assert_eq!(
            missing_secret_probe_status(false),
            BridgeProbeStatus::NotConfigured
        );
        assert_eq!(
            missing_secret_probe_status(true),
            BridgeProbeStatus::Unavailable
        );
    }

    fn javascript_number(source: &str, name: &str) -> u64 {
        let declaration = format!("const {name} = ");
        let start = source
            .find(&declaration)
            .unwrap_or_else(|| panic!("missing JavaScript declaration {name}"))
            + declaration.len();
        source[start..]
            .split(';')
            .next()
            .expect("JavaScript numeric constant")
            .replace('_', "")
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("invalid JavaScript numeric constant {name}"))
    }

    #[test]
    fn bridge_timeout_layers_cover_cold_start_and_silent_challenge() {
        let page = include_str!("../../assets/browser-extension/page.js");
        let bridge = include_str!("../../assets/browser-extension/bridge.js");
        let offscreen = include_str!("../../assets/browser-extension/offscreen.js");
        let service_worker = include_str!("../../assets/browser-extension/service-worker.js");
        let transport = include_str!("../../assets/browser-extension/transport-loopback.js");

        let sdk_ready_timeout = javascript_number(page, "CHALLENGE_SDK_READY_TIMEOUT_MS");
        let hcaptcha_silent_timeout = javascript_number(page, "HCAPTCHA_SILENT_TIMEOUT_MS");
        let turnstile_shared_attempt_budget =
            javascript_number(page, "TURNSTILE_SHARED_ATTEMPT_BUDGET_MS");
        let page_timeout = javascript_number(bridge, "challengePageTimeoutMs");
        let managed_frame_warmup_grace = javascript_number(offscreen, "managedFrameWarmupGraceMs");
        let managed_frame_prepare_timeout =
            javascript_number(offscreen, "managedFramePrepareTimeoutMs");
        let managed_frame_ready_timeout =
            javascript_number(offscreen, "managedFrameReadyTimeoutMs");
        let managed_frame_release_timeout =
            javascript_number(offscreen, "managedFrameReleaseTimeoutMs");
        let managed_frame_result_timeout =
            javascript_number(offscreen, "managedFrameResultTimeoutMs");
        let offscreen_busy_max_age = javascript_number(service_worker, "OFFSCREEN_BUSY_MAX_AGE_MS");
        let transport_request_timeout = javascript_number(transport, "requestTimeoutMs");
        let result_delivery_deadline = javascript_number(transport, "resultDeliveryDeadlineMs");
        let result_retry_initial_delay = javascript_number(transport, "resultRetryInitialDelayMs");
        let result_retry_max_delay = javascript_number(transport, "resultRetryMaxDelayMs");
        let worst_case_transport_budget =
            transport_request_timeout * (u64::from(PORT_COUNT) + 3) + result_delivery_deadline;

        assert_eq!(sdk_ready_timeout, SUNO_CHALLENGE_SDK_READY_TIMEOUT_MS);
        assert_eq!(hcaptcha_silent_timeout, SUNO_HCAPTCHA_SILENT_TIMEOUT_MS);
        assert_eq!(turnstile_shared_attempt_budget, 30_000);
        assert_eq!(result_delivery_deadline, 1_350);
        assert_eq!(result_retry_initial_delay, 25);
        assert_eq!(result_retry_max_delay, 200);
        assert!(
            transport_request_timeout > RESULT_WRITE_TIMEOUT.as_millis() as u64,
            "the browser fetch must outlive the result writer"
        );
        assert!(
            result_delivery_deadline > CONNECTION_TIMEOUT.as_millis() as u64,
            "the exact-result retry deadline must outlive the server writer"
        );
        assert!(
            RESULT_REPLAY_GRACE.as_millis() as u64 >= result_delivery_deadline,
            "the CLI listener must outlive every bounded terminal-result replay"
        );
        assert_eq!(page_timeout, 50_000);
        assert_eq!(managed_frame_warmup_grace, 3_000);
        assert_eq!(managed_frame_prepare_timeout, 9_000);
        assert_eq!(managed_frame_ready_timeout, 45_000);
        assert_eq!(managed_frame_release_timeout, 500);
        assert_eq!(managed_frame_result_timeout, 65_000);
        assert_eq!(offscreen_busy_max_age, 127_000);
        assert_eq!(COMPLETION_TIMEOUT_MS, 130_000);
        assert!(page_timeout > sdk_ready_timeout + turnstile_shared_attempt_budget);
        assert!(page_timeout > sdk_ready_timeout + hcaptcha_silent_timeout);
        assert!(managed_frame_result_timeout > page_timeout);
        let managed_lifecycle_budget = managed_frame_prepare_timeout
            + managed_frame_ready_timeout
            + managed_frame_result_timeout
            + managed_frame_release_timeout
            + worst_case_transport_budget;
        assert_eq!(managed_lifecycle_budget, 124_700);
        assert!(
            offscreen_busy_max_age > managed_lifecycle_budget,
            "busy recovery must preserve prepare, hidden frame, result, release, and transport budgets"
        );
        assert!(
            COMPLETION_TIMEOUT_MS > offscreen_busy_max_age,
            "the CLI must outlive Browser Bridge stale-busy recovery"
        );
        assert!(COMPLETION_TIMEOUT_MS > managed_frame_result_timeout);
    }

    fn state_with_provider(
        secret: &str,
        provider: ChallengeProvider,
    ) -> (BridgeState, oneshot::Receiver<BridgeResult>) {
        let (sender, receiver) = oneshot::channel();
        (
            BridgeState {
                port: 29_764,
                request_id: "request-a".into(),
                server_nonce: "server-nonce-00000001".into(),
                operation: BridgeOperation::Challenge(provider),
                secret: secret.into(),
                claim_state: CLAIM_PENDING.into(),
                claimed_notify: Notify::new(),
                probe_acknowledged: false.into(),
                probe_acknowledged_notify: Notify::new(),
                result_delivery: Arc::new(ResultDeliveryState {
                    slot: Mutex::new(ResultSlot::Pending {
                        sender,
                        fingerprint: None,
                    }),
                    changed: Notify::new(),
                }),
                claim_session: Mutex::new(None),
            },
            receiver,
        )
    }

    fn state(secret: &str) -> (BridgeState, oneshot::Receiver<BridgeResult>) {
        state_with_provider(secret, ChallengeProvider::HCaptcha)
    }

    fn probe_state(secret: &str) -> BridgeState {
        let (mut state, _receiver) = state(secret);
        state.operation = BridgeOperation::Probe;
        state.result_delivery = Arc::new(ResultDeliveryState {
            slot: Mutex::new(ResultSlot::Closed),
            changed: Notify::new(),
        });
        state
    }

    async fn reserve_contiguous_test_ports(port_count: u16) -> (u16, Vec<TcpListener>) {
        for port_start in 40_000..60_000 - port_count {
            let mut listeners = Vec::with_capacity(usize::from(port_count));
            for port in port_start..port_start + port_count {
                match TcpListener::bind(("127.0.0.1", port)).await {
                    Ok(listener) => listeners.push(listener),
                    Err(_) => break,
                }
            }
            if listeners.len() == usize::from(port_count) {
                return (port_start, listeners);
            }
        }
        panic!("could not reserve {port_count} contiguous loopback ports for Browser Bridge test");
    }

    fn request(path: &str, body: serde_json::Value) -> HttpRequest {
        HttpRequest {
            method: "POST".into(),
            path: path.into(),
            headers: HashMap::from([
                (
                    "origin".into(),
                    "chrome-extension://abcdefghijklmnopabcdefghijklmnop".into(),
                ),
                ("x-sunox-extension".into(), "1".into()),
            ]),
            body: serde_json::to_vec(&body).expect("serialize body"),
        }
    }

    fn claim_request(secret: &str, page_url: &str) -> HttpRequest {
        let fields = [
            "29764",
            "client-nonce-00000001",
            "server-nonce-00000001",
            BROWSER_BRIDGE_RUNTIME_BUILD,
            "client-a",
            page_url,
        ];
        request(
            "/v3/challenge/claim",
            serde_json::json!({
                "version": PROTOCOL_VERSION,
                "runtime_build": BROWSER_BRIDGE_RUNTIME_BUILD,
                "client_id": "client-a",
                "page_url": page_url,
                "client_nonce": "client-nonce-00000001",
                "server_nonce": "server-nonce-00000001",
                "proof": authentication_proof(secret, "sunox-bridge-client-v3", &fields)
            }),
        )
    }

    fn probe_ack_request(secret: &str) -> HttpRequest {
        let fields = [
            "29764",
            "client-nonce-00000001",
            "server-nonce-00000001",
            "request-a",
            BROWSER_BRIDGE_RUNTIME_BUILD,
        ];
        request(
            "/v3/challenge/probe-ack",
            serde_json::json!({
                "version": PROTOCOL_VERSION,
                "runtime_build": BROWSER_BRIDGE_RUNTIME_BUILD,
                "request_id": "request-a",
                "client_nonce": "client-nonce-00000001",
                "server_nonce": "server-nonce-00000001",
                "proof": authentication_proof(
                    secret,
                    "sunox-bridge-probe-ack-v3",
                    &fields
                )
            }),
        )
    }

    fn result_request(secret: &str, token: &str) -> HttpRequest {
        let fields = [
            "29764",
            "client-nonce-00000001",
            "server-nonce-00000001",
            "request-a",
            "token",
            token,
        ];
        request(
            "/v3/challenge/result",
            serde_json::json!({
                "version": PROTOCOL_VERSION,
                "request_id": "request-a",
                "client_nonce": "client-nonce-00000001",
                "server_nonce": "server-nonce-00000001",
                "token": token,
                "error": null,
                "proof": authentication_proof(secret, "sunox-bridge-result-v3", &fields)
            }),
        )
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
    fn suno_claim_requires_the_clean_discovery_url() {
        assert!(is_suno_page("https://suno.com/"));
        assert!(is_suno_page("https://suno.com"));
        assert!(!is_suno_page("https://suno.com/home/advanced"));
        assert!(!is_suno_page("https://suno.com/?unexpected=1"));
        assert!(!is_suno_page("https://suno.com/#unexpected"));
        assert!(!is_suno_page("https://user:password@suno.com/"));
        assert!(!is_suno_page("https://suno.com:8443/"));
        assert!(!is_suno_page("http://suno.com/"));
        assert!(!is_suno_page("https://evil.suno.com/"));
        assert!(!is_suno_page("https://suno.com.evil.example/"));
    }

    #[test]
    fn authentication_proof_comparison_is_exact() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn obsolete_v2_bridge_cannot_complete_the_v3_handshake() {
        assert_eq!(PROTOCOL_VERSION, 3);
        let (state, _receiver) = state("secret-value");
        let old_route = request(
            "/v2/challenge/hello",
            serde_json::json!({
                "version": 2,
                "client_nonce": "client-nonce-00000001"
            }),
        );
        let old_version = request(
            "/v3/challenge/hello",
            serde_json::json!({
                "version": 2,
                "client_nonce": "client-nonce-00000001"
            }),
        );
        let current = request(
            "/v3/challenge/hello",
            serde_json::json!({
                "version": PROTOCOL_VERSION,
                "client_nonce": "client-nonce-00000001"
            }),
        );

        assert_eq!(
            route_request(&old_route, &state).expect("old route").status,
            404
        );
        assert_eq!(
            route_request(&old_version, &state)
                .expect("old version")
                .status,
            422
        );
        assert_eq!(
            route_request(&current, &state)
                .expect("current version")
                .status,
            200
        );
    }

    #[test]
    fn first_valid_suno_tab_claims_the_challenge() {
        let (state, _receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");

        let first = route_request(&claim, &state).expect("first response");
        let second = route_request(&claim, &state).expect("second response");

        assert_eq!(first.status, 200);
        assert_eq!(second.status, 409);
    }

    #[test]
    fn claim_response_serializes_each_challenge_provider() {
        for (provider, expected) in [
            (ChallengeProvider::HCaptcha, "hcaptcha"),
            (ChallengeProvider::Turnstile, "turnstile"),
        ] {
            let (state, _receiver) = state_with_provider("secret-value", provider);
            let claim = claim_request("secret-value", "https://suno.com/");
            let response = route_request(&claim, &state).expect("claim response");
            let body: serde_json::Value =
                serde_json::from_slice(&response.body).expect("claim response json");

            assert_eq!(response.status, 200);
            assert_eq!(body["provider"], expected);
        }
    }

    #[test]
    fn probe_claim_requires_a_signed_runtime_ack_without_a_provider() {
        let state = probe_state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");

        let response = route_request(&claim, &state).expect("probe response");
        let body: serde_json::Value =
            serde_json::from_slice(&response.body).expect("probe response json");

        assert_eq!(response.status, 200);
        assert_eq!(body["probe"], true);
        assert_eq!(body["request_id"], "request-a");
        assert!(body.get("provider").is_none());
        assert_eq!(
            state.claim_state.load(std::sync::atomic::Ordering::Acquire),
            super::CLAIMED
        );
        assert!(!state.probe_acknowledged.load(Ordering::Acquire));

        let ack = probe_ack_request("secret-value");
        assert_eq!(
            acknowledge_probe(&ack, &state)
                .expect("probe acknowledgement")
                .status,
            204
        );
        assert!(state.probe_acknowledged.load(Ordering::Acquire));
    }

    #[test]
    fn a_claim_cannot_start_after_discovery_has_closed() {
        let (state, _receiver) = state("secret-value");
        state
            .claim_state
            .store(CLAIM_CLOSED, std::sync::atomic::Ordering::Release);
        let claim = claim_request("secret-value", "https://suno.com/");

        assert_eq!(route_request(&claim, &state).expect("response").status, 409);
    }

    #[tokio::test]
    async fn matching_result_is_not_released_before_the_http_acknowledgement() {
        let (state, mut receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");
        assert_eq!(route_request(&claim, &state).expect("claim").status, 200);
        let result = result_request("secret-value", "abcdefghijklmnopqrstuvwxyz");

        let mut response = route_request(&result, &state).expect("result response");
        assert!(
            matches!(
                receiver.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "the CLI must not consume the result before the extension receives its HTTP acknowledgement"
        );
        let (mut response_writer, mut response_reader) = tokio::io::duplex(64);
        let write = tokio::spawn(async move {
            write_response(
                &mut response_writer,
                &mut response,
                Some("chrome-extension://abcdefghijklmnopabcdefghijklmnop"),
            )
            .await
        });
        tokio::task::yield_now().await;
        assert!(
            matches!(
                receiver.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "backpressure before the HTTP response flush must keep the result pending"
        );
        assert_eq!(
            route_request(&result, &state)
                .expect("in-flight replay")
                .status,
            425,
            "an identical replay must wait for the first response to commit"
        );
        assert_eq!(
            route_request(
                &result_request("secret-value", "zyxwvutsrqponmlkjihgfedcba"),
                &state,
            )
            .expect("conflicting in-flight result")
            .status,
            409,
            "a different terminal result cannot replace an in-flight result"
        );
        let mut response_bytes = Vec::new();
        response_reader
            .read_to_end(&mut response_bytes)
            .await
            .expect("read result response");
        write
            .await
            .expect("response writer task")
            .expect("write result");
        let BridgeResult::Token(token) = receiver.await.expect("bridge result") else {
            panic!("expected token");
        };

        assert!(
            response_bytes.starts_with(b"HTTP/1.1 204 No Content"),
            "the acknowledged response must be the successful one-time result"
        );
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");
        let committed_replay = route_request(&result, &state).expect("committed replay");
        assert_eq!(committed_replay.status, 204);
        assert!(
            committed_replay.result_delivery.is_none(),
            "an identical committed replay is idempotent and must not deliver twice"
        );
        assert_eq!(
            route_request(
                &result_request("secret-value", "zyxwvutsrqponmlkjihgfedcba"),
                &state,
            )
            .expect("conflicting committed result")
            .status,
            409,
            "a different terminal result cannot replace a committed result"
        );
    }

    #[tokio::test]
    async fn an_unwritten_result_response_can_be_retried() {
        let (state, mut receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");
        assert_eq!(route_request(&claim, &state).expect("claim").status, 200);
        let result = result_request("secret-value", "abcdefghijklmnopqrstuvwxyz");

        let abandoned = route_request(&result, &state).expect("first result response");
        assert_eq!(abandoned.status, 204);
        drop(abandoned);
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(
            route_request(
                &result_request("secret-value", "zyxwvutsrqponmlkjihgfedcba"),
                &state,
            )
            .expect("conflicting retry")
            .status,
            409,
            "a write failure may restore the sender but must keep the original terminal fingerprint"
        );

        let mut retry = route_request(&result, &state).expect("retried result response");
        assert_eq!(retry.status, 204);
        retry.acknowledge_result_delivery();
        let BridgeResult::Token(token) = receiver.await.expect("bridge result") else {
            panic!("expected token");
        };
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn a_closed_result_receiver_is_never_acknowledged() {
        let (state, receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");
        assert_eq!(route_request(&claim, &state).expect("claim").status, 200);
        drop(receiver);

        let response = route_request(
            &result_request("secret-value", "abcdefghijklmnopqrstuvwxyz"),
            &state,
        )
        .expect("closed result response");
        assert_eq!(response.status, 410);
        assert!(
            response.result_delivery.is_none(),
            "a closed CLI receiver must not be represented as an accepted result"
        );
    }

    #[tokio::test]
    async fn completion_timeout_waits_for_an_in_flight_result_commit() {
        let (state, mut receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");
        assert_eq!(route_request(&claim, &state).expect("claim").status, 200);
        let result = result_request("secret-value", "abcdefghijklmnopqrstuvwxyz");
        let mut response = route_request(&result, &state).expect("result response");

        let finish = finish_or_close_timed_out_result(&state, &mut receiver);
        let acknowledge = async {
            tokio::task::yield_now().await;
            response.acknowledge_result_delivery();
        };
        let (finished, ()) = tokio::join!(finish, acknowledge);
        let BridgeResult::Token(token) = finished
            .expect("in-flight result must finish")
            .expect("result receiver")
        else {
            panic!("expected token");
        };
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");
    }

    #[tokio::test]
    async fn result_transition_before_notify_poll_preserves_a_wakeup() {
        let (state, receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");
        assert_eq!(route_request(&claim, &state).expect("claim").status, 200);
        let result = result_request("secret-value", "abcdefghijklmnopqrstuvwxyz");
        let mut response = route_request(&result, &state).expect("result response");
        let changed = state.result_delivery.changed.notified();

        response.acknowledge_result_delivery();
        tokio::time::timeout(Duration::from_millis(10), changed)
            .await
            .expect("a transition before the first poll must leave a notify permit");
        let BridgeResult::Token(token) = receiver.await.expect("result receiver") else {
            panic!("expected token");
        };
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");
    }

    #[tokio::test]
    async fn completion_timeout_closes_a_restored_pending_result() {
        let (state, mut receiver) = state("secret-value");
        let claim = claim_request("secret-value", "https://suno.com/");
        assert_eq!(route_request(&claim, &state).expect("claim").status, 200);
        let result = result_request("secret-value", "abcdefghijklmnopqrstuvwxyz");
        let response = route_request(&result, &state).expect("result response");

        let finish = finish_or_close_timed_out_result(&state, &mut receiver);
        let abandon = async {
            tokio::task::yield_now().await;
            drop(response);
        };
        let (finished, ()) = tokio::join!(finish, abandon);
        assert!(
            finished.is_none(),
            "an unwritten result restored after the completion deadline must be closed"
        );
        assert_eq!(
            route_request(&result, &state)
                .expect("post-timeout result")
                .status,
            410
        );
    }

    #[test]
    fn invalid_origin_or_secret_cannot_claim() {
        let (state, _receiver) = state("secret-value");
        let mut bad_origin = claim_request("secret-value", "https://suno.com/");
        bad_origin
            .headers
            .insert("origin".into(), "https://evil.example".into());
        let bad_secret = claim_request("wrong-secret", "https://suno.com/");

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

        let hello_body = serde_json::json!({
            "version": PROTOCOL_VERSION,
            "client_nonce": "client-nonce-00000001"
        })
        .to_string();
        let hello_response = raw_request(address, "/v3/challenge/hello", &hello_body).await;
        assert!(hello_response.starts_with("HTTP/1.1 200 OK"));
        assert!(!hello_response.contains("secret-value"));

        let claim = claim_request("secret-value", "https://suno.com/");
        let claim_body = String::from_utf8(claim.body).expect("claim body");
        let claim_response = raw_request(address, "/v3/challenge/claim", &claim_body).await;
        assert!(claim_response.starts_with("HTTP/1.1 200 OK"));
        assert!(claim_response.contains(
            "Access-Control-Allow-Origin: chrome-extension://abcdefghijklmnopabcdefghijklmnop"
        ));
        assert!(claim_response.contains("\"provider\":\"hcaptcha\""));

        let result = result_request("secret-value", "abcdefghijklmnopqrstuvwxyz");
        let result_body = String::from_utf8(result.body).expect("result body");
        let result_response = raw_request(address, "/v3/challenge/result", &result_body).await;
        assert!(result_response.starts_with("HTTP/1.1 204 No Content"));
        let BridgeResult::Token(token) = receiver.await.expect("bridge result") else {
            panic!("expected token");
        };
        assert_eq!(token, "abcdefghijklmnopqrstuvwxyz");

        cancellation.cancel();
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn loopback_probe_accepts_a_healthy_extension_without_a_challenge() {
        let state = std::sync::Arc::new(probe_state("secret-value"));
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

        let hello_body = serde_json::json!({
            "version": PROTOCOL_VERSION,
            "client_nonce": "client-nonce-00000001"
        })
        .to_string();
        let hello_response = raw_request(address, "/v3/challenge/hello", &hello_body).await;
        assert!(hello_response.starts_with("HTTP/1.1 200 OK"));

        let claim = claim_request("secret-value", "https://suno.com/");
        let claim_body = String::from_utf8(claim.body).expect("claim body");
        let claim_response = raw_request(address, "/v3/challenge/claim", &claim_body).await;

        assert!(claim_response.starts_with("HTTP/1.1 200 OK"));
        assert!(!claim_response.contains("provider"));
        assert!(claim_response.contains("request-a"));
        assert!(
            !wait_for_probe_ack_signal(&state, std::time::Duration::from_millis(1)).await,
            "claim alone must not satisfy the probe"
        );

        let ack = probe_ack_request("secret-value");
        let ack_body = String::from_utf8(ack.body).expect("probe ack body");
        let ack_response = raw_request(address, "/v3/challenge/probe-ack", &ack_body).await;
        assert!(ack_response.starts_with("HTTP/1.1 204 No Content"));
        assert!(
            wait_for_probe_ack_signal(&state, std::time::Duration::from_millis(1)).await,
            "signed runtime acknowledgement should satisfy the probe"
        );

        cancellation.cancel();
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn probe_reports_port_conflict_for_foreign_port_occupancy() {
        let _port_lock = PROBE_PORT_TEST_LOCK.lock().await;
        let port_count = 2;
        let (port_start, mut reserved) = reserve_contiguous_test_ports(port_count).await;

        let lowest_listener = reserved.remove(0);
        drop(reserved);
        let lower_occupied = probe_with_secret_in_range(
            "secret-value".into(),
            Duration::from_millis(20),
            Instant::now(),
            port_start,
            port_count,
        )
        .await
        .expect("probe behind lower occupied port");
        assert_eq!(lower_occupied.status, BridgeProbeStatus::PortConflict);
        assert_eq!(lower_occupied.occupied_ports, vec![port_start]);
        assert!(lower_occupied.bridge_occupied_ports.is_empty());
        assert_eq!(lower_occupied.foreign_occupied_ports, vec![port_start]);
        assert_eq!(lower_occupied.port, Some(port_start + 1));

        drop(lowest_listener);
        // Use a fresh range for the fully occupied case. On Windows, the
        // listener used by the first probe is not guaranteed to be
        // immediately reusable after it closes.
        let (all_port_start, _all_listeners) = reserve_contiguous_test_ports(port_count).await;
        let all_occupied = probe_with_secret_in_range(
            "secret-value".into(),
            Duration::from_millis(20),
            Instant::now(),
            all_port_start,
            port_count,
        )
        .await
        .expect("probe with all ports occupied");
        assert_eq!(all_occupied.status, BridgeProbeStatus::PortConflict);
        assert_eq!(all_occupied.port, None);
        assert_eq!(
            all_occupied.occupied_ports,
            (all_port_start..all_port_start + port_count).collect::<Vec<_>>()
        );
        assert!(all_occupied.bridge_occupied_ports.is_empty());
        assert_eq!(
            all_occupied.foreign_occupied_ports,
            (all_port_start..all_port_start + port_count).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn probe_reports_busy_only_for_an_authenticated_sunox_listener() {
        let _port_lock = PROBE_PORT_TEST_LOCK.lock().await;
        let port_count = 2;
        let (port_start, mut reserved) = reserve_contiguous_test_ports(port_count).await;
        let (mut bridge_state, _receiver) = state("secret-value");
        bridge_state.port = port_start;
        let bridge_state = Arc::new(bridge_state);
        let listener = reserved.remove(0);
        drop(reserved);
        let cancellation = CancellationToken::new();
        let server = tokio::spawn(serve(
            listener,
            Arc::clone(&bridge_state),
            cancellation.clone(),
        ));

        let report = probe_with_secret_in_range(
            "secret-value".into(),
            Duration::from_millis(20),
            Instant::now(),
            port_start,
            port_count,
        )
        .await
        .expect("probe around authenticated Browser Bridge listener");

        assert_eq!(report.status, BridgeProbeStatus::Busy);
        assert_eq!(report.bridge_occupied_ports, vec![port_start]);
        assert!(report.foreign_occupied_ports.is_empty());
        assert_eq!(report.port, Some(port_start + 1));

        cancellation.cancel();
        server.await.expect("current Browser Bridge server");
    }

    #[tokio::test]
    async fn occupied_port_probe_does_not_follow_redirects() {
        let target_listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("redirect target listener: {error}"),
        };
        let target_address = target_listener
            .local_addr()
            .expect("redirect target address");
        let target = tokio::spawn(async move {
            let _ = target_listener.accept().await;
        });
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/v3/challenge/hello\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let Some((redirect_port, redirect_server)) = raw_response_server(redirect).await else {
            return;
        };
        let client = occupied_port_probe_client().expect("occupied-port probe client");

        assert!(
            !occupied_port_is_current_bridge(&client, redirect_port, "secret-value").await,
            "a redirecting foreign listener must not be treated as the current bridge"
        );
        redirect_server.await.expect("redirect server");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !target.is_finished(),
            "the occupied-port probe must not follow a redirect, even to loopback"
        );
        target.abort();
    }

    #[tokio::test]
    async fn occupied_port_probe_rejects_non_json_and_oversized_responses() {
        let Some((non_json_port, non_json_server)) = raw_response_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
                .into(),
        )
        .await
        else {
            return;
        };
        let oversized_length = MAX_OCCUPIED_PORT_HELLO_RESPONSE_BYTES + 1;
        let Some((oversized_port, oversized_server)) = raw_response_server(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {oversized_length}\r\nConnection: close\r\n\r\n"
        ))
        .await
        else {
            return;
        };
        let client = occupied_port_probe_client().expect("occupied-port probe client");

        assert!(!occupied_port_is_current_bridge(&client, non_json_port, "secret-value").await);
        assert!(!occupied_port_is_current_bridge(&client, oversized_port, "secret-value").await);

        non_json_server.await.expect("non-JSON server");
        oversized_server.await.expect("oversized server");
    }

    #[test]
    fn hello_response_proves_server_identity_without_receiving_the_secret() {
        let (state, _receiver) = state("secret-value");
        let hello = request(
            "/v3/challenge/hello",
            serde_json::json!({
                "version": PROTOCOL_VERSION,
                "client_nonce": "client-nonce-00000001"
            }),
        );

        let response = route_request(&hello, &state).expect("hello response");
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("hello JSON");
        let expected = authentication_proof(
            "secret-value",
            "sunox-bridge-server-v3",
            &["29764", "client-nonce-00000001", "server-nonce-00000001"],
        );

        assert_eq!(response.status, 200);
        assert_eq!(body["proof"], expected);
        assert_eq!(
            expected,
            "5d354925cf93fe3cabceca0bdb4c7103e1e1bd000a225003b3ab4785fa976ba9"
        );
        assert!(
            !String::from_utf8(response.body)
                .expect("body")
                .contains("secret-value")
        );
    }

    async fn raw_request(address: std::net::SocketAddr, path: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(address).await.expect("connect");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: chrome-extension://abcdefghijklmnopabcdefghijklmnop\r\nX-Sunox-Extension: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.expect("write");
        stream.shutdown().await.expect("shutdown write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        String::from_utf8(response).expect("UTF-8 response")
    }

    async fn raw_response_server(response: String) -> Option<(u16, tokio::task::JoinHandle<()>)> {
        let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("raw response listener: {error}"),
        };
        let port = listener.local_addr().expect("raw response address").port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("raw response connection");
            let mut request = vec![0; 4096];
            let _ = stream
                .read(&mut request)
                .await
                .expect("raw response request");
            stream
                .write_all(response.as_bytes())
                .await
                .expect("raw response write");
            stream.shutdown().await.expect("raw response shutdown");
        });
        Some((port, server))
    }
}

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::tungstenite::Message;

use crate::core::CliError;

#[derive(Serialize)]
struct CdpRequest<'a> {
    id: u64,
    method: &'a str,
    params: serde_json::Value,
}

type CdpStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) struct CdpSession {
    ws: CdpStream,
    next_id: u64,
}

impl CdpSession {
    pub(super) async fn connect(ws_url: &str) -> Result<Self, CliError> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| CliError::Config(format!("CDP ws connect: {e}")))?;

        Ok(Self { ws, next_id: 0 })
    }

    pub(super) async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CliError> {
        self.call_with_timeout(method, params, Duration::from_secs(60))
            .await
    }

    pub(super) async fn call_with_timeout(
        &mut self,
        method: &str,
        params: serde_json::Value,
        response_timeout: Duration,
    ) -> Result<serde_json::Value, CliError> {
        self.next_id += 1;
        let id = self.next_id;
        let req = CdpRequest { id, method, params };
        let payload = serde_json::to_string(&req).unwrap();

        self.ws
            .send(Message::Text(payload))
            .await
            .map_err(|e| CliError::Config(format!("CDP ws send {method}: {e}")))?;

        // A busy page can emit unrelated CDP events continuously. Keep one
        // absolute deadline for the request so those events cannot extend it.
        let deadline = Instant::now() + response_timeout;
        loop {
            let msg = timeout_at(deadline, self.ws.next())
                .await
                .map_err(|_| CliError::Config(format!("CDP {method} timeout")))?
                .ok_or_else(|| CliError::Config(format!("CDP {method} ws closed")))?
                .map_err(|e| CliError::Config(format!("CDP {method} ws err: {e}")))?;

            let text = match msg {
                Message::Text(text) => text.to_string(),
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                    continue;
                }
                Message::Close(_) => {
                    return Err(CliError::Config(format!("CDP {method} ws closed mid-call")));
                }
            };

            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| CliError::Config(format!("CDP {method} json: {e}")))?;
            if value.get("id").and_then(|id| id.as_u64()) == Some(id) {
                if let Some(err) = value.get("error") {
                    return Err(CliError::Config(format!("CDP {method} error: {err}")));
                }
                return Ok(value
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio::time::timeout;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    use super::CdpSession;

    #[tokio::test]
    async fn unrelated_events_do_not_extend_the_response_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut websocket = accept_async(stream).await.unwrap();
            websocket.next().await.unwrap().unwrap();

            loop {
                websocket
                    .send(Message::Text(
                        r#"{"method":"Network.dataReceived","params":{}}"#.into(),
                    ))
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });

        let mut session = CdpSession::connect(&format!("ws://{address}"))
            .await
            .unwrap();
        let result = timeout(
            Duration::from_millis(250),
            session.call_with_timeout(
                "Runtime.evaluate",
                serde_json::json!({}),
                Duration::from_millis(40),
            ),
        )
        .await
        .expect("the absolute CDP deadline was extended by unrelated events");

        let error = result.unwrap_err().to_string();
        assert!(error.contains("CDP Runtime.evaluate timeout"), "{error}");
        server.abort();
    }
}

use std::time::Duration;

use serde::Deserialize;
use tokio::time::sleep;

use crate::captcha::CDP_HOST;
use crate::core::CliError;
use crate::net::http;

#[derive(Debug, Deserialize)]
pub(in crate::captcha) struct Target {
    #[serde(rename = "type")]
    target_type: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub(in crate::captcha) web_socket_debugger_url: String,
}

pub(in crate::captcha) async fn cdp_version(port: u16) -> Result<serde_json::Value, CliError> {
    let url = format!("http://{CDP_HOST}:{port}/json/version");
    let resp = http::loopback_client()?
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/version: {e}")))?;
    let version: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP json parse: {e}")))?;
    Ok(version)
}

async fn cdp_list(port: u16) -> Result<Vec<Target>, CliError> {
    let url = format!("http://{CDP_HOST}:{port}/json/list");
    let resp = http::loopback_client()?
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/list: {e}")))?;
    let list: Vec<Target> = resp
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP json parse: {e}")))?;
    Ok(list)
}

pub(in crate::captcha) async fn find_or_create_suno_tab(port: u16) -> Result<Target, CliError> {
    let targets = cdp_list(port).await?;
    if let Some(mut target) = targets.into_iter().find(|target| {
        target.target_type == "page"
            && !target.web_socket_debugger_url.is_empty()
            && !target.url.starts_with("chrome://")
    }) {
        target.web_socket_debugger_url =
            validate_and_pin_ws_url(&target.web_socket_debugger_url, port)?;
        return Ok(target);
    }

    let url = format!(
        "http://{CDP_HOST}:{port}/json/new?{}",
        urlencode("about:blank")
    );
    let resp = http::loopback_client()?
        .put(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/new: {e}")))?;
    let mut target: Target = resp
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/new parse: {e}")))?;
    target.web_socket_debugger_url =
        validate_and_pin_ws_url(&target.web_socket_debugger_url, port)?;
    sleep(Duration::from_millis(800)).await;
    Ok(target)
}

fn urlencode(s: &str) -> String {
    s.replace(":", "%3A").replace("/", "%2F")
}

pub(in crate::captcha) fn validate_and_pin_ws_url(
    ws_url: &str,
    expected_port: u16,
) -> Result<String, CliError> {
    let mut url = reqwest::Url::parse(ws_url)
        .map_err(|_| CliError::Config("CDP returned an invalid websocket URL".into()))?;
    if url.scheme() != "ws"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() != Some(expected_port)
        || !url.path().starts_with("/devtools/")
    {
        return Err(CliError::Config(
            "CDP websocket endpoint did not match the owned browser".into(),
        ));
    }

    let host_is_loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !host_is_loopback {
        return Err(CliError::Config(
            "CDP websocket endpoint was not loopback".into(),
        ));
    }

    url.set_host(Some("127.0.0.1"))
        .map_err(|_| CliError::Config("could not pin CDP websocket to loopback".into()))?;
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::validate_and_pin_ws_url;

    #[test]
    fn cdp_websocket_rejects_non_loopback_hosts_and_wrong_ports() {
        assert!(validate_and_pin_ws_url("ws://example.com:43123/devtools/page/a", 43123).is_err());
        assert!(validate_and_pin_ws_url("ws://127.0.0.1:43124/devtools/page/a", 43123).is_err());
    }

    #[test]
    fn cdp_websocket_is_pinned_to_the_owned_loopback_port() {
        let pinned =
            validate_and_pin_ws_url("ws://localhost:43123/devtools/page/owned-target", 43123)
                .expect("owned CDP endpoint");

        assert_eq!(pinned, "ws://127.0.0.1:43123/devtools/page/owned-target");
    }
}

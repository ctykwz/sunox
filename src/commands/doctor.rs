use std::future::Future;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;

use crate::app::AppContext;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize)]
struct NetworkReport {
    ok: bool,
    proxy: crate::net::proxy::ProxyReport,
    targets: Vec<NetworkTarget>,
}

#[derive(Serialize)]
struct NetworkTarget {
    name: &'static str,
    host: &'static str,
    dns: ProbeStage,
    tcp: ProbeStage,
    https: ProbeStage,
}

#[derive(Serialize)]
struct ProbeStage {
    ok: bool,
    latency_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub async fn network(ctx: &AppContext, strict: bool) -> Result<(), CliError> {
    let client = crate::net::proxy::apply_to_client_builder(
        reqwest::Client::builder()
            .connect_timeout(PROBE_TIMEOUT)
            .timeout(PROBE_TIMEOUT),
    )?
    .build()
    .map_err(|error| CliError::Config(format!("network diagnostic client: {error}")))?;
    let (auth, api) = tokio::join!(
        probe_target(
            &client,
            "auth",
            "auth.suno.com",
            "https://auth.suno.com/v1/client"
        ),
        probe_target(
            &client,
            "api",
            "studio-api-prod.suno.com",
            "https://studio-api-prod.suno.com/api/billing/info/",
        )
    );
    let targets = vec![auth, api];
    let report = NetworkReport {
        ok: network_usable(&targets),
        proxy: crate::net::proxy::proxy_report(),
        targets,
    };

    if strict && !report.ok {
        if matches!(ctx.fmt, OutputFormat::Table) {
            print_network_report(&report);
        }
        return Err(CliError::Diagnostic {
            code: "network_degraded",
            message: "one or more Suno network paths are unavailable".into(),
            details: serde_json::to_value(&report)?,
        });
    }

    match ctx.fmt {
        OutputFormat::Json => output::json::success(&report),
        OutputFormat::Table => {
            print_network_report(&report);
        }
    }
    Ok(())
}

fn print_network_report(report: &NetworkReport) {
    eprintln!(
        "Proxy: {}{}",
        report.proxy.source,
        report
            .proxy
            .address
            .as_deref()
            .map(|address| format!(" ({address})"))
            .unwrap_or_default()
    );
    for target in &report.targets {
        eprintln!(
            "{} ({}): DNS {}, direct TCP {}, HTTPS {}",
            target.name,
            target.host,
            stage_summary(&target.dns),
            stage_summary(&target.tcp),
            stage_summary(&target.https),
        );
    }
    if report.ok {
        eprintln!("Network: OK");
    } else {
        eprintln!("Network: degraded — inspect failed stages above");
    }
}

async fn probe_target(
    client: &reqwest::Client,
    name: &'static str,
    host: &'static str,
    url: &'static str,
) -> NetworkTarget {
    let dns = timed_stage(async {
        let addresses = lookup_host((host, 443))
            .await
            .map_err(|error| error.to_string())?;
        if addresses.count() == 0 {
            return Err("DNS returned no addresses".into());
        }
        Ok(None)
    })
    .await;
    let tcp = timed_stage(async {
        TcpStream::connect((host, 443))
            .await
            .map_err(|error| error.to_string())?;
        Ok(None)
    })
    .await;
    let https = timed_stage(async {
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        Ok(Some(response.status().as_u16()))
    })
    .await;
    NetworkTarget {
        name,
        host,
        dns,
        tcp,
        https,
    }
}

async fn timed_stage<F>(future: F) -> ProbeStage
where
    F: Future<Output = Result<Option<u16>, String>>,
{
    let started = Instant::now();
    match timeout(PROBE_TIMEOUT, future).await {
        Ok(Ok(status)) => ProbeStage {
            ok: true,
            latency_ms: started.elapsed().as_millis(),
            status,
            error: None,
        },
        Ok(Err(error)) => ProbeStage {
            ok: false,
            latency_ms: started.elapsed().as_millis(),
            status: None,
            error: Some(error),
        },
        Err(_) => ProbeStage {
            ok: false,
            latency_ms: started.elapsed().as_millis(),
            status: None,
            error: Some(format!(
                "timed out after {} seconds",
                PROBE_TIMEOUT.as_secs()
            )),
        },
    }
}

fn stage_summary(stage: &ProbeStage) -> String {
    if stage.ok {
        match stage.status {
            Some(status) => format!("OK HTTP {status} ({} ms)", stage.latency_ms),
            None => format!("OK ({} ms)", stage.latency_ms),
        }
    } else {
        format!(
            "FAILED: {}",
            stage.error.as_deref().unwrap_or("unknown error")
        )
    }
}

fn network_usable(targets: &[NetworkTarget]) -> bool {
    targets.iter().all(|target| target.https.ok)
}

#[cfg(test)]
mod tests {
    use super::{NetworkTarget, ProbeStage, network_usable, stage_summary, timed_stage};

    #[test]
    fn stage_summary_includes_http_status() {
        let stage = ProbeStage {
            ok: true,
            latency_ms: 12,
            status: Some(401),
            error: None,
        };

        assert_eq!(stage_summary(&stage), "OK HTTP 401 (12 ms)");
    }

    #[tokio::test]
    async fn timed_stage_preserves_probe_failure() {
        let stage = timed_stage(async { Err("connection refused".into()) }).await;

        assert!(!stage.ok);
        assert_eq!(stage.error.as_deref(), Some("connection refused"));
    }

    #[test]
    fn network_health_follows_the_actual_https_path() {
        let failed = || ProbeStage {
            ok: false,
            latency_ms: 1,
            status: None,
            error: Some("proxy-only network".into()),
        };
        let target = NetworkTarget {
            name: "api",
            host: "example.com",
            dns: failed(),
            tcp: failed(),
            https: ProbeStage {
                ok: true,
                latency_ms: 2,
                status: Some(401),
                error: None,
            },
        };

        assert!(network_usable(&[target]));
    }
}

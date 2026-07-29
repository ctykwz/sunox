use std::future::Future;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;

use crate::app::AppContext;
use crate::captcha::bridge_contract::{BROWSER_BRIDGE_RUNTIME_BUILD, PROTOCOL_VERSION};
use crate::captcha::{BridgeProbe, BridgeProbeStatus};
use crate::commands::browser_extension::{BridgePairingStatus, PendingActivation};
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

#[derive(Serialize)]
struct BrowserBridgeReport {
    ok: bool,
    status: &'static str,
    configured: bool,
    responsive: bool,
    busy: bool,
    port_conflict: bool,
    inconclusive: bool,
    runtime_ack_pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_origin: Option<&'static str>,
    pairing_status: &'static str,
    transport: &'static str,
    protocol_version: u8,
    runtime_build: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    occupied_ports: Vec<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bridge_occupied_ports: Vec<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    foreign_occupied_ports: Vec<u16>,
    latency_ms: u128,
    challenge_executed: bool,
    generation_submitted: bool,
    credits_consumed: bool,
}

pub async fn browser_bridge(ctx: &AppContext) -> Result<(), CliError> {
    let pairing_status_before_probe = crate::commands::browser_extension::bridge_pairing_status();
    let probe = if matches!(
        pairing_status_before_probe,
        BridgePairingStatus::PairingMissing
            | BridgePairingStatus::Corrupt
            | BridgePairingStatus::BundleMissing
            | BridgePairingStatus::BundleOutdated
            | BridgePairingStatus::BundleCorrupt
            | BridgePairingStatus::BundleUnrecognized
            | BridgePairingStatus::Exposed
            | BridgePairingStatus::UnsafeOrInaccessible
    ) {
        unavailable_bridge_probe()
    } else {
        crate::captcha::probe_existing_bridge().await?
    };
    // Pairing can be repaired or removed while a ten-second probe is in
    // flight. Report the final filesystem state rather than the preflight
    // snapshot used only to decide whether probing was safe.
    let mut pairing_status = crate::commands::browser_extension::bridge_pairing_status();
    // A responsive probe authenticates the exact runtime build and pairing
    // secret, which clears this marker before we read it.
    let pending_activation = if pairing_status == BridgePairingStatus::Present {
        match crate::commands::browser_extension::pending_activation() {
            Ok(pending) => pending,
            Err(_) => {
                let refreshed = crate::commands::browser_extension::bridge_pairing_status();
                pairing_status = if refreshed == BridgePairingStatus::Present {
                    BridgePairingStatus::UnsafeOrInaccessible
                } else {
                    refreshed
                };
                None
            }
        }
    } else {
        None
    };
    let report = browser_bridge_report(probe, pending_activation, pairing_status);

    if !report.ok {
        let (code, message) = browser_bridge_failure(&report);
        return Err(CliError::Diagnostic {
            code,
            message: message.into(),
            details: serde_json::to_value(&report)?,
        });
    }

    match ctx.fmt {
        OutputFormat::Json => output::json::success(&report),
        OutputFormat::Table => {
            eprintln!(
                "Browser Bridge: OK (runtime {}, authenticated loopback v{}, port {}, {} ms)",
                report.runtime_build,
                report.protocol_version,
                report.port.expect("healthy bridge has a listener port"),
                report.latency_ms
            );
            eprintln!(
                "Probe returned no challenge instruction and did not submit a generation or consume credits."
            );
        }
    }
    Ok(())
}

fn unavailable_bridge_probe() -> BridgeProbe {
    BridgeProbe {
        status: BridgeProbeStatus::Unavailable,
        port: None,
        occupied_ports: Vec::new(),
        bridge_occupied_ports: Vec::new(),
        foreign_occupied_ports: Vec::new(),
        latency: Duration::ZERO,
    }
}

fn browser_bridge_report(
    probe: BridgeProbe,
    pending_activation: Option<PendingActivation>,
    pairing_status: BridgePairingStatus,
) -> BrowserBridgeReport {
    let status = probe.status;
    BrowserBridgeReport {
        ok: status == BridgeProbeStatus::Responsive
            && pairing_status == BridgePairingStatus::Present
            && pending_activation.is_none(),
        status: status.as_str(),
        configured: pairing_status != BridgePairingStatus::Missing,
        responsive: status == BridgeProbeStatus::Responsive,
        busy: status == BridgeProbeStatus::Busy,
        port_conflict: status == BridgeProbeStatus::PortConflict,
        inconclusive: matches!(
            status,
            BridgeProbeStatus::Busy | BridgeProbeStatus::PortConflict
        ),
        runtime_ack_pending: pending_activation.is_some(),
        pending_origin: pending_activation.map(PendingActivation::as_str),
        pairing_status: pairing_status.as_str(),
        transport: "authenticated_loopback",
        protocol_version: PROTOCOL_VERSION,
        runtime_build: BROWSER_BRIDGE_RUNTIME_BUILD,
        port: probe.port,
        occupied_ports: probe.occupied_ports,
        bridge_occupied_ports: probe.bridge_occupied_ports,
        foreign_occupied_ports: probe.foreign_occupied_ports,
        latency_ms: probe.latency.as_millis(),
        challenge_executed: false,
        generation_submitted: false,
        credits_consumed: false,
    }
}

fn browser_bridge_failure(report: &BrowserBridgeReport) -> (&'static str, &'static str) {
    if !report.configured {
        (
            "browser_bridge_not_configured",
            "Sunox Browser Bridge is not installed or paired; run `sunox install-browser-extension`",
        )
    } else if report.pairing_status == BridgePairingStatus::PairingMissing.as_str() {
        (
            "browser_bridge_pairing_missing",
            "the Browser Bridge installation is recorded, but its pairing secret is missing. Run `sunox install-browser-extension --force` once to rebuild the managed pairing, follow the command's `activation_required` guidance, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.pairing_status == BridgePairingStatus::Corrupt.as_str() {
        (
            "browser_bridge_pairing_corrupt",
            "the Browser Bridge pairing secret contains an invalid value. Run `sunox install-browser-extension --force` once to replace the repairable value and synchronize the managed bundle, follow the command's `activation_required` guidance, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.pairing_status == BridgePairingStatus::BundleMissing.as_str() {
        (
            "browser_bridge_bundle_missing",
            "the Browser Bridge pairing is recorded, but its managed extension bundle is missing. Run `sunox install-browser-extension --force` once to restore it, follow the command's `activation_required=ensure_loaded` branches, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.pairing_status == BridgePairingStatus::BundleOutdated.as_str() {
        (
            "browser_bridge_bundle_outdated",
            "the Browser Bridge pairing and managed extension bundle are valid, but the recognized runtime is outdated. Run `sunox install-browser-extension --force` once to update it, follow the command's single `activation_required` decision, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.pairing_status == BridgePairingStatus::BundleCorrupt.as_str() {
        (
            "browser_bridge_bundle_corrupt",
            "the Browser Bridge managed extension bundle is incomplete, modified, or no longer matches its pairing. Run `sunox install-browser-extension --force` once to replace it, follow the command's single `activation_required` decision, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.pairing_status == BridgePairingStatus::BundleUnrecognized.as_str() {
        (
            "browser_bridge_bundle_unrecognized",
            "the reserved Browser Bridge path contains a non-empty bundle whose Sunox ownership cannot be proven. Sunox will not use or overwrite it, and `--force` is intentionally refused. Inspect the per-user configuration path and preserve or remove the entry manually only after identifying it",
        )
    } else if report.pairing_status == BridgePairingStatus::Exposed.as_str() {
        (
            "browser_bridge_pairing_exposed",
            "the Browser Bridge pairing material or its managed directory is more permissive than Sunox's private-file policy. Do not reuse the potentially exposed secret. Run `sunox install-browser-extension --force` once to atomically rotate the pairing and repair the managed files, follow the command's `activation_required` guidance, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.pairing_status == BridgePairingStatus::UnsafeOrInaccessible.as_str() {
        (
            "browser_bridge_pairing_unsafe",
            "the Browser Bridge pairing or managed activation state cannot be read safely. This includes unsafe reserved entry types and a corrupt private pending-activation marker whose intended action cannot be inferred. Sunox will not follow or overwrite such an entry. Inspect the reserved Browser Bridge entries in the per-user Sunox configuration path, then retry `sunox doctor --browser-bridge`; do not assume Reload or `--force` can repair this state",
        )
    } else if report.busy {
        (
            "browser_bridge_busy",
            "the Browser Bridge probe is inconclusive because an authenticated current Sunox listener already owns a Bridge port, typically for an in-flight challenge or another probe; wait for that operation to finish, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.port_conflict {
        (
            "browser_bridge_port_conflict",
            "the Browser Bridge probe is inconclusive because one or more local Bridge ports are owned by a process that could not authenticate as the current Sunox protocol. Stop the conflicting process or configure it away from ports 29764-29771, then retry `sunox doctor --browser-bridge`",
        )
    } else if report.runtime_ack_pending {
        (
            "browser_bridge_runtime_ack_pending",
            "Browser Bridge activation evidence is pending, but the exact runtime and pairing have not authenticated. Run `sunox install-browser-extension --force` once to validate the managed files and pairing, then follow its single `activation_required` decision; `activation_options` are mutually exclusive environment-dependent alternatives, not a sequence. Do not click Reload repeatedly or assume a stale pending marker proves the files were updated",
        )
    } else {
        (
            "browser_bridge_unavailable",
            "the configured Sunox Browser Bridge did not claim the loopback probe within 10 seconds. In chrome://extensions: if its card is missing, choose Load unpacked and select the managed path; if the card is disabled, enable it. Then run `sunox install-browser-extension --force` and follow its single `activation_required` decision; click Reload only when that decision or its matching condition explicitly requires it",
        )
    }
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
    use std::time::Duration;

    use super::{
        NetworkTarget, ProbeStage, browser_bridge_failure, browser_bridge_report, network_usable,
        stage_summary, timed_stage,
    };
    use crate::captcha::{BridgeProbe, BridgeProbeStatus};
    use crate::commands::browser_extension::{BridgePairingStatus, PendingActivation};

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

    #[test]
    fn occupied_bridge_ports_are_reported_as_busy_without_reload_advice() {
        let report = browser_bridge_report(
            BridgeProbe {
                status: BridgeProbeStatus::Busy,
                port: Some(29_765),
                occupied_ports: vec![29_764],
                bridge_occupied_ports: vec![29_764],
                foreign_occupied_ports: Vec::new(),
                latency: Duration::from_secs(10),
            },
            None,
            BridgePairingStatus::Present,
        );
        let (code, message) = browser_bridge_failure(&report);
        let json = serde_json::to_value(&report).expect("busy report JSON");

        assert_eq!(code, "browser_bridge_busy");
        assert!(!message.contains("Reload"));
        assert_eq!(json["status"], "busy");
        assert_eq!(json["busy"], true);
        assert_eq!(json["inconclusive"], true);
        assert_eq!(json["responsive"], false);
        assert_eq!(json["occupied_ports"], serde_json::json!([29_764]));
    }

    #[test]
    fn foreign_bridge_ports_are_reported_as_conflicts_not_active_challenges() {
        let report = browser_bridge_report(
            BridgeProbe {
                status: BridgeProbeStatus::PortConflict,
                port: Some(29_765),
                occupied_ports: vec![29_764],
                bridge_occupied_ports: Vec::new(),
                foreign_occupied_ports: vec![29_764],
                latency: Duration::from_millis(500),
            },
            None,
            BridgePairingStatus::Present,
        );
        let (code, message) = browser_bridge_failure(&report);
        let json = serde_json::to_value(&report).expect("port conflict report JSON");

        assert_eq!(code, "browser_bridge_port_conflict");
        assert!(message.contains("could not authenticate"));
        assert!(!message.contains("active operation"));
        assert_eq!(json["status"], "port_conflict");
        assert_eq!(json["busy"], false);
        assert_eq!(json["port_conflict"], true);
        assert_eq!(json["inconclusive"], true);
        assert_eq!(json["foreign_occupied_ports"], serde_json::json!([29_764]));
    }

    #[test]
    fn unavailable_bridge_distinguishes_missing_disabled_and_reload_conditions() {
        let report = browser_bridge_report(
            BridgeProbe {
                status: BridgeProbeStatus::Unavailable,
                port: Some(29_764),
                occupied_ports: Vec::new(),
                bridge_occupied_ports: Vec::new(),
                foreign_occupied_ports: Vec::new(),
                latency: Duration::from_secs(10),
            },
            None,
            BridgePairingStatus::Present,
        );
        let (code, message) = browser_bridge_failure(&report);

        assert_eq!(code, "browser_bridge_unavailable");
        assert!(message.contains("install-browser-extension --force"));
        assert!(message.contains("card is missing"));
        assert!(message.contains("card is disabled"));
        assert!(message.contains("activation_required"));
        assert!(message.contains("click Reload only"));
    }

    #[test]
    fn pending_runtime_requires_one_managed_recheck_instead_of_blind_reload() {
        let report = browser_bridge_report(
            BridgeProbe {
                status: BridgeProbeStatus::Unavailable,
                port: Some(29_764),
                occupied_ports: Vec::new(),
                bridge_occupied_ports: Vec::new(),
                foreign_occupied_ports: Vec::new(),
                latency: Duration::from_secs(10),
            },
            Some(PendingActivation::Reload),
            BridgePairingStatus::Present,
        );
        let (code, message) = browser_bridge_failure(&report);
        let json = serde_json::to_value(&report).expect("pending report JSON");

        assert_eq!(code, "browser_bridge_runtime_ack_pending");
        assert!(message.contains("install-browser-extension --force"));
        assert!(message.contains("activation_required"));
        assert!(message.contains("Do not click Reload repeatedly"));
        assert_eq!(json["runtime_ack_pending"], true);
        assert_eq!(json["pending_origin"], "reload");
    }

    #[test]
    fn repairable_pairing_failures_take_precedence_over_a_pending_marker() {
        for (pairing_status, expected_code) in [
            (
                BridgePairingStatus::PairingMissing,
                "browser_bridge_pairing_missing",
            ),
            (
                BridgePairingStatus::Corrupt,
                "browser_bridge_pairing_corrupt",
            ),
            (
                BridgePairingStatus::Exposed,
                "browser_bridge_pairing_exposed",
            ),
        ] {
            let report = browser_bridge_report(
                BridgeProbe {
                    status: BridgeProbeStatus::Unavailable,
                    port: None,
                    occupied_ports: Vec::new(),
                    bridge_occupied_ports: Vec::new(),
                    foreign_occupied_ports: Vec::new(),
                    latency: Duration::ZERO,
                },
                Some(PendingActivation::Reload),
                pairing_status,
            );
            let (code, message) = browser_bridge_failure(&report);

            assert_eq!(code, expected_code);
            assert!(message.contains("install-browser-extension --force"));
            assert!(message.contains("activation_required"));
            assert_eq!(report.pairing_status, pairing_status.as_str());
        }
    }

    #[test]
    fn unsafe_pairing_does_not_promise_that_force_or_reload_can_repair_it() {
        let report = browser_bridge_report(
            BridgeProbe {
                status: BridgeProbeStatus::Unavailable,
                port: None,
                occupied_ports: Vec::new(),
                bridge_occupied_ports: Vec::new(),
                foreign_occupied_ports: Vec::new(),
                latency: Duration::ZERO,
            },
            Some(PendingActivation::Reload),
            BridgePairingStatus::UnsafeOrInaccessible,
        );
        let (code, message) = browser_bridge_failure(&report);

        assert_eq!(code, "browser_bridge_pairing_unsafe");
        assert!(message.contains("will not follow or overwrite"));
        assert!(message.contains("do not assume Reload or `--force`"));
    }

    #[test]
    fn final_pairing_state_prevents_a_responsive_probe_from_hiding_secret_loss() {
        let report = browser_bridge_report(
            BridgeProbe {
                status: BridgeProbeStatus::Responsive,
                port: Some(29_764),
                occupied_ports: Vec::new(),
                bridge_occupied_ports: Vec::new(),
                foreign_occupied_ports: Vec::new(),
                latency: Duration::from_millis(20),
            },
            None,
            BridgePairingStatus::PairingMissing,
        );
        let (code, _) = browser_bridge_failure(&report);

        assert!(!report.ok);
        assert_eq!(code, "browser_bridge_pairing_missing");
    }
}

use crate::app::AppContext;
use crate::browser_bridge::{
    INSTALL_BEHAVIOR_GUIDANCE, InstallOutcome, PendingActivation, install_with_probe,
};
use crate::cli::InstallBrowserExtensionArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn install(args: InstallBrowserExtensionArgs, ctx: &AppContext) -> Result<(), CliError> {
    let mut probe_warning = None;
    let report = install_with_probe(args.path, args.force, async {
        if let Err(error) = crate::captcha::probe_existing_bridge().await {
            probe_warning = Some(format!(
                "the current Browser Bridge runtime could not be probed after checking its files: {error}"
            ));
        }
    })
    .await?;
    if let Some(warning) = &probe_warning {
        eprintln!("Warning: {warning}");
    }
    let destination = &report.destination;
    let outcome = report.outcome;
    let runtime_state = report.runtime_state;

    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "installed": true,
            "status": outcome.status(runtime_state),
            "path": destination.display().to_string(),
            "reload_required": runtime_state.reload_required,
            "runtime_ack_pending": runtime_state.runtime_ack_pending,
            "pending_origin": runtime_state.pending_origin.map(PendingActivation::as_str),
            "activation_required": report.activation_required,
            "activation_options": report.activation_options,
            "next_steps": report.next_steps,
        })),
        OutputFormat::Table => {
            match outcome {
                InstallOutcome::Installed => {
                    eprintln!(
                        "Extracted the Sunox Browser Bridge to: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=load_unpacked, activation_required=load_unpacked)",
                        destination.display()
                    );
                    eprintln!(
                        "Open chrome://extensions, enable Developer mode, choose Load unpacked, and select that directory."
                    );
                    eprintln!(
                        "Then run `sunox doctor --browser-bridge` to authenticate the loaded runtime. Do not click Reload before the extension has first been loaded."
                    );
                }
                InstallOutcome::Restored if runtime_state.runtime_ack_pending => {
                    eprintln!(
                        "Restored the Sunox Browser Bridge files at: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=restore, activation_required=ensure_loaded; activation_options=load_unpacked_if_missing|enable_and_reload_if_present)",
                        destination.display()
                    );
                    eprintln!(
                        "In chrome://extensions: if no Sunox Browser Bridge card exists, choose Load unpacked and select that directory; if the card exists, enable it and click Reload once. Then run `sunox doctor --browser-bridge`."
                    );
                }
                InstallOutcome::Restored => {
                    eprintln!(
                        "Restored the Sunox Browser Bridge files at: {} (reload_required=false, runtime_ack_pending=false)",
                        destination.display()
                    );
                    eprintln!("The exact loaded runtime and pairing are already authenticated.");
                }
                InstallOutcome::Updated
                    if runtime_state.pending_origin == Some(PendingActivation::Restore) =>
                {
                    eprintln!(
                        "Updated restored Sunox Browser Bridge files before runtime acknowledgement at: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=restore, activation_required=ensure_loaded; activation_options=load_unpacked_if_missing|enable_and_reload_if_present)",
                        destination.display()
                    );
                    eprintln!(
                        "In chrome://extensions: if no Sunox Browser Bridge card exists, choose Load unpacked and select that directory; if the card exists, enable it and click Reload once because its files changed. Then run `sunox doctor --browser-bridge`."
                    );
                }
                InstallOutcome::Updated
                    if runtime_state.pending_origin == Some(PendingActivation::LoadUnpacked) =>
                {
                    eprintln!(
                        "Updated the Sunox Browser Bridge before its first runtime acknowledgement at: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=load_unpacked, activation_required=ensure_loaded; activation_options=load_unpacked_if_missing|enable_and_reload_if_present)",
                        destination.display()
                    );
                    eprintln!(
                        "In chrome://extensions: if no Sunox Browser Bridge card exists, choose Load unpacked and select that directory; if the card exists, ensure it is enabled and click Reload once because its files changed. Then run `sunox doctor --browser-bridge`."
                    );
                }
                InstallOutcome::Updated if runtime_state.reload_required == Some(true) => {
                    eprintln!(
                        "Updated the Sunox Browser Bridge at: {} (reload_required=true, runtime_ack_pending=true, pending_origin=reload, activation_required=reload)",
                        destination.display()
                    );
                    eprintln!(
                        "Open chrome://extensions and click Reload once on the existing Sunox Browser Bridge, then run `sunox doctor --browser-bridge` to confirm the loaded runtime."
                    );
                }
                InstallOutcome::AlreadyCurrent
                    if runtime_state.pending_origin == Some(PendingActivation::LoadUnpacked) =>
                {
                    eprintln!(
                        "Sunox Browser Bridge files are current at: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=load_unpacked, activation_required=ensure_loaded; activation_options=load_unpacked_if_missing|enable_if_disabled|reload_if_enabled_but_unresponsive)",
                        destination.display()
                    );
                    eprintln!(
                        "Chrome has not authenticated this installation. In chrome://extensions, choose Load unpacked only if no Sunox Browser Bridge card exists. If the card exists, enable it; if it was already enabled and this probe still failed, click Reload once. Then run `sunox doctor --browser-bridge`."
                    );
                }
                InstallOutcome::AlreadyCurrent
                    if runtime_state.pending_origin == Some(PendingActivation::Restore) =>
                {
                    eprintln!(
                        "Sunox Browser Bridge restored files are current at: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=restore, activation_required=ensure_loaded; activation_options=load_unpacked_if_missing|enable_and_reload_if_present)",
                        destination.display()
                    );
                    eprintln!(
                        "In chrome://extensions: if no Sunox Browser Bridge card exists, choose Load unpacked and select that directory; if the card exists, enable it and click Reload once. Then run `sunox doctor --browser-bridge`."
                    );
                }
                InstallOutcome::AlreadyCurrent if runtime_state.runtime_ack_pending => {
                    eprintln!(
                        "Sunox Browser Bridge files are current at: {} (reload_required=unknown, runtime_ack_pending=true, pending_origin=reload, activation_required=ensure_loaded; activation_options=enable_if_disabled|reload_if_enabled_but_unresponsive_or_not_refreshed)",
                        destination.display()
                    );
                    eprintln!(
                        "The loaded runtime has not authenticated yet. Ensure Chrome is running and the extension is enabled. If you have not reloaded it since the last Sunox update, click Reload once; then run `sunox doctor --browser-bridge`."
                    );
                }
                InstallOutcome::AlreadyCurrent => {
                    eprintln!(
                        "Sunox Browser Bridge files are already current at: {} (reload_required=false, runtime_ack_pending=false)",
                        destination.display()
                    );
                    eprintln!("No Chrome reload is required.");
                }
                InstallOutcome::Updated => {
                    eprintln!(
                        "Updated the Sunox Browser Bridge at: {} (reload_required=false, runtime_ack_pending=false)",
                        destination.display()
                    );
                    eprintln!(
                        "The current Browser Bridge runtime already acknowledged this update; no Chrome reload is required."
                    );
                }
            }
            eprintln!("{INSTALL_BEHAVIOR_GUIDANCE}");
        }
    }
    Ok(())
}

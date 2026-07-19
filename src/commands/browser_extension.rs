use std::io::Write;
use std::path::{Path, PathBuf};

use crate::app::AppContext;
use crate::captcha::bridge_contract::{LOOPBACK_PORT_COUNT, LOOPBACK_PORT_START, PROTOCOL_VERSION};
use crate::cli::InstallBrowserExtensionArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

const MANIFEST: &str = include_str!("../../assets/browser-extension/manifest.json");
const SERVICE_WORKER: &str = include_str!("../../assets/browser-extension/service-worker.js");
const LOOPBACK_TRANSPORT: &str =
    include_str!("../../assets/browser-extension/transport-loopback.js");
const BRIDGE: &str = include_str!("../../assets/browser-extension/bridge.js");
const PAGE: &str = include_str!("../../assets/browser-extension/page.js");
const CONFIG_TEMPLATE: &str = include_str!("../../assets/browser-extension/config.js");
const ICON_16: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-16.png");
const ICON_32: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-32.png");
const ICON_48: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-48.png");
const ICON_128: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-128.png");

pub async fn install(args: InstallBrowserExtensionArgs, ctx: &AppContext) -> Result<(), CliError> {
    let config_dir = crate::core::project_config_dir()
        .ok_or_else(|| CliError::Config("could not resolve config directory".into()))?;
    let destination = args
        .path
        .map(PathBuf::from)
        .unwrap_or_else(|| config_dir.join("browser-extension"));
    let updating = destination.exists();
    if updating && !args.force {
        return Err(CliError::Config(format!(
            "{} already exists — pass --force to update it",
            destination.display()
        )));
    }
    if updating && !destination.is_dir() {
        return Err(CliError::Config(format!(
            "{} exists but is not a directory",
            destination.display()
        )));
    }

    let secret = load_or_create_secret(&config_dir)?;
    let parent = destination
        .parent()
        .ok_or_else(|| CliError::Config("extension path has no parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix("sunox-browser-extension-")
        .tempdir_in(parent)?;
    write_asset(staging.path(), "manifest.json", MANIFEST)?;
    write_asset(staging.path(), "service-worker.js", SERVICE_WORKER)?;
    write_asset(staging.path(), "transport-loopback.js", LOOPBACK_TRANSPORT)?;
    write_asset(staging.path(), "bridge.js", BRIDGE)?;
    write_asset(staging.path(), "page.js", PAGE)?;
    std::fs::create_dir(staging.path().join("icons"))?;
    write_binary_asset(staging.path(), "icons/icon-16.png", ICON_16)?;
    write_binary_asset(staging.path(), "icons/icon-32.png", ICON_32)?;
    write_binary_asset(staging.path(), "icons/icon-48.png", ICON_48)?;
    write_binary_asset(staging.path(), "icons/icon-128.png", ICON_128)?;
    write_asset(staging.path(), "config.js", &render_config(&secret))?;

    replace_directory(staging, &destination)?;

    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "installed": true,
            "path": destination.display().to_string(),
            "next_steps": [
                "Open chrome://extensions",
                "Enable Developer mode",
                if updating { "Click Reload on the existing extension" } else { "Choose Load unpacked and select the reported path" },
                "Reload an existing suno.com tab"
            ]
        })),
        OutputFormat::Table => {
            eprintln!(
                "Extracted the Sunox Browser Bridge to: {}",
                destination.display()
            );
            if updating {
                eprintln!(
                    "Open chrome://extensions and click Reload on the existing Sunox Browser Bridge."
                );
            } else {
                eprintln!(
                    "Open chrome://extensions, enable Developer mode, choose Load unpacked, and select that directory."
                );
            }
            eprintln!(
                "Then reload an existing suno.com tab. Auto and existing challenge modes can now use the bridge; auto still falls back to an isolated browser when needed."
            );
        }
    }
    Ok(())
}

fn render_config(secret: &str) -> String {
    CONFIG_TEMPLATE
        .replace(
            "__SUNOX_BRIDGE_PROTOCOL_VERSION__",
            &PROTOCOL_VERSION.to_string(),
        )
        .replace(
            "__SUNOX_BRIDGE_PORT_START__",
            &LOOPBACK_PORT_START.to_string(),
        )
        .replace(
            "__SUNOX_BRIDGE_PORT_COUNT__",
            &LOOPBACK_PORT_COUNT.to_string(),
        )
        .replace("__SUNOX_BRIDGE_SECRET__", secret)
}

fn load_or_create_secret(config_dir: &Path) -> Result<String, CliError> {
    let path = config_dir.join("browser-extension-secret");
    if let Ok(secret) = std::fs::read_to_string(&path) {
        let secret = secret.trim();
        if secret.len() >= 32
            && secret
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Ok(secret.to_string());
        }
    }

    std::fs::create_dir_all(config_dir)?;
    let secret = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(secret.as_bytes())?;
    file.sync_all()?;
    Ok(secret)
}

pub(crate) fn bridge_secret() -> Result<Option<String>, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(None);
    };
    let path = config_dir.join("browser-extension-secret");
    let Ok(secret) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let secret = secret.trim();
    if secret.len() < 32
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(CliError::Config(
            "browser extension secret is corrupt; run `sunox install-browser-extension --force`"
                .into(),
        ));
    }
    Ok(Some(secret.to_string()))
}

fn write_asset(directory: &Path, name: &str, contents: &str) -> Result<(), CliError> {
    write_binary_asset(directory, name, contents.as_bytes())
}

fn write_binary_asset(directory: &Path, name: &str, contents: &[u8]) -> Result<(), CliError> {
    let path = directory.join(name);
    let mut file = std::fs::File::create(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn replace_directory(staging: tempfile::TempDir, destination: &Path) -> Result<(), CliError> {
    let staged_path = staging.keep();
    let backup = destination.with_extension(format!("backup-{}", uuid::Uuid::new_v4()));
    let had_existing = destination.exists();
    if had_existing {
        std::fs::rename(destination, &backup)?;
    }
    if let Err(error) = std::fs::rename(&staged_path, destination) {
        if had_existing {
            let _ = std::fs::rename(&backup, destination);
        }
        let _ = std::fs::remove_dir_all(&staged_path);
        return Err(CliError::Io(error));
    }
    if had_existing {
        std::fs::remove_dir_all(backup)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BRIDGE, CONFIG_TEMPLATE, LOOPBACK_TRANSPORT, MANIFEST, PAGE, SERVICE_WORKER, render_config,
    };

    #[test]
    fn extension_assets_share_the_bridge_contract() {
        assert!(MANIFEST.contains("https://suno.com/*"));
        assert!(MANIFEST.contains("http://127.0.0.1/*"));
        assert!(MANIFEST.contains("\"version\": \"0.1.3\""));
        assert!(MANIFEST.contains("\"alarms\""));
        assert!(MANIFEST.contains("icons/icon-16.png"));
        assert!(MANIFEST.contains("icons/icon-128.png"));
        assert!(SERVICE_WORKER.contains("SUNOX_BRIDGE_TRANSPORTS"));
        assert!(SERVICE_WORKER.contains("transport?.contractVersion !== 1"));
        assert!(SERVICE_WORKER.contains("transport-${transportName}.js"));
        assert!(!SERVICE_WORKER.contains("transport-loopback.js"));
        assert!(SERVICE_WORKER.contains("chrome.alarms"));
        assert!(SERVICE_WORKER.contains("transport.claimChallenge"));
        assert!(SERVICE_WORKER.contains("transport.submitResult"));
        assert!(LOOPBACK_TRANSPORT.contains("sunox-bridge-server-v1"));
        assert!(LOOPBACK_TRANSPORT.contains("contractVersion: 1"));
        assert!(LOOPBACK_TRANSPORT.contains("/v1/challenge/claim"));
        assert!(!LOOPBACK_TRANSPORT.contains("Authorization"));
        assert!(BRIDGE.contains("sunox-wake"));
        assert!(PAGE.contains("hcaptcha.execute"));
        assert!(PAGE.contains("turnstile.execute"));
        assert!(CONFIG_TEMPLATE.contains("schemaVersion: 1"));
        assert!(CONFIG_TEMPLATE.contains("transport: \"loopback\""));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_PROTOCOL_VERSION__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_PORT_START__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_PORT_COUNT__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_SECRET__"));
        assert!(LOOPBACK_TRANSPORT.contains("sunox-bridge-receipt-v1"));
        assert!(BRIDGE.contains("transportReceipt"));
        assert!(!BRIDGE.contains("bridgePort"));
        assert!(!BRIDGE.contains("clientNonce"));
        assert!(!BRIDGE.contains("serverNonce"));
    }

    #[test]
    fn rendered_config_uses_the_rust_bridge_contract() {
        let config = render_config("secret-value");

        assert!(config.contains("protocolVersion: 1"));
        assert!(config.contains("portStart: 29764"));
        assert!(config.contains("portCount: 8"));
        assert!(config.contains("sharedSecret: \"secret-value\""));
        assert!(!config.contains("__SUNOX_BRIDGE_"));
    }
}

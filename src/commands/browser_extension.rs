use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use sha2::{Digest, Sha256};

use crate::app::AppContext;
use crate::captcha::bridge_contract::{
    BROWSER_BRIDGE_RUNTIME_BUILD, LOOPBACK_PORT_COUNT, LOOPBACK_PORT_START, PROTOCOL_VERSION,
};
use crate::cli::InstallBrowserExtensionArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

mod permissions;

const MANIFEST: &str = include_str!("../../assets/browser-extension/manifest.json");
const SERVICE_WORKER: &str = include_str!("../../assets/browser-extension/service-worker.js");
const LOOPBACK_TRANSPORT: &str =
    include_str!("../../assets/browser-extension/transport-loopback.js");
const BRIDGE: &str = include_str!("../../assets/browser-extension/bridge.js");
const PAGE: &str = include_str!("../../assets/browser-extension/page.js");
const SHARED: &str = include_str!("../../assets/browser-extension/shared.js");
const OFFSCREEN_HTML: &str = include_str!("../../assets/browser-extension/offscreen.html");
const OFFSCREEN: &str = include_str!("../../assets/browser-extension/offscreen.js");
const POLL_WORKER: &str = include_str!("../../assets/browser-extension/poll-worker.js");
const CONFIG_TEMPLATE: &str = include_str!("../../assets/browser-extension/config.js");
const ICON_16: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-16.png");
const ICON_32: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-32.png");
const ICON_48: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-48.png");
const ICON_128: &[u8] = include_bytes!("../../assets/browser-extension/icons/icon-128.png");
const MANAGED_SENTINEL: &str = ".sunox-browser-bridge-managed";
const MANAGED_SENTINEL_CONTENT: &str = "sunox-browser-bridge\nschema=1\n";
const INSTALL_LOCK_FILE: &str = "browser-extension-install.lock";
const INSTALLATION_MARKER_FILE: &str = "browser-extension-installed";
const INSTALLATION_MARKER_CONTENT: &str = "sunox-browser-bridge-installed\nschema=1\n";
const RELOAD_PENDING_FILE: &str = "browser-extension-reload-pending";
const RUNTIME_ACK_FILE: &str = "browser-extension-runtime-ack";
const BRIDGE_SECRET_FILE: &str = "browser-extension-secret";
const DEFAULT_EXTENSION_DIRECTORY: &str = "browser-extension";
// Builds extracted from the v0.2.0 development branch briefly used the CLI
// package version as manifest.version_name. Accept only these exact rendered
// manifests while migrating to the stable Bridge runtime identity.
const LEGACY_CURRENT_VERSION_NAMES: &[&str] = &["0.2.0"];
const INSTALL_BEHAVIOR_GUIDANCE: &str = "No Suno tab or browser window is opened. For a required challenge, the bridge creates one credentialless, nonce-bound Suno iframe inside Chrome's invisible offscreen document, stops and replaces the host response with a provider-only challenge document, executes one silent challenge, removes the frame on every terminal path, and fails closed instead of creating a visible or minimized fallback; once paired, auto also fails closed instead of launching an isolated browser.";

#[derive(Clone, Copy)]
enum BundleAssetContents {
    StaticText(&'static str),
    StaticBinary(&'static [u8]),
    RenderedManifest,
    RenderedPrivateConfig,
}

#[derive(Clone, Copy)]
struct BundleAsset {
    path: &'static str,
    contents: BundleAssetContents,
}

const CURRENT_BUNDLE_ASSETS: &[BundleAsset] = &[
    BundleAsset {
        path: "manifest.json",
        contents: BundleAssetContents::RenderedManifest,
    },
    BundleAsset {
        path: "service-worker.js",
        contents: BundleAssetContents::StaticText(SERVICE_WORKER),
    },
    BundleAsset {
        path: "transport-loopback.js",
        contents: BundleAssetContents::StaticText(LOOPBACK_TRANSPORT),
    },
    BundleAsset {
        path: "bridge.js",
        contents: BundleAssetContents::StaticText(BRIDGE),
    },
    BundleAsset {
        path: "page.js",
        contents: BundleAssetContents::StaticText(PAGE),
    },
    BundleAsset {
        path: "shared.js",
        contents: BundleAssetContents::StaticText(SHARED),
    },
    BundleAsset {
        path: "offscreen.html",
        contents: BundleAssetContents::StaticText(OFFSCREEN_HTML),
    },
    BundleAsset {
        path: "offscreen.js",
        contents: BundleAssetContents::StaticText(OFFSCREEN),
    },
    BundleAsset {
        path: "poll-worker.js",
        contents: BundleAssetContents::StaticText(POLL_WORKER),
    },
    BundleAsset {
        path: "config.js",
        contents: BundleAssetContents::RenderedPrivateConfig,
    },
    BundleAsset {
        path: "icons/icon-16.png",
        contents: BundleAssetContents::StaticBinary(ICON_16),
    },
    BundleAsset {
        path: "icons/icon-32.png",
        contents: BundleAssetContents::StaticBinary(ICON_32),
    },
    BundleAsset {
        path: "icons/icon-48.png",
        contents: BundleAssetContents::StaticBinary(ICON_48),
    },
    BundleAsset {
        path: "icons/icon-128.png",
        contents: BundleAssetContents::StaticBinary(ICON_128),
    },
];

// Historical 0.3.4-0.3.6 manifests and digests were published with this
// exact file list. Keep it independent from the current asset registry.
const LEGACY_034_036_FILES: &[&str] = &[
    "manifest.json",
    "service-worker.js",
    "transport-loopback.js",
    "bridge.js",
    "page.js",
    "shared.js",
    "offscreen.html",
    "offscreen.js",
    "poll-worker.js",
    "config.js",
    "icons/icon-16.png",
    "icons/icon-32.png",
    "icons/icon-48.png",
    "icons/icon-128.png",
];
const LEGACY_013_FILES: &[&str] = &[
    "manifest.json",
    "service-worker.js",
    "transport-loopback.js",
    "bridge.js",
    "page.js",
    "config.js",
    "icons/icon-16.png",
    "icons/icon-32.png",
    "icons/icon-48.png",
    "icons/icon-128.png",
];
const LEGACY_CONFIG_TEMPLATE: &str = r#"globalThis.SUNOX_BRIDGE_CONFIG = Object.freeze({
  schemaVersion: 1,
  transport: "loopback",
  loopback: Object.freeze({
    protocolVersion: __SUNOX_BRIDGE_PROTOCOL_VERSION__,
    portStart: __SUNOX_BRIDGE_PORT_START__,
    portCount: __SUNOX_BRIDGE_PORT_COUNT__,
    sharedSecret: "__SUNOX_BRIDGE_SECRET__"
  })
});
"#;
// Official v0.0.21-v0.0.24 extension tree
// ce273fcdd870ca6128810276f3d89f5a7a41d1ac, rendered as extension 0.1.3.
const LEGACY_013_DIGEST: &str = "2f7071bf0a4e2b181307ecc100ed06fb505d6d63eb874d9dfe21ed3ca962c439";
// Official v0.1.0 extension tree 58a86fc3d3b5d448580f2a09b580412767559674,
// rendered as extension 0.3.4 / CLI 0.1.0.
const LEGACY_034_DIGEST: &str = "58722e0edb026d638f20f77a6643670776ed9e2f437336986c6c67bd8191082a";
// Official v0.1.1 extension tree 07c95731528fb6747a05c2d3bac72090838845fb,
// rendered as extension 0.3.5 / CLI 0.1.1.
const LEGACY_035_DIGEST: &str = "0ebcdac8ffb3a851f4a3a11ed47b171cf235bfe6aa9ac6cdac2070e36aaa2c23";
// Pre-release extension tree on feature/fix-bridge-restart-recovery,
// rendered as extension 0.3.6 / CLI 0.1.1.
const LEGACY_036_DIGEST: &str = "8edcb5d071b691c34b1ae5db661ed69a22749ccb9df47da6f0e9635b7aff6945";
// Pre-release v0.2.0 extension tree at runtime 0.3.16, before
// manifest.version_name moved from the CLI version to the runtime identity.
const LEGACY_0316_DIGEST: &str = "f686b5eefe76a23a1373b515f11b994da9d3be3a05661afaec28336e9a0dafef";

#[derive(Clone, Copy)]
struct LegacyLineage {
    extension_version: &'static str,
    cli_version: Option<&'static str>,
    protocol_version: u8,
    runtime_build: Option<&'static str>,
    files: &'static [&'static str],
    digest: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume_serial_number: u32,
    #[cfg(windows)]
    file_index: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectorySnapshot {
    identity: DirectoryIdentity,
    directories: BTreeSet<String>,
    file_digests: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReloadPendingMarker {
    runtime_build: String,
    secret_fingerprint: Option<String>,
    activation: PendingActivation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReloadPendingState {
    Missing,
    Valid(ReloadPendingMarker),
    Exposed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeAckMarker {
    runtime_build: String,
    secret_fingerprint: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingActivation {
    LoadUnpacked,
    Reload,
    Restore,
}

impl PendingActivation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LoadUnpacked => "load_unpacked",
            Self::Reload => "reload",
            Self::Restore => "restore",
        }
    }
}

struct LoadedBridgeSecret {
    value: String,
    persisted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BridgePairingStatus {
    Present,
    Missing,
    PairingMissing,
    Corrupt,
    BundleMissing,
    BundleOutdated,
    BundleCorrupt,
    BundleUnrecognized,
    Exposed,
    UnsafeOrInaccessible,
}

impl BridgePairingStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
            Self::PairingMissing => "pairing_missing",
            Self::Corrupt => "corrupt",
            Self::BundleMissing => "bundle_missing",
            Self::BundleOutdated => "bundle_outdated",
            Self::BundleCorrupt => "bundle_corrupt",
            Self::BundleUnrecognized => "bundle_unrecognized",
            Self::Exposed => "exposed",
            Self::UnsafeOrInaccessible => "unsafe_or_inaccessible",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ManagedBundleOwnership {
    Missing,
    Empty,
    Managed,
    Unrecognized,
}

const LEGACY_LINEAGES: &[LegacyLineage] = &[
    LegacyLineage {
        extension_version: "0.1.3",
        cli_version: None,
        protocol_version: 1,
        runtime_build: None,
        files: LEGACY_013_FILES,
        digest: LEGACY_013_DIGEST,
    },
    LegacyLineage {
        extension_version: "0.3.4",
        cli_version: Some("0.1.0"),
        protocol_version: 2,
        runtime_build: None,
        files: LEGACY_034_036_FILES,
        digest: LEGACY_034_DIGEST,
    },
    LegacyLineage {
        extension_version: "0.3.5",
        cli_version: Some("0.1.1"),
        protocol_version: 2,
        runtime_build: None,
        files: LEGACY_034_036_FILES,
        digest: LEGACY_035_DIGEST,
    },
    LegacyLineage {
        extension_version: "0.3.6",
        cli_version: Some("0.1.1"),
        protocol_version: 3,
        runtime_build: Some("0.3.6"),
        files: LEGACY_034_036_FILES,
        digest: LEGACY_036_DIGEST,
    },
    LegacyLineage {
        extension_version: "0.3.16",
        cli_version: Some("0.2.0"),
        protocol_version: 3,
        runtime_build: Some("0.3.16"),
        files: LEGACY_034_036_FILES,
        digest: LEGACY_0316_DIGEST,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallOutcome {
    Installed,
    Restored,
    Updated,
    AlreadyCurrent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InstallRuntimeState {
    reload_required: Option<bool>,
    runtime_ack_pending: bool,
    pending_origin: Option<PendingActivation>,
}

impl InstallOutcome {
    fn status(self, runtime_state: InstallRuntimeState) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Restored => "restored",
            Self::Updated
                if matches!(
                    runtime_state.pending_origin,
                    Some(PendingActivation::LoadUnpacked | PendingActivation::Restore)
                ) =>
            {
                "activation_pending"
            }
            Self::Updated if runtime_state.reload_required == Some(true) => "reload_pending",
            Self::AlreadyCurrent if runtime_state.runtime_ack_pending => "runtime_ack_pending",
            Self::Updated => "updated",
            Self::AlreadyCurrent => "already_current",
        }
    }

    fn runtime_state(self, pending: Option<PendingActivation>) -> InstallRuntimeState {
        InstallRuntimeState {
            // Only a bundle changed by this invocation proves that Chrome must
            // load new files. A pre-existing marker on an already-current
            // bundle means that the exact runtime has not authenticated yet.
            // Until a live probe succeeds, that state proves neither that
            // Reload is still needed nor that it has already happened.
            reload_required: match pending {
                Some(PendingActivation::Reload) if matches!(self, Self::Updated) => Some(true),
                Some(_) => None,
                None => Some(false),
            },
            runtime_ack_pending: pending.is_some(),
            pending_origin: pending,
        }
    }
}

fn activation_guidance(
    outcome: InstallOutcome,
    runtime_state: InstallRuntimeState,
) -> (Option<&'static str>, Vec<&'static str>) {
    match (outcome, runtime_state.pending_origin) {
        (_, None) => (None, Vec::new()),
        (InstallOutcome::Installed, Some(PendingActivation::LoadUnpacked)) => {
            (Some("load_unpacked"), vec!["load_unpacked"])
        }
        (InstallOutcome::Updated, Some(PendingActivation::LoadUnpacked)) => (
            Some("ensure_loaded"),
            vec!["load_unpacked_if_missing", "enable_and_reload_if_present"],
        ),
        (InstallOutcome::AlreadyCurrent, Some(PendingActivation::LoadUnpacked)) => (
            Some("ensure_loaded"),
            vec![
                "load_unpacked_if_missing",
                "enable_if_disabled",
                "reload_if_enabled_but_unresponsive",
            ],
        ),
        (InstallOutcome::Updated, Some(PendingActivation::Reload)) => {
            (Some("reload"), vec!["reload"])
        }
        (InstallOutcome::AlreadyCurrent, Some(PendingActivation::Reload)) => (
            Some("ensure_loaded"),
            vec![
                "enable_if_disabled",
                "reload_if_enabled_but_unresponsive_or_not_refreshed",
            ],
        ),
        (InstallOutcome::Installed, Some(PendingActivation::Reload)) => {
            (Some("load_unpacked"), vec!["load_unpacked"])
        }
        (InstallOutcome::Restored, Some(_)) => (
            Some("ensure_loaded"),
            vec!["load_unpacked_if_missing", "enable_and_reload_if_present"],
        ),
        (InstallOutcome::Installed, Some(PendingActivation::Restore)) => {
            (Some("load_unpacked"), vec!["load_unpacked"])
        }
        (InstallOutcome::Updated, Some(PendingActivation::Restore))
        | (InstallOutcome::AlreadyCurrent, Some(PendingActivation::Restore)) => (
            Some("ensure_loaded"),
            vec!["load_unpacked_if_missing", "enable_and_reload_if_present"],
        ),
    }
}

struct BrowserExtensionInstallLock {
    file: File,
    _config_dir: File,
    config_dir_identity: DirectoryIdentity,
    config_dir_was_exposed: bool,
}

impl BrowserExtensionInstallLock {
    fn acquire(config_dir: &Path) -> Result<Self, CliError> {
        Self::acquire_with_exposure_policy(config_dir, true)?.ok_or_else(|| {
            CliError::Config(
                "the Browser Bridge configuration directory requires pairing rotation".into(),
            )
        })
    }

    fn acquire_for_ack(config_dir: &Path) -> Result<Option<Self>, CliError> {
        Self::acquire_with_exposure_policy(config_dir, false)
    }

    fn acquire_with_exposure_policy(
        config_dir: &Path,
        repair_exposed_directory: bool,
    ) -> Result<Option<Self>, CliError> {
        std::fs::create_dir_all(config_dir)?;
        let config_dir_handle = permissions::open_locked_directory_without_following_symlink(
            config_dir,
            "Sunox configuration directory",
        )?;
        let config_dir_was_exposed =
            permissions::verify_owned_nonwritable_directory_handle(&config_dir_handle, config_dir)?
                == permissions::PrivateObjectStatus::Exposed;
        if config_dir_was_exposed && !repair_exposed_directory {
            return Ok(None);
        }
        if config_dir_was_exposed {
            permissions::harden_private_directory_handle(&config_dir_handle, config_dir)?;
        }
        let config_dir_identity = directory_identity_from_handle(&config_dir_handle, config_dir)?;
        let path = config_dir.join(INSTALL_LOCK_FILE);
        reject_symlink(&path, "Browser Bridge install lock")?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        configure_no_follow(&mut options);
        let file = options.open(&path).map_err(|error| {
            CliError::Config(format!(
                "could not open Browser Bridge install lock at {}: {error}",
                path.display()
            ))
        })?;
        permissions::harden_private_file_handle(&file, &path)?;
        file.lock_exclusive()?;
        Ok(Some(Self {
            file,
            _config_dir: config_dir_handle,
            config_dir_identity,
            config_dir_was_exposed,
        }))
    }

    fn verify_config_directory(&self, config_dir: &Path) -> Result<(), CliError> {
        if directory_identity(config_dir)? == self.config_dir_identity {
            return Ok(());
        }
        Err(CliError::Config(format!(
            "the Sunox configuration directory was replaced while acquiring the Browser Bridge install lock at {}",
            config_dir.display()
        )))
    }

    fn config_directory_was_exposed(&self) -> bool {
        self.config_dir_was_exposed
    }

    fn config_directory_is_exposed_now(&self, config_dir: &Path) -> Result<bool, CliError> {
        Ok(
            permissions::verify_owned_nonwritable_directory_handle(&self._config_dir, config_dir)?
                == permissions::PrivateObjectStatus::Exposed,
        )
    }
}

impl Drop for BrowserExtensionInstallLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub async fn install(args: InstallBrowserExtensionArgs, ctx: &AppContext) -> Result<(), CliError> {
    let config_dir = crate::core::project_config_dir()
        .ok_or_else(|| CliError::Config("could not resolve config directory".into()))?;
    let destination = resolve_install_destination(args.path, &config_dir)?;
    let outcome = install_bundle(&destination, &config_dir, args.force)?;
    let runtime_state = runtime_state_after_probe(outcome, &config_dir, async {
        if let Err(error) = crate::captcha::probe_existing_bridge().await {
            eprintln!(
                "Warning: the current Browser Bridge runtime could not be probed after checking its files: {error}"
            );
        }
    })
    .await?;
    let (activation_required, activation_options) = activation_guidance(outcome, runtime_state);
    let next_steps = install_next_steps(outcome, runtime_state);

    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "installed": true,
            "status": outcome.status(runtime_state),
            "path": destination.display().to_string(),
            "reload_required": runtime_state.reload_required,
            "runtime_ack_pending": runtime_state.runtime_ack_pending,
            "pending_origin": runtime_state.pending_origin.map(PendingActivation::as_str),
            "activation_required": activation_required,
            "activation_options": activation_options,
            "next_steps": next_steps,
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

async fn runtime_state_after_probe(
    outcome: InstallOutcome,
    config_dir: &Path,
    probe: impl std::future::Future<Output = ()>,
) -> Result<InstallRuntimeState, CliError> {
    let mut pending = reload_pending_at(config_dir)?.map(|marker| marker.activation);
    if matches!(
        outcome,
        InstallOutcome::AlreadyCurrent | InstallOutcome::Restored
    ) && pending.is_some()
    {
        probe.await;
        // A successful exact runtime+secret probe clears the marker. Any
        // other result leaves it intact and therefore remains explicitly
        // unknown instead of being mistaken for another required Reload.
        pending = reload_pending_at(config_dir)?.map(|marker| marker.activation);
    }
    Ok(outcome.runtime_state(pending))
}

fn resolve_install_destination(
    explicit_destination: Option<String>,
    config_dir: &Path,
) -> Result<PathBuf, CliError> {
    let Some(destination) = explicit_destination.map(PathBuf::from) else {
        return Ok(config_dir.join(DEFAULT_EXTENSION_DIRECTORY));
    };
    let (resolved_destination, resolved_config_dir) =
        resolve_and_validate_install_paths(&destination, config_dir)?;
    if resolved_destination == resolved_config_dir.join(DEFAULT_EXTENSION_DIRECTORY) {
        return Ok(destination);
    }
    Err(CliError::Config(
        "custom Browser Bridge destinations are no longer supported because Sunox cannot guarantee that an arbitrary parent directory is protected against local replacement; omit --path to use the protected default location".into(),
    ))
}

fn install_next_steps(
    outcome: InstallOutcome,
    runtime_state: InstallRuntimeState,
) -> Vec<&'static str> {
    match outcome {
        InstallOutcome::Installed => vec![
            "Open chrome://extensions",
            "Enable Developer mode",
            "Choose Load unpacked and select the reported path",
            "Run `sunox doctor --browser-bridge` to authenticate the loaded runtime",
            "No Suno tab or browser window is created",
        ],
        InstallOutcome::Restored if runtime_state.runtime_ack_pending => vec![
            "Open chrome://extensions",
            "If no Sunox Browser Bridge card exists, enable Developer mode, choose Load unpacked, and select the reported path",
            "If the card exists, enable it and click Reload once",
            "Run `sunox doctor --browser-bridge` to authenticate the loaded runtime",
            "No Suno tab or browser window is created",
        ],
        InstallOutcome::Restored => vec![
            "No Chrome reload is required",
            "Keep the extension enabled",
            "No Suno tab or browser window is created",
        ],
        InstallOutcome::Updated
            if runtime_state.pending_origin == Some(PendingActivation::Restore) =>
        {
            vec![
                "Open chrome://extensions",
                "If no Sunox Browser Bridge card exists, enable Developer mode, choose Load unpacked, and select the reported path",
                "If the card exists, enable it and click Reload once because its files changed",
                "Run `sunox doctor --browser-bridge` to authenticate the loaded runtime",
                "No Suno tab or browser window is created",
            ]
        }
        InstallOutcome::Updated
            if runtime_state.pending_origin == Some(PendingActivation::LoadUnpacked) =>
        {
            vec![
                "Open chrome://extensions",
                "If no Sunox Browser Bridge card exists, choose Load unpacked and select the reported path",
                "If the card exists, enable it and click Reload once because its files changed",
                "Run `sunox doctor --browser-bridge` to confirm the loaded runtime",
                "No Suno tab or browser window is created",
            ]
        }
        InstallOutcome::Updated if runtime_state.reload_required == Some(true) => vec![
            "Open chrome://extensions",
            "Click Reload once on the existing extension",
            "Run `sunox doctor --browser-bridge` to confirm the loaded runtime",
            "No Suno tab or browser window is created",
        ],
        InstallOutcome::AlreadyCurrent
            if runtime_state.pending_origin == Some(PendingActivation::LoadUnpacked) =>
        {
            vec![
                "Open chrome://extensions",
                "If no Sunox Browser Bridge card exists, enable Developer mode, choose Load unpacked, and select the reported path",
                "If the card exists, enable it; if it was already enabled and the probe still failed, click Reload once",
                "Run `sunox doctor --browser-bridge` to authenticate the loaded runtime",
                "No Suno tab or browser window is created",
            ]
        }
        InstallOutcome::AlreadyCurrent
            if runtime_state.pending_origin == Some(PendingActivation::Restore) =>
        {
            vec![
                "Open chrome://extensions",
                "If no Sunox Browser Bridge card exists, enable Developer mode, choose Load unpacked, and select the reported path",
                "If the card exists, enable it and click Reload once",
                "Run `sunox doctor --browser-bridge` to authenticate the loaded runtime",
                "No Suno tab or browser window is created",
            ]
        }
        InstallOutcome::AlreadyCurrent if runtime_state.runtime_ack_pending => vec![
            "Ensure Chrome is running and the extension is enabled",
            "If it has not been reloaded since the last Sunox update, click Reload once",
            "Run `sunox doctor --browser-bridge` to confirm the loaded runtime",
            "No Suno tab or browser window is created",
        ],
        InstallOutcome::AlreadyCurrent => vec![
            "No Chrome reload is required",
            "Keep the extension enabled",
            "No Suno tab or browser window is created",
        ],
        InstallOutcome::Updated => vec![
            "The current Browser Bridge runtime already acknowledged this update",
            "No Chrome reload is required",
            "No Suno tab or browser window is created",
        ],
    }
}

fn install_bundle(
    destination: &Path,
    config_dir: &Path,
    force: bool,
) -> Result<InstallOutcome, CliError> {
    reject_config_directory_symlink(config_dir)?;
    reject_destination_symlink(destination)?;
    let (_, locked_config_dir) = resolve_and_validate_install_paths(destination, config_dir)?;
    let _lock = BrowserExtensionInstallLock::acquire(&locked_config_dir)?;
    // Resolve again after acquiring the lock. This closes the window where a
    // path component could be replaced with a symlink while this process was
    // waiting for another installer.
    reject_config_directory_symlink(config_dir)?;
    reject_destination_symlink(destination)?;
    let (resolved_destination, resolved_config_dir) =
        resolve_and_validate_install_paths(destination, config_dir)?;
    if resolved_config_dir != locked_config_dir {
        return Err(CliError::Config(format!(
            "the Sunox configuration path changed while waiting for the Browser Bridge install lock ({} -> {})",
            locked_config_dir.display(),
            resolved_config_dir.display()
        )));
    }
    _lock.verify_config_directory(&resolved_config_dir)?;
    let destination = resolved_destination.as_path();
    let prior_pending = reload_pending_at(&resolved_config_dir)?;
    let _ = runtime_ack_at(&resolved_config_dir)?;
    let _ = installation_marker_recorded_at(&resolved_config_dir)?;
    let bridge_artifacts_recorded = bridge_artifact_entry_exists_at(&resolved_config_dir)?;
    let pairing_status = if _lock.config_directory_was_exposed() && bridge_artifacts_recorded {
        BridgePairingStatus::Exposed
    } else {
        bridge_pairing_status_at(&resolved_config_dir)
    };
    let installation_recorded = bridge_artifacts_recorded;

    let existed = destination.exists();
    let mut had_existing_bundle = false;
    if existed {
        let metadata = std::fs::symlink_metadata(destination)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(CliError::Config(format!(
                "{} exists but is not a regular directory",
                destination.display()
            )));
        }
        let destination_is_empty = directory_is_empty(destination)?;
        if !force && !destination_is_empty {
            return Err(CliError::Config(format!(
                "{} already exists — pass --force to update it",
                destination.display()
            )));
        }
        if !destination_is_empty {
            had_existing_bundle = true;
            if !is_managed_bundle(destination)? {
                return Err(CliError::Config(format!(
                    "{} is not a Sunox-managed Browser Bridge directory; refusing to replace a non-empty directory",
                    destination.display()
                )));
            }
            harden_bundle_permissions(destination)?;
        }
    }
    let existing_snapshot = if existed {
        Some(capture_stable_directory_snapshot(destination)?)
    } else {
        None
    };

    let loaded_secret = load_or_prepare_secret(
        &resolved_config_dir,
        pairing_status == BridgePairingStatus::Exposed,
    )?;
    let parent = destination
        .parent()
        .ok_or_else(|| CliError::Config("extension path has no parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix("sunox-browser-extension-")
        .tempdir_in(parent)?;
    permissions::harden_private_directory(staging.path())?;
    write_bundle(staging.path(), &loaded_secret.value)?;

    if existed && bundles_equal(staging.path(), destination)? {
        let sentinel = destination.join(MANAGED_SENTINEL);
        if !sentinel.exists() {
            atomic_write_private_file(
                &sentinel,
                MANAGED_SENTINEL_CONTENT.as_bytes(),
                "Browser Bridge managed marker",
            )?;
        }
        harden_bundle_permissions(destination)?;
        let current_secret_fingerprint = secret_fingerprint(&loaded_secret.value);
        if let Some(pending) = reload_pending_at(&resolved_config_dir)? {
            #[cfg(windows)]
            permissions::harden_private_file(&resolved_config_dir.join(RELOAD_PENDING_FILE))?;
            if pending.runtime_build != BROWSER_BRIDGE_RUNTIME_BUILD
                || pending.secret_fingerprint.as_deref()
                    != Some(current_secret_fingerprint.as_str())
            {
                mark_runtime_ack_pending_locked(
                    &resolved_config_dir,
                    BROWSER_BRIDGE_RUNTIME_BUILD,
                    &loaded_secret.value,
                    pending.activation,
                )?;
            }
        } else if !runtime_ack_matches_at(
            &resolved_config_dir,
            BROWSER_BRIDGE_RUNTIME_BUILD,
            &loaded_secret.value,
        )? {
            // Files without either pending evidence or an exact durable
            // acknowledgement are not proof that Chrome ever loaded and
            // authenticated this bundle. Re-establish a conservative state so
            // the caller probes the live runtime before reporting readiness.
            mark_runtime_ack_pending_locked(
                &resolved_config_dir,
                BROWSER_BRIDGE_RUNTIME_BUILD,
                &loaded_secret.value,
                PendingActivation::Restore,
            )?;
        }
        record_installation_locked(&resolved_config_dir)?;
        return Ok(InstallOutcome::AlreadyCurrent);
    }

    if had_existing_bundle {
        let pending_activation = prior_pending
            .as_ref()
            .map(|marker| marker.activation)
            .unwrap_or(PendingActivation::Reload);
        let (install_secret, persist_after_swap) = if loaded_secret.persisted {
            (new_bridge_secret(), true)
        } else {
            (loaded_secret.value, true)
        };
        write_private_asset(
            staging.path(),
            "config.js",
            render_config(&install_secret).as_bytes(),
        )?;
        replace_updated_bundle(
            staging,
            destination,
            &resolved_config_dir,
            existing_snapshot
                .as_ref()
                .expect("existing managed bundle has a captured snapshot"),
            &install_secret,
            persist_after_swap,
            pending_activation,
        )?;
    } else {
        // The bundle becomes usable only after Chrome loads this exact build
        // with this pairing secret. Commit that activation expectation as part
        // of the directory swap so a marker failure cannot leave a successful
        // installation that falsely reports an acknowledged runtime.
        let pending_activation = if installation_recorded {
            PendingActivation::Restore
        } else {
            PendingActivation::LoadUnpacked
        };
        let (install_secret, persist_after_swap) =
            if installation_recorded && loaded_secret.persisted {
                // Restoring a missing bundle must not reuse an old positive
                // acknowledgement: the Chrome card may have disappeared with the
                // directory. Rotate the pairing so the restored files require a
                // fresh authenticated runtime.
                (new_bridge_secret(), true)
            } else {
                let persist_after_swap = !loaded_secret.persisted;
                (loaded_secret.value, persist_after_swap)
            };
        write_private_asset(
            staging.path(),
            "config.js",
            render_config(&install_secret).as_bytes(),
        )?;
        // Persist pending evidence before the staged directory becomes live.
        // A crash or failed swap may leave a conservative stale marker, but
        // can never leave unauthenticated files that look acknowledged.
        mark_runtime_ack_pending_locked(
            &resolved_config_dir,
            BROWSER_BRIDGE_RUNTIME_BUILD,
            &install_secret,
            pending_activation,
        )?;
        replace_directory(staging, destination, existing_snapshot.as_ref(), || {
            if persist_after_swap {
                persist_rotated_secret(&resolved_config_dir, &install_secret)
            } else {
                Ok(())
            }
        })?;
    }
    harden_bundle_permissions(destination)?;
    record_installation_locked(&resolved_config_dir)?;
    Ok(if had_existing_bundle {
        InstallOutcome::Updated
    } else if installation_recorded {
        InstallOutcome::Restored
    } else {
        InstallOutcome::Installed
    })
}

fn reject_config_directory_symlink(config_dir: &Path) -> Result<(), CliError> {
    match std::fs::symlink_metadata(config_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CliError::Config(format!(
            "the Sunox configuration directory {} is a symbolic link and Browser Bridge installation will not follow it",
            config_dir.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn reject_destination_symlink(destination: &Path) -> Result<(), CliError> {
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CliError::Config(format!(
            "{} exists but is not a regular directory",
            destination.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn resolve_and_validate_install_paths(
    destination: &Path,
    config_dir: &Path,
) -> Result<(PathBuf, PathBuf), CliError> {
    let resolved_destination = resolve_path_with_missing_tail(destination)?;
    let resolved_config_dir = resolve_path_with_missing_tail(config_dir)?;
    if resolved_config_dir.starts_with(&resolved_destination) {
        return Err(CliError::Config(format!(
            "{} contains the Sunox configuration directory and cannot be used as the Browser Bridge destination",
            destination.display()
        )));
    }
    let default_destination = resolved_config_dir.join(DEFAULT_EXTENSION_DIRECTORY);
    if resolved_destination.starts_with(&resolved_config_dir)
        && resolved_destination != default_destination
    {
        return Err(CliError::Config(format!(
            "{} overlaps reserved Sunox configuration metadata; inside {} only the default Browser Bridge directory {} is allowed",
            destination.display(),
            config_dir.display(),
            default_destination.display()
        )));
    }
    Ok((resolved_destination, resolved_config_dir))
}

fn resolve_path_with_missing_tail(path: &Path) -> Result<PathBuf, CliError> {
    // Canonicalize the original path before peeling missing components. Lexically
    // collapsing `..` first is incorrect when a preceding component is a
    // symlink: `link/../child` is resolved relative to the link target by the
    // filesystem, not relative to the directory containing `link`.
    let mut existing = std::path::absolute(path)?;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(component) = existing.file_name().map(ToOwned::to_owned) else {
                    return Err(CliError::Config(format!(
                        "could not resolve path {}: {error}",
                        path.display()
                    )));
                };
                missing.push(component);
                existing.pop();
            }
            Err(error) => {
                return Err(CliError::Config(format!(
                    "could not resolve path {}: {error}",
                    path.display()
                )));
            }
        }
    }
}

fn write_bundle(directory: &Path, secret: &str) -> Result<(), CliError> {
    for asset in CURRENT_BUNDLE_ASSETS {
        let path = directory.join(asset.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match asset.contents {
            BundleAssetContents::StaticText(contents) => {
                write_asset(directory, asset.path, contents)?;
            }
            BundleAssetContents::StaticBinary(contents) => {
                write_binary_asset(directory, asset.path, contents)?;
            }
            BundleAssetContents::RenderedManifest => {
                write_asset(directory, asset.path, &render_manifest())?;
            }
            BundleAssetContents::RenderedPrivateConfig => {
                write_private_asset(directory, asset.path, render_config(secret).as_bytes())?;
            }
        }
    }
    write_asset(directory, MANAGED_SENTINEL, MANAGED_SENTINEL_CONTENT)?;
    Ok(())
}

fn harden_bundle_permissions(directory: &Path) -> Result<(), CliError> {
    permissions::harden_private_directory(directory)?;
    let config = directory.join("config.js");
    reject_symlink(&config, "Browser Bridge rendered configuration")?;
    permissions::harden_private_file(&config)
}

fn directory_is_empty(directory: &Path) -> Result<bool, CliError> {
    Ok(std::fs::read_dir(directory)?.next().is_none())
}

fn is_managed_bundle(directory: &Path) -> Result<bool, CliError> {
    let entries = bundle_file_names(directory)?;
    let sentinel = directory.join(MANAGED_SENTINEL);
    match read_file_without_following_symlink(&sentinel, "Browser Bridge managed marker")? {
        Some(contents) if contents == MANAGED_SENTINEL_CONTENT => {
            let mut allowed = current_bundle_file_set();
            allowed.insert(MANAGED_SENTINEL.to_string());
            return Ok(entries.iter().all(|entry| allowed.contains(entry)));
        }
        Some(_) => return Ok(false),
        None => {}
    }

    is_recognizable_legacy_bundle(directory, &entries)
}

fn is_recognizable_legacy_bundle(
    directory: &Path,
    entries: &BTreeSet<String>,
) -> Result<bool, CliError> {
    let manifest_path = directory.join("manifest.json");
    let manifest_text =
        match read_file_without_following_symlink(&manifest_path, "Browser Bridge manifest")? {
            Some(manifest) => manifest,
            None => return Ok(false),
        };
    let Ok(manifest): Result<serde_json::Value, _> = serde_json::from_str(&manifest_text) else {
        return Ok(false);
    };
    if manifest["manifest_version"] != 3
        || manifest["name"] != "Sunox Browser Bridge"
        || manifest["background"]["service_worker"] != "service-worker.js"
    {
        return Ok(false);
    }

    let extension_version = manifest["version"].as_str();
    let cli_version = manifest["version_name"].as_str();

    if manifest_bytes_match_current_runtime(manifest_text.as_bytes()) {
        return current_bundle_without_sentinel_matches(directory, entries);
    }

    let Some(lineage) = find_legacy_lineage(extension_version, cli_version) else {
        return Ok(false);
    };
    if entries != &bundle_file_set(lineage.files) {
        return Ok(false);
    }
    let digest = immutable_bundle_digest(directory, lineage.files)?;
    let config_matches = rendered_config_file_matches(
        &directory.join("config.js"),
        if lineage.runtime_build.is_some() {
            CONFIG_TEMPLATE
        } else {
            LEGACY_CONFIG_TEMPLATE
        },
        lineage.protocol_version,
        lineage.runtime_build,
    )?;
    Ok(legacy_lineage_facts_match(
        lineage,
        entries,
        &digest,
        config_matches,
    ))
}

fn find_legacy_lineage(
    extension_version: Option<&str>,
    cli_version: Option<&str>,
) -> Option<&'static LegacyLineage> {
    LEGACY_LINEAGES.iter().find(|lineage| {
        extension_version == Some(lineage.extension_version) && cli_version == lineage.cli_version
    })
}

fn legacy_lineage_facts_match(
    lineage: &LegacyLineage,
    entries: &BTreeSet<String>,
    digest: &str,
    config_matches: bool,
) -> bool {
    entries == &bundle_file_set(lineage.files) && digest == lineage.digest && config_matches
}

fn current_bundle_without_sentinel_matches(
    directory: &Path,
    entries: &BTreeSet<String>,
) -> Result<bool, CliError> {
    if entries != &current_bundle_file_set() {
        return Ok(false);
    }
    for asset in CURRENT_BUNDLE_ASSETS {
        match asset.contents {
            BundleAssetContents::RenderedPrivateConfig => {
                if !rendered_config_file_matches(
                    &directory.join(asset.path),
                    CONFIG_TEMPLATE,
                    PROTOCOL_VERSION,
                    Some(BROWSER_BRIDGE_RUNTIME_BUILD),
                )? {
                    return Ok(false);
                }
            }
            BundleAssetContents::StaticText(expected) => {
                if std::fs::read(directory.join(asset.path))? != expected.as_bytes() {
                    return Ok(false);
                }
            }
            BundleAssetContents::StaticBinary(expected) => {
                if std::fs::read(directory.join(asset.path))? != expected {
                    return Ok(false);
                }
            }
            BundleAssetContents::RenderedManifest => {
                if !manifest_bytes_match_current_runtime(&std::fs::read(
                    directory.join(asset.path),
                )?) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn immutable_bundle_digest(directory: &Path, files: &[&str]) -> Result<String, CliError> {
    let mut digest = Sha256::new();
    for name in files {
        if *name == "config.js" {
            continue;
        }
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(std::fs::read(directory.join(name))?);
        digest.update([0]);
    }
    Ok(encode_digest(digest.finalize()))
}

fn encode_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn bundle_file_set(files: &[&str]) -> BTreeSet<String> {
    files.iter().map(|name| (*name).to_string()).collect()
}

fn current_bundle_file_set() -> BTreeSet<String> {
    CURRENT_BUNDLE_ASSETS
        .iter()
        .map(|asset| asset.path.to_string())
        .collect()
}

fn rendered_config_file_matches(
    path: &Path,
    template: &str,
    protocol_version: u8,
    runtime_build: Option<&str>,
) -> Result<bool, CliError> {
    let contents =
        match read_file_without_following_symlink(path, "Browser Bridge rendered configuration")? {
            Some(contents) => contents,
            None => return Ok(false),
        };
    let mut expected = template
        .replace(
            "__SUNOX_BRIDGE_PROTOCOL_VERSION__",
            &protocol_version.to_string(),
        )
        .replace(
            "__SUNOX_BRIDGE_PORT_START__",
            &LOOPBACK_PORT_START.to_string(),
        )
        .replace(
            "__SUNOX_BRIDGE_PORT_COUNT__",
            &LOOPBACK_PORT_COUNT.to_string(),
        );
    if let Some(runtime_build) = runtime_build {
        expected = expected.replace("__SUNOX_BRIDGE_RUNTIME_BUILD__", runtime_build);
    }
    let Some((prefix, suffix)) = expected.split_once("__SUNOX_BRIDGE_SECRET__") else {
        return Ok(false);
    };
    let Some(secret) = contents
        .strip_prefix(prefix)
        .and_then(|contents| contents.strip_suffix(suffix))
    else {
        return Ok(false);
    };
    Ok(valid_secret(secret))
}

fn bundle_file_names(directory: &Path) -> Result<BTreeSet<String>, CliError> {
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    collect_bundle_file_names(directory, directory, &mut files, &mut directories)?;
    Ok(files)
}

fn collect_bundle_file_names(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
    directories: &mut BTreeSet<String>,
) -> Result<(), CliError> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(CliError::Config(format!(
                "{} contains a symbolic link and cannot be managed safely",
                root.display()
            )));
        }
        if metadata.is_dir() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| CliError::Config("extension asset escaped its root directory".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            if relative != "icons" {
                return Err(CliError::Config(format!(
                    "{} contains unsupported directory {relative}; refusing to replace it",
                    root.display()
                )));
            }
            directories.insert(relative);
            collect_bundle_file_names(root, &path, files, directories)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(CliError::Config(format!(
                "{} contains an unsupported filesystem entry",
                root.display()
            )));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CliError::Config("extension asset escaped its root directory".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        files.insert(relative);
    }
    Ok(())
}

fn capture_stable_directory_snapshot(directory: &Path) -> Result<DirectorySnapshot, CliError> {
    let first = directory_snapshot(directory)?;
    let second = directory_snapshot(directory)?;
    if first != second {
        return Err(CliError::Config(format!(
            "{} changed while the Browser Bridge installer was validating it; no files were replaced",
            directory.display()
        )));
    }
    Ok(second)
}

fn directory_snapshot(directory: &Path) -> Result<DirectorySnapshot, CliError> {
    let identity = directory_identity(directory)?;
    let mut files = BTreeSet::new();
    let mut directories = BTreeSet::new();
    collect_bundle_file_names(directory, directory, &mut files, &mut directories)?;
    let mut file_digests = BTreeMap::new();
    for name in files {
        let contents = read_bytes_without_following_symlink(
            &directory.join(&name),
            "Browser Bridge bundle asset",
        )?
        .ok_or_else(|| {
            CliError::Config(format!(
                "{} changed while the Browser Bridge installer was validating it",
                directory.display()
            ))
        })?;
        file_digests.insert(name, encode_digest(Sha256::digest(contents)));
    }
    if directory_identity(directory)? != identity {
        return Err(CliError::Config(format!(
            "{} changed while the Browser Bridge installer was validating it",
            directory.display()
        )));
    }
    Ok(DirectorySnapshot {
        identity,
        directories,
        file_digests,
    })
}

fn directory_matches_snapshot(
    directory: &Path,
    expected: &DirectorySnapshot,
) -> Result<bool, CliError> {
    match directory_snapshot(directory) {
        Ok(actual) => Ok(&actual == expected),
        Err(CliError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn directory_identity(directory: &Path) -> Result<DirectoryIdentity, CliError> {
    let metadata = std::fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CliError::Config(format!(
            "{} is not a regular directory",
            directory.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(DirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };

        let mut wide_path = directory.as_os_str().encode_wide().collect::<Vec<_>>();
        wide_path.push(0);
        // FILE_FLAG_BACKUP_SEMANTICS is required for directory handles. Opening
        // the reparse point itself keeps identity checks from following a path
        // that was replaced after symlink_metadata returned.
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(CliError::Io(std::io::Error::last_os_error()));
        }
        let identity = windows_directory_identity_from_raw_handle(handle, directory);
        unsafe {
            CloseHandle(handle);
        }
        identity
    }
}

fn directory_identity_from_handle(
    directory: &File,
    path: &Path,
) -> Result<DirectoryIdentity, CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = directory.metadata()?;
        if !metadata.is_dir() {
            return Err(CliError::Config(format!(
                "{} is not a regular directory",
                path.display()
            )));
        }
        return Ok(DirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        });
    }
    #[cfg(windows)]
    {
        return windows_directory_identity_from_handle(directory, path);
    }
    #[allow(unreachable_code)]
    Err(CliError::Config(format!(
        "directory identity is unsupported for {} on this platform",
        path.display()
    )))
}

#[cfg(windows)]
fn windows_directory_identity_from_handle(
    directory: &File,
    path: &Path,
) -> Result<DirectoryIdentity, CliError> {
    use std::os::windows::io::AsRawHandle;

    windows_directory_identity_from_raw_handle(directory.as_raw_handle(), path)
}

#[cfg(windows)]
fn windows_directory_identity_from_raw_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    path: &Path,
) -> Result<DirectoryIdentity, CliError> {
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let succeeded = unsafe { GetFileInformationByHandle(handle, &mut information) };
    if succeeded == 0 {
        return Err(CliError::Io(std::io::Error::last_os_error()));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(CliError::Config(format!(
            "{} is not a regular directory",
            path.display()
        )));
    }
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok(DirectoryIdentity {
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index,
    })
}

fn bundles_equal(expected: &Path, actual: &Path) -> Result<bool, CliError> {
    let expected_names = bundle_file_names(expected)?;
    let mut actual_names = bundle_file_names(actual)?;
    actual_names.remove(MANAGED_SENTINEL);
    let mut expected_without_sentinel = expected_names;
    expected_without_sentinel.remove(MANAGED_SENTINEL);
    if actual_names != expected_without_sentinel {
        return Ok(false);
    }
    for name in actual_names {
        let expected_contents = std::fs::read(expected.join(&name))?;
        let actual_contents = std::fs::read(actual.join(&name))?;
        if name == "manifest.json"
            && expected_contents == render_manifest().as_bytes()
            && manifest_bytes_match_current_runtime(&actual_contents)
        {
            continue;
        }
        if expected_contents != actual_contents {
            return Ok(false);
        }
    }
    Ok(true)
}

fn manifest_bytes_match_current_runtime(contents: &[u8]) -> bool {
    contents == render_manifest().as_bytes()
        || LEGACY_CURRENT_VERSION_NAMES.iter().any(|version_name| {
            contents == render_manifest_with_version_name(version_name).as_bytes()
        })
}

fn render_config(secret: &str) -> String {
    render_config_template(
        CONFIG_TEMPLATE,
        PROTOCOL_VERSION,
        Some(BROWSER_BRIDGE_RUNTIME_BUILD),
        secret,
    )
}

fn render_lineage_config(lineage: &LegacyLineage, secret: &str) -> String {
    render_config_template(
        if lineage.runtime_build.is_some() {
            CONFIG_TEMPLATE
        } else {
            LEGACY_CONFIG_TEMPLATE
        },
        lineage.protocol_version,
        lineage.runtime_build,
        secret,
    )
}

fn render_config_template(
    template: &str,
    protocol_version: u8,
    runtime_build: Option<&str>,
    secret: &str,
) -> String {
    let mut rendered = template
        .replace(
            "__SUNOX_BRIDGE_PROTOCOL_VERSION__",
            &protocol_version.to_string(),
        )
        .replace(
            "__SUNOX_BRIDGE_PORT_START__",
            &LOOPBACK_PORT_START.to_string(),
        )
        .replace(
            "__SUNOX_BRIDGE_PORT_COUNT__",
            &LOOPBACK_PORT_COUNT.to_string(),
        )
        .replace("__SUNOX_BRIDGE_SECRET__", secret);
    if let Some(runtime_build) = runtime_build {
        rendered = rendered.replace("__SUNOX_BRIDGE_RUNTIME_BUILD__", runtime_build);
    }
    rendered
}

fn render_manifest() -> String {
    render_manifest_with_version_name(BROWSER_BRIDGE_RUNTIME_BUILD)
}

fn render_manifest_with_version_name(version_name: &str) -> String {
    MANIFEST
        .replace(
            "__SUNOX_BRIDGE_RUNTIME_BUILD__",
            BROWSER_BRIDGE_RUNTIME_BUILD,
        )
        .replacen(
            &format!("\"version_name\": \"{BROWSER_BRIDGE_RUNTIME_BUILD}\""),
            &format!("\"version_name\": \"{version_name}\""),
            1,
        )
}

fn load_or_prepare_secret(
    config_dir: &Path,
    rotate_exposed_secret: bool,
) -> Result<LoadedBridgeSecret, CliError> {
    std::fs::create_dir_all(config_dir)?;
    let path = config_dir.join(BRIDGE_SECRET_FILE);
    if !rotate_exposed_secret {
        match read_private_file_without_following_symlink(&path, "browser extension secret")? {
            PrivateFileRead::Private(secret) if valid_secret(secret.trim()) => {
                return Ok(LoadedBridgeSecret {
                    value: secret.trim().to_string(),
                    persisted: true,
                });
            }
            PrivateFileRead::Missing | PrivateFileRead::Private(_) | PrivateFileRead::Exposed => {}
        }
    }

    let secret = new_bridge_secret();
    Ok(LoadedBridgeSecret {
        value: secret,
        persisted: false,
    })
}

fn new_bridge_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn secret_fingerprint(secret: &str) -> String {
    encode_digest(Sha256::digest(secret.as_bytes()))
}

fn atomic_write_private_file(
    path: &Path,
    contents: &[u8],
    description: &str,
) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::Config(format!("{description} has no parent directory")))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(windows)]
    permissions::harden_private_directory(parent)?;
    reject_symlink(path, description)?;

    let mut temporary = tempfile::Builder::new()
        .prefix(".sunox-private-")
        .tempfile_in(parent)?;
    permissions::harden_private_file_handle(temporary.as_file(), temporary.path())?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;

    // A rename replaces the directory entry rather than following it, but
    // explicitly reject a symlink so a corrupt or hostile config is visible
    // instead of silently repaired.
    reject_symlink(path, description)?;
    let persisted = temporary
        .persist(path)
        .map_err(|error| CliError::Io(error.error))?;
    permissions::harden_private_file_handle(&persisted, path)?;

    #[cfg(unix)]
    File::open(parent)?.sync_all()?;

    Ok(())
}

fn valid_secret(secret: &str) -> bool {
    secret.len() >= 32
        && secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn read_bridge_secret(path: &Path) -> Result<Option<String>, CliError> {
    let secret = match read_private_file_without_following_symlink(
        path,
        "browser extension secret",
    )? {
        PrivateFileRead::Private(secret) => secret,
        PrivateFileRead::Missing => return Ok(None),
        PrivateFileRead::Exposed => {
            return Err(CliError::Config(format!(
                "browser extension secret at {} is more permissive than the private-file policy and will not be trusted; run `sunox install-browser-extension --force` to rotate it",
                path.display()
            )));
        }
    };
    let secret = secret.trim();
    if !valid_secret(secret) {
        return Err(CliError::Config(
            "browser extension secret is corrupt; run `sunox install-browser-extension --force`"
                .into(),
        ));
    }
    Ok(Some(secret.to_string()))
}

enum PrivateFileRead {
    Missing,
    Private(String),
    Exposed,
}

fn read_private_file_without_following_symlink(
    path: &Path,
    description: &str,
) -> Result<PrivateFileRead, CliError> {
    reject_symlink(path, description)?;
    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PrivateFileRead::Missing);
        }
        Err(error) => {
            return Err(CliError::Config(format!(
                "could not read {description} at {}: {error}",
                path.display()
            )));
        }
    };
    if permissions::verify_private_file_handle(&file, path)?
        == permissions::PrivateObjectStatus::Exposed
    {
        return Ok(PrivateFileRead::Exposed);
    }
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).map_err(|error| {
        CliError::Config(format!(
            "could not read {description} at {}: {error}",
            path.display()
        ))
    })?;
    String::from_utf8(contents)
        .map(PrivateFileRead::Private)
        .map_err(|error| {
            CliError::Config(format!(
                "could not read {description} at {} as UTF-8: {error}",
                path.display()
            ))
        })
}

fn read_file_without_following_symlink(
    path: &Path,
    description: &str,
) -> Result<Option<String>, CliError> {
    let Some(contents) = read_bytes_without_following_symlink(path, description)? else {
        return Ok(None);
    };
    String::from_utf8(contents).map(Some).map_err(|error| {
        CliError::Config(format!(
            "could not read {description} at {} as UTF-8: {error}",
            path.display()
        ))
    })
}

fn read_bytes_without_following_symlink(
    path: &Path,
    description: &str,
) -> Result<Option<Vec<u8>>, CliError> {
    reject_symlink(path, description)?;
    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CliError::Config(format!(
                "could not read {description} at {}: {error}",
                path.display()
            )));
        }
    };
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).map_err(|error| {
        CliError::Config(format!(
            "could not read {description} at {}: {error}",
            path.display()
        ))
    })?;
    Ok(Some(contents))
}

fn reject_symlink(path: &Path, description: &str) -> Result<(), CliError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(CliError::Config(format!(
            "{description} at {} is a symbolic link and will not be followed",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Config(format!(
            "could not inspect {description} at {}: {error}",
            path.display()
        ))),
    }
}

fn configure_no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open the directory entry itself instead
        // of transparently following a symbolic link or junction.
        options.custom_flags(0x0020_0000);
    }
}

fn record_installation_locked(config_dir: &Path) -> Result<(), CliError> {
    atomic_write_private_file(
        &config_dir.join(INSTALLATION_MARKER_FILE),
        INSTALLATION_MARKER_CONTENT.as_bytes(),
        "Browser Bridge installation marker",
    )
}

fn bridge_artifact_entry_exists_at(config_dir: &Path) -> Result<bool, CliError> {
    for name in [
        BRIDGE_SECRET_FILE,
        INSTALLATION_MARKER_FILE,
        RELOAD_PENDING_FILE,
        RUNTIME_ACK_FILE,
    ] {
        match std::fs::symlink_metadata(config_dir.join(name)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    let bundle = config_dir.join(DEFAULT_EXTENSION_DIRECTORY);
    match std::fs::symlink_metadata(&bundle) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            Ok(!directory_is_empty(&bundle)?)
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn managed_bundle_ownership_at(config_dir: &Path) -> Result<ManagedBundleOwnership, CliError> {
    let bundle = config_dir.join(DEFAULT_EXTENSION_DIRECTORY);
    let metadata = match std::fs::symlink_metadata(&bundle) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ManagedBundleOwnership::Missing);
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(ManagedBundleOwnership::Unrecognized);
    }
    if directory_is_empty(&bundle)? {
        return Ok(ManagedBundleOwnership::Empty);
    }
    Ok(if is_managed_bundle(&bundle)? {
        ManagedBundleOwnership::Managed
    } else {
        ManagedBundleOwnership::Unrecognized
    })
}

fn installation_evidence_at(config_dir: &Path) -> Result<bool, CliError> {
    if !bridge_artifact_entry_exists_at(config_dir)? {
        return Ok(false);
    }
    validate_reserved_state_entries_at(config_dir)?;
    if managed_bundle_ownership_at(config_dir)? == ManagedBundleOwnership::Unrecognized {
        return Err(CliError::Config(format!(
            "reserved Browser Bridge path {} contains an unrecognized entry that Sunox will not overwrite",
            config_dir.join(DEFAULT_EXTENSION_DIRECTORY).display()
        )));
    }
    Ok(true)
}

fn installation_marker_recorded_at(config_dir: &Path) -> Result<bool, CliError> {
    let marker = config_dir.join(INSTALLATION_MARKER_FILE);
    match read_private_file_without_following_symlink(
        &marker,
        "Browser Bridge installation marker",
    )? {
        PrivateFileRead::Private(contents) if contents == INSTALLATION_MARKER_CONTENT => {
            return Ok(true);
        }
        // This marker carries no secret or activation decision. Any owned
        // regular but stale/exposed contents remain conservative artifact
        // evidence and may be atomically rebuilt by the force installer.
        PrivateFileRead::Private(_) | PrivateFileRead::Missing | PrivateFileRead::Exposed => {}
    }
    Ok(false)
}

fn validate_reserved_state_entries_at(config_dir: &Path) -> Result<(), CliError> {
    let _ = installation_marker_recorded_at(config_dir)?;
    let _ = reload_pending_state_at(config_dir)?;
    let _ = runtime_ack_at(config_dir)?;
    Ok(())
}

pub(crate) fn bridge_secret() -> Result<Option<String>, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(None);
    };
    trusted_bridge_secret_at(&config_dir)
}

fn trusted_bridge_secret_at(config_dir: &Path) -> Result<Option<String>, CliError> {
    match bridge_pairing_status_at(config_dir) {
        BridgePairingStatus::Present => {}
        BridgePairingStatus::Missing => return Ok(None),
        BridgePairingStatus::PairingMissing => {
            return Err(CliError::Config(
                "the Browser Bridge installation is recorded, but its pairing secret is missing; run `sunox install-browser-extension --force` once to rebuild it".into(),
            ));
        }
        BridgePairingStatus::Corrupt => {
            return Err(CliError::Config(
                "Browser Bridge pairing material is inconsistent or corrupt; run `sunox install-browser-extension --force`".into(),
            ));
        }
        BridgePairingStatus::BundleMissing => {
            return Err(CliError::Config(
                "Browser Bridge pairing exists, but its managed bundle is missing; run `sunox install-browser-extension --force` once to restore it".into(),
            ));
        }
        BridgePairingStatus::BundleOutdated => {
            return Err(CliError::Config(
                "Browser Bridge pairing and managed bundle are valid, but the recognized Bridge runtime is outdated; run `sunox install-browser-extension --force` once to update it, then follow the command's activation guidance".into(),
            ));
        }
        BridgePairingStatus::BundleCorrupt => {
            return Err(CliError::Config(
                "Browser Bridge pairing exists, but its managed bundle is incomplete or modified; run `sunox install-browser-extension --force` once to replace it".into(),
            ));
        }
        BridgePairingStatus::BundleUnrecognized => {
            return Err(CliError::Config(
                "the reserved Browser Bridge path contains a bundle whose Sunox ownership cannot be proven. Sunox will not use or overwrite it; inspect the per-user configuration path manually".into(),
            ));
        }
        BridgePairingStatus::Exposed => {
            return Err(CliError::Config(
                "Browser Bridge pairing material is more permissive than the private-file policy and will not be trusted; run `sunox install-browser-extension --force` to rotate it".into(),
            ));
        }
        BridgePairingStatus::UnsafeOrInaccessible => {
            return Err(CliError::Config(
                "Browser Bridge pairing or managed activation state cannot be read safely; inspect the reserved Browser Bridge entries in the per-user Sunox configuration path before retrying".into(),
            ));
        }
    }
    let path = config_dir.join(BRIDGE_SECRET_FILE);
    read_bridge_secret(&path)
}

pub(crate) fn bridge_pairing_status() -> BridgePairingStatus {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return BridgePairingStatus::Missing;
    };
    bridge_pairing_status_at(&config_dir)
}

fn bridge_pairing_status_at(config_dir: &Path) -> BridgePairingStatus {
    let config_directory_status =
        match private_directory_status_at(config_dir, true, "Sunox configuration directory") {
            Ok(None) => return BridgePairingStatus::Missing,
            Ok(Some(status)) => status,
            Err(_) => return BridgePairingStatus::UnsafeOrInaccessible,
        };
    match bridge_artifact_entry_exists_at(config_dir) {
        Ok(false) => return BridgePairingStatus::Missing,
        Ok(true) => {}
        Err(_) => return BridgePairingStatus::UnsafeOrInaccessible,
    }
    if validate_reserved_state_entries_at(config_dir).is_err() {
        return BridgePairingStatus::UnsafeOrInaccessible;
    }
    let bundle_ownership = match managed_bundle_ownership_at(config_dir) {
        Ok(ManagedBundleOwnership::Unrecognized) => {
            return BridgePairingStatus::BundleUnrecognized;
        }
        Ok(ownership) => ownership,
        Err(_) => return BridgePairingStatus::UnsafeOrInaccessible,
    };
    if config_directory_status == permissions::PrivateObjectStatus::Exposed {
        return BridgePairingStatus::Exposed;
    }

    let secret = match read_private_file_without_following_symlink(
        &config_dir.join(BRIDGE_SECRET_FILE),
        "browser extension secret",
    ) {
        Ok(PrivateFileRead::Missing) => return BridgePairingStatus::PairingMissing,
        Ok(PrivateFileRead::Exposed) => return BridgePairingStatus::Exposed,
        Ok(PrivateFileRead::Private(secret)) if valid_secret(secret.trim()) => secret,
        Ok(PrivateFileRead::Private(_)) => return BridgePairingStatus::Corrupt,
        Err(_) => return BridgePairingStatus::UnsafeOrInaccessible,
    };

    if matches!(
        bundle_ownership,
        ManagedBundleOwnership::Missing | ManagedBundleOwnership::Empty
    ) {
        return BridgePairingStatus::BundleMissing;
    }

    let bundle = config_dir.join(DEFAULT_EXTENSION_DIRECTORY);
    match private_directory_status_at(&bundle, false, "Browser Bridge bundle directory") {
        Ok(None) => BridgePairingStatus::BundleMissing,
        Ok(Some(permissions::PrivateObjectStatus::Exposed)) => BridgePairingStatus::Exposed,
        Ok(Some(permissions::PrivateObjectStatus::Private)) => {
            match read_private_file_without_following_symlink(
                &bundle.join("config.js"),
                "Browser Bridge rendered configuration",
            ) {
                Ok(PrivateFileRead::Exposed) => BridgePairingStatus::Exposed,
                Ok(PrivateFileRead::Private(config)) => match (
                    config == render_config(secret.trim()),
                    current_managed_bundle_matches_at(&bundle),
                ) {
                    (true, Ok(true)) => BridgePairingStatus::Present,
                    (_, Err(_)) => BridgePairingStatus::UnsafeOrInaccessible,
                    _ => match known_outdated_bundle_matches_at(&bundle, secret.trim()) {
                        Ok(true) => BridgePairingStatus::BundleOutdated,
                        Ok(false) => BridgePairingStatus::BundleCorrupt,
                        Err(_) => BridgePairingStatus::UnsafeOrInaccessible,
                    },
                },
                Ok(PrivateFileRead::Missing) => BridgePairingStatus::BundleCorrupt,
                Err(_) => BridgePairingStatus::UnsafeOrInaccessible,
            }
        }
        Err(_) => BridgePairingStatus::UnsafeOrInaccessible,
    }
}

fn current_managed_bundle_matches_at(bundle: &Path) -> Result<bool, CliError> {
    let mut entries = bundle_file_names(bundle)?;
    if !entries.remove(MANAGED_SENTINEL)
        || read_file_without_following_symlink(
            &bundle.join(MANAGED_SENTINEL),
            "Browser Bridge managed marker",
        )?
        .as_deref()
            != Some(MANAGED_SENTINEL_CONTENT)
    {
        return Ok(false);
    }
    current_bundle_without_sentinel_matches(bundle, &entries)
}

fn known_outdated_bundle_matches_at(bundle: &Path, secret: &str) -> Result<bool, CliError> {
    let mut entries = bundle_file_names(bundle)?;
    match read_file_without_following_symlink(
        &bundle.join(MANAGED_SENTINEL),
        "Browser Bridge managed marker",
    )? {
        Some(contents) if contents == MANAGED_SENTINEL_CONTENT => {
            entries.remove(MANAGED_SENTINEL);
        }
        Some(_) => return Ok(false),
        None => {}
    }

    let manifest_text = match read_file_without_following_symlink(
        &bundle.join("manifest.json"),
        "Browser Bridge manifest",
    )? {
        Some(manifest) => manifest,
        None => return Ok(false),
    };
    let Ok(manifest): Result<serde_json::Value, _> = serde_json::from_str(&manifest_text) else {
        return Ok(false);
    };
    if manifest["manifest_version"] != 3
        || manifest["name"] != "Sunox Browser Bridge"
        || manifest["background"]["service_worker"] != "service-worker.js"
    {
        return Ok(false);
    }
    let Some(lineage) = find_legacy_lineage(
        manifest["version"].as_str(),
        manifest["version_name"].as_str(),
    ) else {
        return Ok(false);
    };
    if entries != bundle_file_set(lineage.files)
        || immutable_bundle_digest(bundle, lineage.files)? != lineage.digest
    {
        return Ok(false);
    }
    let config = read_file_without_following_symlink(
        &bundle.join("config.js"),
        "Browser Bridge rendered configuration",
    )?;
    Ok(config.as_deref() == Some(render_lineage_config(lineage, secret).as_str()))
}

fn private_directory_status_at(
    path: &Path,
    outer_config_directory: bool,
    description: &str,
) -> Result<Option<permissions::PrivateObjectStatus>, CliError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(CliError::Config(format!(
                "{description} at {} is not a regular directory",
                path.display()
            )));
        }
        Err(error) => return Err(CliError::Io(error)),
    }
    let directory = permissions::open_directory_without_following_symlink(path, description)?;
    let status = if outer_config_directory {
        permissions::verify_owned_nonwritable_directory_handle(&directory, path)?
    } else {
        permissions::verify_private_directory_handle(&directory, path)?
    };
    Ok(Some(status))
}

pub(crate) fn bridge_is_configured() -> Result<bool, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(false);
    };
    bridge_is_configured_at(&config_dir)
}

fn bridge_is_configured_at(config_dir: &Path) -> Result<bool, CliError> {
    match bridge_pairing_status_at(config_dir) {
        BridgePairingStatus::Present => return Ok(true),
        BridgePairingStatus::Missing => {}
        BridgePairingStatus::PairingMissing => {
            return Err(CliError::Config(
                "Browser Bridge installation evidence exists but its pairing secret is missing; rebuild it with `sunox install-browser-extension --force`".into(),
            ));
        }
        BridgePairingStatus::Corrupt => {
            return Err(CliError::Config(
                "Browser Bridge pairing material is inconsistent or corrupt; run `sunox install-browser-extension --force`".into(),
            ));
        }
        BridgePairingStatus::BundleMissing | BridgePairingStatus::BundleCorrupt => {
            return Err(CliError::Config(
                "Browser Bridge installation is recorded but its managed bundle must be restored with `sunox install-browser-extension --force`".into(),
            ));
        }
        BridgePairingStatus::BundleOutdated => {
            return Err(CliError::Config(
                "Browser Bridge is installed with a recognized but outdated runtime; update it once with `sunox install-browser-extension --force`".into(),
            ));
        }
        BridgePairingStatus::BundleUnrecognized => {
            return Err(CliError::Config(
                "the reserved Browser Bridge path contains an unrecognized bundle that Sunox will not overwrite; inspect the per-user configuration path manually".into(),
            ));
        }
        BridgePairingStatus::Exposed => {
            return Err(CliError::Config(
                "Browser Bridge pairing material is exposed and must be rotated with `sunox install-browser-extension --force`".into(),
            ));
        }
        BridgePairingStatus::UnsafeOrInaccessible => {
            return Err(CliError::Config(
                "Browser Bridge pairing or managed activation state cannot be read safely; inspect the reserved Browser Bridge entries in the per-user Sunox configuration path before retrying".into(),
            ));
        }
    }
    installation_evidence_at(config_dir)
}

fn reload_pending_at(config_dir: &Path) -> Result<Option<ReloadPendingMarker>, CliError> {
    Ok(match reload_pending_state_at(config_dir)? {
        ReloadPendingState::Missing => None,
        ReloadPendingState::Valid(marker) => Some(marker),
        // An exposed marker is not trusted for build/fingerprint/action data,
        // but remains conservative pending evidence until force rewrites it.
        ReloadPendingState::Exposed => Some(ReloadPendingMarker {
            runtime_build: BROWSER_BRIDGE_RUNTIME_BUILD.to_string(),
            secret_fingerprint: None,
            activation: PendingActivation::Restore,
        }),
    })
}

fn reload_pending_state_at(config_dir: &Path) -> Result<ReloadPendingState, CliError> {
    let path = config_dir.join(RELOAD_PENDING_FILE);
    let contents = match read_private_file_without_following_symlink(
        &path,
        "Browser Bridge reload-pending marker",
    )? {
        PrivateFileRead::Missing => return Ok(ReloadPendingState::Missing),
        PrivateFileRead::Exposed => return Ok(ReloadPendingState::Exposed),
        PrivateFileRead::Private(contents) => contents,
    };
    let contents = contents.trim();
    if valid_build_id(contents) {
        return Ok(ReloadPendingState::Valid(ReloadPendingMarker {
            runtime_build: contents.to_string(),
            secret_fingerprint: None,
            activation: PendingActivation::Reload,
        }));
    }

    let mut lines = contents.lines();
    let parsed = (|| {
        let schema = lines.next()?;
        let runtime_build = lines.next()?.strip_prefix("runtime_build=")?;
        let secret_fingerprint = lines.next()?.strip_prefix("secret_fingerprint=")?;
        let activation = match schema {
            "schema=1" => PendingActivation::Reload,
            "schema=2" => match lines.next()?.strip_prefix("activation=")? {
                "load_unpacked" => PendingActivation::LoadUnpacked,
                "reload" => PendingActivation::Reload,
                "restore" => PendingActivation::Restore,
                _ => return None,
            },
            _ => return None,
        };
        if lines.next().is_some()
            || !valid_build_id(runtime_build)
            || !valid_secret_fingerprint(secret_fingerprint)
        {
            return None;
        }
        Some(ReloadPendingMarker {
            runtime_build: runtime_build.to_string(),
            secret_fingerprint: Some(secret_fingerprint.to_string()),
            activation,
        })
    })();
    let Some(marker) = parsed else {
        return Err(CliError::Config(format!(
            "Browser Bridge runtime-ack marker at {} is corrupt. Sunox will not infer an activation action or overwrite it automatically; inspect the per-user configuration path and remove the marker only after verifying it is the regular Sunox marker file, then rerun `sunox install-browser-extension --force`",
            path.display()
        )));
    };
    Ok(ReloadPendingState::Valid(marker))
}

fn runtime_ack_at(config_dir: &Path) -> Result<Option<RuntimeAckMarker>, CliError> {
    let path = config_dir.join(RUNTIME_ACK_FILE);
    let contents = match read_private_file_without_following_symlink(
        &path,
        "Browser Bridge runtime acknowledgement",
    )? {
        PrivateFileRead::Missing | PrivateFileRead::Exposed => return Ok(None),
        PrivateFileRead::Private(contents) => contents,
    };
    let mut lines = contents.trim().lines();
    let parsed = (|| {
        if lines.next()? != "schema=1" {
            return None;
        }
        let runtime_build = lines.next()?.strip_prefix("runtime_build=")?;
        let secret_fingerprint = lines.next()?.strip_prefix("secret_fingerprint=")?;
        if lines.next().is_some()
            || !valid_build_id(runtime_build)
            || !valid_secret_fingerprint(secret_fingerprint)
        {
            return None;
        }
        Some(RuntimeAckMarker {
            runtime_build: runtime_build.to_string(),
            secret_fingerprint: secret_fingerprint.to_string(),
        })
    })();
    // Positive evidence is optional. A malformed regular private marker is
    // treated as absent so force/doctor stay conservative and can replace it;
    // unsafe file types, owners, or unreadable entries still fail above.
    Ok(parsed)
}

fn runtime_ack_matches_at(
    config_dir: &Path,
    build_id: &str,
    secret: &str,
) -> Result<bool, CliError> {
    Ok(runtime_ack_at(config_dir)?.is_some_and(|ack| {
        ack.runtime_build == build_id && ack.secret_fingerprint == secret_fingerprint(secret)
    }))
}

fn valid_build_id(build_id: &str) -> bool {
    !build_id.is_empty()
        && build_id.len() <= 128
        && build_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn valid_secret_fingerprint(fingerprint: &str) -> bool {
    fingerprint.len() == 64
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
fn mark_reload_pending_locked(
    config_dir: &Path,
    build_id: &str,
    secret: &str,
) -> Result<(), CliError> {
    mark_runtime_ack_pending_locked(config_dir, build_id, secret, PendingActivation::Reload)
}

fn mark_runtime_ack_pending_locked(
    config_dir: &Path,
    build_id: &str,
    secret: &str,
    activation: PendingActivation,
) -> Result<(), CliError> {
    // Invalidate positive evidence first. If the process stops before the
    // pending marker is written, the missing acknowledgement still forces a
    // conservative probe instead of reporting false readiness.
    remove_runtime_ack_locked(config_dir)?;
    let path = config_dir.join(RELOAD_PENDING_FILE);
    if reload_pending_state_at(config_dir)? == ReloadPendingState::Exposed {
        remove_private_state_file(&path, "Browser Bridge reload-pending marker")?;
    }
    let marker = format!(
        "schema=2\nruntime_build={build_id}\nsecret_fingerprint={}\nactivation={}\n",
        secret_fingerprint(secret),
        activation.as_str(),
    );
    atomic_write_private_file(
        &path,
        marker.as_bytes(),
        "Browser Bridge reload-pending marker",
    )
}

fn write_runtime_ack_locked(
    config_dir: &Path,
    build_id: &str,
    secret: &str,
) -> Result<(), CliError> {
    let marker = format!(
        "schema=1\nruntime_build={build_id}\nsecret_fingerprint={}\n",
        secret_fingerprint(secret),
    );
    atomic_write_private_file(
        &config_dir.join(RUNTIME_ACK_FILE),
        marker.as_bytes(),
        "Browser Bridge runtime acknowledgement",
    )
}

fn remove_runtime_ack_locked(config_dir: &Path) -> Result<(), CliError> {
    remove_private_state_file(
        &config_dir.join(RUNTIME_ACK_FILE),
        "Browser Bridge runtime acknowledgement",
    )
}

fn remove_private_state_file(path: &Path, description: &str) -> Result<(), CliError> {
    reject_symlink(path, description)?;
    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(CliError::Config(format!(
                "could not safely open {description} at {} for removal: {error}",
                path.display()
            )));
        }
    };
    // Exposed-but-owned regular files may be invalidated safely. Type and
    // ownership failures remain errors and are never overwritten.
    let _ = permissions::verify_private_file_handle(&file, path)?;
    reject_symlink(path, description)?;
    let current = std::fs::symlink_metadata(path)?;
    if !current.is_file() {
        return Err(CliError::Config(format!(
            "{description} at {} changed before removal and is no longer a regular file",
            path.display()
        )));
    }
    std::fs::remove_file(path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub(crate) fn pending_activation() -> Result<Option<PendingActivation>, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(None);
    };
    if let Some(pending) = reload_pending_at(&config_dir)? {
        return Ok(Some(pending.activation));
    }
    let Some(secret) = read_bridge_secret(&config_dir.join(BRIDGE_SECRET_FILE))? else {
        return Ok(None);
    };
    if runtime_ack_matches_at(&config_dir, BROWSER_BRIDGE_RUNTIME_BUILD, &secret)? {
        return Ok(None);
    }
    // Missing or stale positive evidence is itself an unknown activation
    // state. This also closes the case where a pending marker was manually
    // deleted before Chrome authenticated the runtime.
    Ok(Some(PendingActivation::Restore))
}

pub(crate) fn acknowledge_runtime_build(
    build_id: &str,
    authenticated_secret: &str,
) -> Result<bool, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(false);
    };
    acknowledge_runtime_build_at(&config_dir, build_id, authenticated_secret)
}

fn acknowledge_runtime_build_at(
    config_dir: &Path,
    build_id: &str,
    authenticated_secret: &str,
) -> Result<bool, CliError> {
    if build_id != BROWSER_BRIDGE_RUNTIME_BUILD || !config_dir.exists() {
        return Ok(false);
    }
    let resolved_config_dir = resolve_path_with_missing_tail(config_dir)?;
    let Some(_lock) = BrowserExtensionInstallLock::acquire_for_ack(&resolved_config_dir)? else {
        return Ok(false);
    };
    _lock.verify_config_directory(&resolved_config_dir)?;
    if _lock.config_directory_is_exposed_now(&resolved_config_dir)? {
        // Never harden and reuse a value that may already have escaped. Leave
        // the exposure visible so the managed force installer rotates it.
        return Ok(false);
    }
    let Some(current_secret) = read_bridge_secret(&resolved_config_dir.join(BRIDGE_SECRET_FILE))?
    else {
        return Ok(false);
    };
    let pairing_status = bridge_pairing_status_at(&resolved_config_dir);
    if current_secret != authenticated_secret || pairing_status != BridgePairingStatus::Present {
        return Ok(false);
    }
    let pending = match reload_pending_state_at(&resolved_config_dir)? {
        ReloadPendingState::Missing => None,
        ReloadPendingState::Valid(marker) => Some(marker),
        ReloadPendingState::Exposed => return Ok(false),
    };
    if pending.as_ref().is_some_and(|expected| {
        expected.runtime_build != build_id
            || expected
                .secret_fingerprint
                .as_deref()
                .is_some_and(|fingerprint| fingerprint != secret_fingerprint(authenticated_secret))
    }) {
        return Ok(false);
    }

    // Positive evidence is durable before pending is removed. If either write
    // or removal fails, callers retain the conservative pending state.
    write_runtime_ack_locked(&resolved_config_dir, build_id, authenticated_secret)?;
    if pending.is_some() {
        remove_private_state_file(
            &resolved_config_dir.join(RELOAD_PENDING_FILE),
            "Browser Bridge reload-pending marker",
        )?;
    }
    Ok(true)
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

fn write_private_asset(directory: &Path, name: &str, contents: &[u8]) -> Result<(), CliError> {
    let path = directory.join(name);
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    permissions::harden_private_file_handle(&file, &path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn replace_updated_bundle(
    staging: tempfile::TempDir,
    destination: &Path,
    config_dir: &Path,
    previous_snapshot: &DirectorySnapshot,
    install_secret: &str,
    persist_secret_after_swap: bool,
    pending_activation: PendingActivation,
) -> Result<(), CliError> {
    // Persist the expectation before touching the live directory. If swapping
    // the bundle fails, keeping a conservative pending marker is safer than a
    // false negative after a successful swap whose marker could not be written.
    mark_runtime_ack_pending_locked(
        config_dir,
        BROWSER_BRIDGE_RUNTIME_BUILD,
        install_secret,
        pending_activation,
    )?;
    replace_directory(staging, destination, Some(previous_snapshot), || {
        if persist_secret_after_swap {
            persist_rotated_secret(config_dir, install_secret)
        } else {
            Ok(())
        }
    })
}

fn persist_rotated_secret(config_dir: &Path, secret: &str) -> Result<(), CliError> {
    let path = config_dir.join(BRIDGE_SECRET_FILE);
    match atomic_write_private_file(&path, secret.as_bytes(), "browser extension secret") {
        Ok(()) => Ok(()),
        Err(error) => match read_bridge_secret(&path) {
            Ok(Some(persisted)) if persisted == secret => {
                eprintln!(
                    "Warning: the rotated Browser Bridge secret is active, but its final directory sync reported an error: {error}"
                );
                Ok(())
            }
            _ => Err(error),
        },
    }
}

fn replace_directory<C>(
    staging: tempfile::TempDir,
    destination: &Path,
    previous_snapshot: Option<&DirectorySnapshot>,
    commit: C,
) -> Result<(), CliError>
where
    C: FnOnce() -> Result<(), CliError>,
{
    let staged_snapshot = capture_stable_directory_snapshot(staging.path())?;
    let staged_path = staging.keep();
    let backup = destination.with_extension(format!("backup-{}", uuid::Uuid::new_v4()));
    let failed = destination.with_extension(format!("failed-{}", uuid::Uuid::new_v4()));
    replace_directory_paths(
        &staged_path,
        destination,
        &backup,
        &failed,
        previous_snapshot,
        &staged_snapshot,
        |from, to| std::fs::rename(from, to),
        directory_matches_snapshot,
        remove_snapshot_tree,
        commit,
    )
}

#[allow(clippy::too_many_arguments)]
fn replace_directory_paths<R, V, D, C>(
    staged_path: &Path,
    destination: &Path,
    backup: &Path,
    failed: &Path,
    previous_snapshot: Option<&DirectorySnapshot>,
    staged_snapshot: &DirectorySnapshot,
    mut rename: R,
    mut validate: V,
    mut remove_snapshot: D,
    commit: C,
) -> Result<(), CliError>
where
    R: FnMut(&Path, &Path) -> std::io::Result<()>,
    V: FnMut(&Path, &DirectorySnapshot) -> Result<bool, CliError>,
    D: FnMut(&Path, &DirectorySnapshot) -> Result<(), CliError>,
    C: FnOnce() -> Result<(), CliError>,
{
    if let Some(previous_snapshot) = previous_snapshot {
        if !validate(destination, previous_snapshot).unwrap_or(false) {
            let _ = remove_snapshot(staged_path, staged_snapshot);
            return Err(CliError::Config(format!(
                "{} changed after it was accepted as a managed Browser Bridge directory; no existing files were replaced",
                destination.display()
            )));
        }
        if let Err(error) = rename(destination, backup) {
            let _ = remove_snapshot(staged_path, staged_snapshot);
            return Err(CliError::Io(error));
        }
        if !validate(backup, previous_snapshot).unwrap_or(false) {
            let rollback_error = rename(backup, destination).err();
            let _ = remove_snapshot(staged_path, staged_snapshot);
            if let Some(rollback_error) = rollback_error {
                return Err(CliError::Config(format!(
                    "the previous Browser Bridge directory changed during the update and was preserved at {}; restoring it to {} also failed: {rollback_error}",
                    backup.display(),
                    destination.display()
                )));
            }
            return Err(CliError::Config(format!(
                "the previous Browser Bridge directory changed during the update; it was restored at {} and no unknown data was deleted",
                destination.display()
            )));
        }
    } else {
        match std::fs::symlink_metadata(destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = remove_snapshot(staged_path, staged_snapshot);
                return Err(CliError::Io(error));
            }
            Ok(_) => {
                let _ = remove_snapshot(staged_path, staged_snapshot);
                return Err(CliError::Config(format!(
                    "{} appeared while the Browser Bridge was being installed; refusing to replace it",
                    destination.display()
                )));
            }
        }
    }

    if let Err(install_error) = rename(staged_path, destination) {
        let rollback_error = if previous_snapshot.is_some() {
            rename(backup, destination).err()
        } else {
            None
        };
        let _ = remove_snapshot(staged_path, staged_snapshot);
        if let Some(rollback_error) = rollback_error {
            return Err(CliError::Config(format!(
                "failed to install the Browser Bridge at {}: {install_error}; rollback also failed: {rollback_error}; the previous bundle remains at {}",
                destination.display(),
                backup.display()
            )));
        }
        return Err(CliError::Io(install_error));
    }

    if let Err(commit_error) = commit() {
        let quarantine_error = rename(destination, failed).err();
        let rollback_error = if quarantine_error.is_none() && previous_snapshot.is_some() {
            rename(backup, destination).err()
        } else {
            None
        };
        let cleanup_error = if quarantine_error.is_none() {
            remove_snapshot(failed, staged_snapshot).err()
        } else {
            None
        };
        return Err(CliError::Config(format!(
            "installed Browser Bridge files but could not commit their paired runtime state: {commit_error}; quarantine_error={}; rollback_error={}; cleanup_error={}; previous bundle path={}; new bundle quarantine={}",
            display_optional_error(quarantine_error.as_ref()),
            display_optional_error(rollback_error.as_ref()),
            display_optional_error(cleanup_error.as_ref()),
            backup.display(),
            failed.display()
        )));
    }

    if let Some(previous_snapshot) = previous_snapshot {
        if !validate(backup, previous_snapshot).unwrap_or(false) {
            return Err(CliError::Config(format!(
                "Browser Bridge was installed at {}, but the previous directory changed before cleanup; it was preserved at {} and runtime activation remains pending",
                destination.display(),
                backup.display()
            )));
        }
        if let Err(error) = remove_snapshot(backup, previous_snapshot) {
            return Err(CliError::Config(format!(
                "Browser Bridge was installed at {}, but its validated previous bundle could not be removed safely from {}: {error}; runtime activation remains pending",
                destination.display(),
                backup.display()
            )));
        }
    }
    Ok(())
}

fn display_optional_error(error: Option<&impl std::fmt::Display>) -> String {
    error
        .map(ToString::to_string)
        .unwrap_or_else(|| "none".into())
}

fn remove_snapshot_tree(directory: &Path, expected: &DirectorySnapshot) -> Result<(), CliError> {
    remove_snapshot_tree_paths(directory, expected, |path| std::fs::remove_file(path))
}

fn remove_snapshot_tree_paths<F>(
    directory: &Path,
    expected: &DirectorySnapshot,
    mut remove_file: F,
) -> Result<(), CliError>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    let parent = directory.parent().ok_or_else(|| {
        CliError::Config(format!(
            "{} has no parent directory and cannot be isolated for safe cleanup",
            directory.display()
        ))
    })?;
    let cleanup = tempfile::Builder::new()
        .prefix(".sunox-browser-extension-cleanup-")
        .tempdir_in(parent)?;
    permissions::harden_private_directory(cleanup.path())?;
    let cleanup_root = cleanup.keep();
    let isolated_tree = cleanup_root.join("tree");
    let isolated_entries = cleanup_root.join("entries");
    std::fs::create_dir(&isolated_entries)?;

    if let Err(error) = std::fs::rename(directory, &isolated_tree) {
        let _ = std::fs::remove_dir(&isolated_entries);
        let _ = std::fs::remove_dir(&cleanup_root);
        return Err(CliError::Io(error));
    }

    let matches = directory_matches_snapshot(&isolated_tree, expected).map_err(|error| {
        preserved_cleanup_error(
            &cleanup_root,
            format!("could not validate the atomically isolated directory: {error}"),
        )
    })?;
    if !matches {
        return Err(preserved_cleanup_error(
            &cleanup_root,
            "the atomically isolated directory did not match its accepted snapshot",
        ));
    }

    for (name, expected_digest) in &expected.file_digests {
        let source = isolated_tree.join(name);
        let isolated = isolated_entries.join(name);
        if let Some(parent) = isolated.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                preserved_cleanup_error(
                    &cleanup_root,
                    format!("could not prepare an isolated file path for {name}: {error}"),
                )
            })?;
        }
        std::fs::rename(&source, &isolated).map_err(|error| {
            preserved_cleanup_error(
                &cleanup_root,
                format!("could not atomically isolate {name} before deletion: {error}"),
            )
        })?;
        let Some(contents) =
            read_bytes_without_following_symlink(&isolated, "Browser Bridge cleanup asset")
                .map_err(|error| {
                    preserved_cleanup_error(
                        &cleanup_root,
                        format!("could not validate isolated cleanup asset {name}: {error}"),
                    )
                })?
        else {
            return Err(preserved_cleanup_error(
                &cleanup_root,
                format!("isolated cleanup asset {name} disappeared before validation"),
            ));
        };
        if encode_digest(Sha256::digest(contents)) != *expected_digest {
            return Err(preserved_cleanup_error(
                &cleanup_root,
                format!("isolated cleanup asset {name} did not match its accepted digest"),
            ));
        }
        remove_file(&isolated).map_err(|error| {
            preserved_cleanup_error(
                &cleanup_root,
                format!("could not delete validated isolated cleanup asset {name}: {error}"),
            )
        })?;
    }
    for name in expected.directories.iter().rev() {
        std::fs::remove_dir(isolated_tree.join(name)).map_err(|error| {
            preserved_cleanup_error(
                &cleanup_root,
                format!(
                    "could not remove expected directory {name}; concurrently added data was not deleted: {error}"
                ),
            )
        })?;
    }
    std::fs::remove_dir(&isolated_tree).map_err(|error| {
        preserved_cleanup_error(
            &cleanup_root,
            format!(
                "could not remove the isolated tree; concurrently added data was not deleted: {error}"
            ),
        )
    })?;
    for name in expected.directories.iter().rev() {
        std::fs::remove_dir(isolated_entries.join(name)).map_err(|error| {
            preserved_cleanup_error(
                &cleanup_root,
                format!("could not remove isolated cleanup directory {name}: {error}"),
            )
        })?;
    }
    std::fs::remove_dir(&isolated_entries).map_err(|error| {
        preserved_cleanup_error(
            &cleanup_root,
            format!("could not remove isolated cleanup entry root: {error}"),
        )
    })?;

    match std::fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(preserved_cleanup_error(
                &cleanup_root,
                format!("could not confirm that the original cleanup path stayed absent: {error}"),
            ));
        }
        Ok(_) => {
            std::fs::remove_dir(&cleanup_root).map_err(CliError::Io)?;
            return Err(CliError::Config(format!(
                "{} appeared during cleanup and was preserved",
                directory.display()
            )));
        }
    }

    std::fs::remove_dir(&cleanup_root)?;
    Ok(())
}

fn preserved_cleanup_error(cleanup_root: &Path, detail: impl std::fmt::Display) -> CliError {
    CliError::Config(format!(
        "{detail}; cleanup stopped and all remaining entries were preserved at {}",
        cleanup_root.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::{
        BRIDGE, BridgePairingStatus, BrowserExtensionInstallLock, CONFIG_TEMPLATE,
        INSTALL_BEHAVIOR_GUIDANCE, InstallOutcome, LOOPBACK_TRANSPORT, MANAGED_SENTINEL, MANIFEST,
        OFFSCREEN, OFFSCREEN_HTML, PAGE, POLL_WORKER, PendingActivation, SERVICE_WORKER, SHARED,
        acknowledge_runtime_build_at, activation_guidance, bridge_is_configured_at,
        bridge_pairing_status_at, capture_stable_directory_snapshot, current_bundle_file_set,
        install_bundle, install_next_steps, installation_evidence_at,
        manifest_bytes_match_current_runtime, mark_reload_pending_locked, read_bridge_secret,
        reload_pending_at, remove_snapshot_tree, remove_snapshot_tree_paths, render_config,
        render_manifest, render_manifest_with_version_name, replace_directory_paths,
        runtime_state_after_probe, secret_fingerprint,
    };

    #[test]
    fn extension_assets_share_the_bridge_contract() {
        assert_eq!(super::BROWSER_BRIDGE_RUNTIME_BUILD, "0.3.41");
        assert!(MANIFEST.contains("https://suno.com/*"));
        assert!(MANIFEST.contains("http://127.0.0.1/*"));
        assert!(MANIFEST.contains("\"version\": \"__SUNOX_BRIDGE_RUNTIME_BUILD__\""));
        assert!(MANIFEST.contains("\"version_name\": \"__SUNOX_BRIDGE_RUNTIME_BUILD__\""));
        assert!(!MANIFEST.contains("__SUNOX_VERSION__"));
        assert!(MANIFEST.contains("\"alarms\""));
        assert!(!MANIFEST.contains("\"declarativeNetRequestFeedback\""));
        assert!(MANIFEST.contains("\"declarativeNetRequestWithHostAccess\""));
        assert!(MANIFEST.contains("\"offscreen\""));
        assert!(MANIFEST.contains("\"webRequest\""));
        assert!(!MANIFEST.contains("\"activeTab\""));
        assert!(!MANIFEST.contains("\"storage\""));
        assert!(!MANIFEST.contains("\"system.display\""));
        assert!(!MANIFEST.contains("\"tabs\""));
        assert!(MANIFEST.contains("\"all_frames\": true"));
        assert!(MANIFEST.contains("\"minimum_chrome_version\": \"128\""));
        assert!(MANIFEST.contains("\"world\": \"ISOLATED\""));
        assert!(!MANIFEST.contains("\"world\": \"MAIN\""));
        assert!(!MANIFEST.contains("https://auth.suno.com/*"));
        assert!(MANIFEST.contains("icons/icon-16.png"));
        assert!(MANIFEST.contains("icons/icon-128.png"));
        let manifest: serde_json::Value =
            serde_json::from_str(MANIFEST).expect("valid Browser Bridge manifest asset");
        assert_eq!(
            manifest["content_scripts"],
            serde_json::json!([{
                "matches": ["https://suno.com/*"],
                "js": ["bridge.js"],
                "run_at": "document_start",
                "all_frames": true,
                "world": "ISOLATED"
            }])
        );
        assert_eq!(
            manifest["web_accessible_resources"],
            serde_json::json!([{
                "resources": ["page.js"],
                "matches": ["https://suno.com/*"]
            }])
        );
        assert!(SERVICE_WORKER.contains("chrome.offscreen.createDocument"));
        assert!(SERVICE_WORKER.contains("chrome.declarativeNetRequest.updateDynamicRules"));
        assert!(SERVICE_WORKER.contains("chrome.declarativeNetRequest.updateSessionRules"));
        assert!(SERVICE_WORKER.contains("addRules"));
        assert!(SERVICE_WORKER.contains("content-security-policy"));
        assert!(SERVICE_WORKER.contains("x-frame-options"));
        assert!(SERVICE_WORKER.contains("challengeDocumentCsp"));
        assert!(SERVICE_WORKER.contains("__sunox_bridge"));
        assert!(SERVICE_WORKER.contains("chrome.webRequest.onSendHeaders"));
        assert!(SERVICE_WORKER.contains("chrome.webRequest.onResponseStarted"));
        assert!(SERVICE_WORKER.contains("reasons: [\"IFRAME_SCRIPTING\", \"WORKERS\"]"));
        assert!(SERVICE_WORKER.contains("chrome.alarms"));
        assert!(SERVICE_WORKER.contains("chrome.tabs.TAB_ID_NONE"));
        assert!(!SERVICE_WORKER.contains("chrome.windows"));
        assert!(!SERVICE_WORKER.contains("chrome.system.display"));
        assert!(!SERVICE_WORKER.contains("chrome.tabs.create"));
        assert!(SERVICE_WORKER.contains("bindManagedSenderIdentity"));
        assert!(
            SERVICE_WORKER.contains("frameEnvironmentPromise = prepareFrameEnvironmentForOwner")
        );
        assert!(SERVICE_WORKER.contains("pendingEnvironmentOwnerDocumentId"));
        assert!(SERVICE_WORKER.contains("requireCurrentOffscreenOwner"));
        assert!(SERVICE_WORKER.contains("offscreenOwnerBinding"));
        assert!(SERVICE_WORKER.contains("OFFSCREEN_CLIENT_ID_PATTERN"));
        assert!(SERVICE_WORKER.contains("pendingEnvironmentNonce !== nonce"));
        assert!(SERVICE_WORKER.contains("pendingEnvironmentNonce === nonce"));
        assert!(SERVICE_WORKER.contains("sunox-frame-environment-retire-v1"));
        assert!(SERVICE_WORKER.contains("network.retiring"));
        assert!(SERVICE_WORKER.contains("sunox-managed-frame-stage-report-v1"));
        assert!(SERVICE_WORKER.contains("sunox-managed-frame-stage-v1"));
        assert!(OFFSCREEN_HTML.contains("transport-loopback.js"));
        assert!(OFFSCREEN_HTML.contains("offscreen.js"));
        assert!(OFFSCREEN_HTML.contains("shared.js"));
        assert!(OFFSCREEN.contains("transport.claimChallenge"));
        assert!(OFFSCREEN.contains("transport.submitResult"));
        assert!(OFFSCREEN.contains("runtimeMessageBeforeDeadline"));
        assert!(!OFFSCREEN.contains("chrome.runtime.getContexts"));
        assert!(!OFFSCREEN.contains("ownerDocumentId"));
        assert!(OFFSCREEN.contains("clientId"));
        assert!(OFFSCREEN.contains("managedFramePrepareTimeoutMs = 9_000"));
        assert!(OFFSCREEN.contains("managedFrameReleaseTimeoutMs = 500"));
        assert!(OFFSCREEN.contains("document.createElement(\"iframe\")"));
        assert!(OFFSCREEN.contains("managedFrame.credentialless = true"));
        assert!(
            OFFSCREEN.contains("managedFrame.referrerPolicy = \"strict-origin-when-cross-origin\"")
        );
        assert!(OFFSCREEN.contains("sunox-managed-frame-execute-v2"));
        assert!(OFFSCREEN.contains("sunox-frame-environment-retire-v1"));
        assert!(OFFSCREEN.contains("retirementPending"));
        assert!(OFFSCREEN.contains("sunox-managed-frame-stage-v1"));
        assert!(OFFSCREEN.contains("managedFrameWarmupGraceMs"));
        assert!(!OFFSCREEN.contains("sunox-managed-window"));
        assert!(OFFSCREEN.contains("new Worker(\"poll-worker.js\")"));
        assert!(POLL_WORKER.contains("sunox-poll"));
        assert!(SHARED.contains("errorMessage(error)"));
        assert!(LOOPBACK_TRANSPORT.contains("contractVersion: 1"));
        for expected in [
            "/v3/challenge/hello",
            "/v3/challenge/probe-ack",
            "/v3/challenge/claim",
            "/v3/challenge/result",
            "sunox-bridge-server-v3",
            "sunox-bridge-client-v3",
            "sunox-bridge-result-v3",
            "sunox-bridge-receipt-v3",
        ] {
            assert!(LOOPBACK_TRANSPORT.contains(expected));
        }
        assert!(!LOOPBACK_TRANSPORT.contains("/v2/challenge/"));
        assert!(!LOOPBACK_TRANSPORT.contains("/v1/challenge/"));
        assert!(!LOOPBACK_TRANSPORT.contains("sunox-bridge-receipt-v1"));
        assert!(!LOOPBACK_TRANSPORT.contains("Authorization"));
        assert!(BRIDGE.contains("chrome.runtime.connect"));
        assert!(BRIDGE.contains("window.stop()"));
        assert!(BRIDGE.contains("let html = document.documentElement"));
        assert!(BRIDGE.contains("html.replaceChildren(head, body)"));
        assert!(!BRIDGE.contains("document.replaceChildren"));
        assert!(BRIDGE.contains("chrome.runtime.getURL(\"page.js\")"));
        assert!(BRIDGE.contains("document.head.appendChild(mainRunner)"));
        assert!(BRIDGE.contains("sunox-managed-frame-stage-report-v1"));
        assert!(BRIDGE.contains("sunox-managed-frame-result-v2"));
        assert!(PAGE.contains("hcaptcha.execute"));
        assert!(PAGE.contains("turnstile.execute"));
        assert!(CONFIG_TEMPLATE.contains("schemaVersion: 1"));
        assert!(CONFIG_TEMPLATE.contains("transport: \"loopback\""));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_PROTOCOL_VERSION__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_PORT_START__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_PORT_COUNT__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_RUNTIME_BUILD__"));
        assert!(CONFIG_TEMPLATE.contains("__SUNOX_BRIDGE_SECRET__"));
        assert!(OFFSCREEN.contains("transportReceipt"));
        assert!(!BRIDGE.contains("bridgePort"));
        assert!(!BRIDGE.contains("clientNonce"));
        assert!(!BRIDGE.contains("serverNonce"));
    }

    #[test]
    fn rendered_config_uses_the_rust_bridge_contract() {
        let config = render_config("secret-value");

        assert!(config.contains("protocolVersion: 3"));
        assert!(config.contains(&format!(
            "runtimeBuild: \"{}\"",
            super::BROWSER_BRIDGE_RUNTIME_BUILD
        )));
        assert!(config.contains("portStart: 29764"));
        assert!(config.contains("portCount: 8"));
        assert!(config.contains("sharedSecret: \"secret-value\""));
        assert!(!config.contains("__SUNOX_BRIDGE_"));
    }

    #[test]
    fn rendered_manifest_uses_only_the_stable_bridge_runtime_identity() {
        let manifest: serde_json::Value =
            serde_json::from_str(&render_manifest()).expect("rendered extension manifest");

        assert_eq!(manifest["version"], super::BROWSER_BRIDGE_RUNTIME_BUILD);
        assert_eq!(
            manifest["version_name"],
            super::BROWSER_BRIDGE_RUNTIME_BUILD
        );
        assert!(!MANIFEST.contains("__SUNOX_VERSION__"));
        assert!(!render_manifest().contains("__SUNOX_BRIDGE_RUNTIME_BUILD__"));
    }

    #[test]
    fn only_exact_current_or_known_dev_manifests_match_the_runtime_identity() {
        let current = render_manifest();
        let historical = render_manifest_with_version_name("0.2.0");
        let mut reformatted: serde_json::Value =
            serde_json::from_str(&historical).expect("historical manifest");
        reformatted["description"] =
            serde_json::json!("Runs Suno generation challenges invisibly for the Sunox CLI.");
        let reformatted = serde_json::to_vec(&reformatted).expect("reformatted manifest");
        let unknown = render_manifest_with_version_name("0.2.1");
        let duplicate_version_name = historical.replacen(
            "\"version_name\": \"0.2.0\",",
            "\"version_name\": \"0.2.0\",\n  \"version_name\": \"0.2.0\",",
            1,
        );

        assert!(manifest_bytes_match_current_runtime(current.as_bytes()));
        assert!(manifest_bytes_match_current_runtime(historical.as_bytes()));
        assert!(!manifest_bytes_match_current_runtime(&reformatted));
        assert!(!manifest_bytes_match_current_runtime(unknown.as_bytes()));
        assert!(!manifest_bytes_match_current_runtime(
            duplicate_version_name.as_bytes()
        ));
    }

    #[test]
    fn installed_current_bundle_entries_come_from_the_asset_registry() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);

        install_bundle(&destination, &config_dir, false).expect("install current bundle");
        let mut installed = super::bundle_file_names(&destination).expect("installed entries");
        installed.remove(MANAGED_SENTINEL);

        assert_eq!(installed, current_bundle_file_set());
        assert!(
            super::current_bundle_without_sentinel_matches(&destination, &installed)
                .expect("registry-backed comparison")
        );
    }

    #[test]
    fn install_guidance_describes_the_current_hidden_frame_contract() {
        assert!(INSTALL_BEHAVIOR_GUIDANCE.contains("offscreen document"));
        assert!(INSTALL_BEHAVIOR_GUIDANCE.contains("nonce-bound Suno iframe"));
        assert!(INSTALL_BEHAVIOR_GUIDANCE.contains("no tab or browser window"));
        assert!(INSTALL_BEHAVIOR_GUIDANCE.contains("fails closed"));
    }

    #[test]
    fn update_acknowledged_before_render_is_a_valid_no_reload_success() {
        let outcome = InstallOutcome::Updated;
        let runtime_state = outcome.runtime_state(None);

        assert_eq!(outcome.status(runtime_state), "updated");
        assert_eq!(runtime_state.reload_required, Some(false));
        assert!(!runtime_state.runtime_ack_pending);
        let next_steps = install_next_steps(outcome, runtime_state);
        assert!(next_steps.contains(&"No Chrome reload is required"));
    }

    #[test]
    fn changed_bundle_requires_one_reload_until_the_runtime_authenticates() {
        let outcome = InstallOutcome::Updated;
        let runtime_state = outcome.runtime_state(Some(PendingActivation::Reload));

        assert_eq!(outcome.status(runtime_state), "reload_pending");
        assert_eq!(runtime_state.reload_required, Some(true));
        assert!(runtime_state.runtime_ack_pending);
        assert_eq!(
            runtime_state.pending_origin,
            Some(PendingActivation::Reload)
        );
        let next_steps = install_next_steps(outcome, runtime_state);
        assert!(next_steps.contains(&"Click Reload once on the existing extension"));
        assert!(
            next_steps
                .iter()
                .any(|step| step.contains("doctor --browser-bridge"))
        );
    }

    #[test]
    fn updated_restore_origin_stays_activation_pending_with_conditional_loading_guidance() {
        let outcome = InstallOutcome::Updated;
        let runtime_state = outcome.runtime_state(Some(PendingActivation::Restore));

        assert_eq!(outcome.status(runtime_state), "activation_pending");
        assert_eq!(runtime_state.reload_required, None);
        assert!(runtime_state.runtime_ack_pending);
        assert_eq!(
            activation_guidance(outcome, runtime_state),
            (
                Some("ensure_loaded"),
                vec!["load_unpacked_if_missing", "enable_and_reload_if_present"]
            )
        );
        let next_steps = install_next_steps(outcome, runtime_state);
        assert!(next_steps.iter().any(|step| step.contains("Load unpacked")));
        assert!(next_steps.iter().any(|step| step.contains("card exists")));
    }

    #[test]
    fn fresh_install_requires_load_unpacked_and_runtime_acknowledgement() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);

        let outcome = install_bundle(&destination, &config_dir, false).expect("initial install");
        let pending = reload_pending_at(&config_dir)
            .expect("activation marker")
            .map(|marker| marker.activation);
        let runtime_state = outcome.runtime_state(pending);

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(outcome.status(runtime_state), "installed");
        assert_eq!(runtime_state.reload_required, None);
        assert!(runtime_state.runtime_ack_pending);
        assert_eq!(
            runtime_state.pending_origin,
            Some(PendingActivation::LoadUnpacked)
        );
        let next_steps = install_next_steps(outcome, runtime_state);
        assert!(next_steps.iter().any(|step| step.contains("Load unpacked")));
        assert!(
            next_steps
                .iter()
                .any(|step| step.contains("doctor --browser-bridge"))
        );
        assert!(!next_steps.iter().any(|step| step.contains("Click Reload")));
        assert_eq!(
            activation_guidance(outcome, runtime_state),
            (Some("load_unpacked"), vec!["load_unpacked"])
        );
    }

    #[test]
    fn already_current_pending_bundle_reports_unknown_instead_of_repeating_reload() {
        let outcome = InstallOutcome::AlreadyCurrent;
        let runtime_state = outcome.runtime_state(Some(PendingActivation::Reload));

        assert_eq!(outcome.status(runtime_state), "runtime_ack_pending");
        assert_eq!(runtime_state.reload_required, None);
        assert!(runtime_state.runtime_ack_pending);
        let next_steps = install_next_steps(outcome, runtime_state);
        assert!(
            next_steps
                .iter()
                .any(|step| step.contains("doctor --browser-bridge"))
        );
        assert!(
            !next_steps
                .iter()
                .any(|step| step == &"Click Reload on the existing extension")
        );
    }

    #[tokio::test]
    async fn already_current_pending_bundle_probes_and_accepts_only_the_exact_runtime_pairing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");

        let old_runtime_state =
            runtime_state_after_probe(InstallOutcome::AlreadyCurrent, &config_dir, async {
                assert!(
                    !acknowledge_runtime_build_at(&config_dir, "0.3.5", &secret)
                        .expect("old runtime acknowledgement")
                );
            })
            .await
            .expect("old runtime probe state");
        assert_eq!(old_runtime_state.reload_required, None);
        assert!(old_runtime_state.runtime_ack_pending);

        let current_runtime_state =
            runtime_state_after_probe(InstallOutcome::AlreadyCurrent, &config_dir, async {
                assert!(
                    acknowledge_runtime_build_at(
                        &config_dir,
                        super::BROWSER_BRIDGE_RUNTIME_BUILD,
                        &secret,
                    )
                    .expect("current runtime acknowledgement")
                );
            })
            .await
            .expect("current runtime probe state");
        assert_eq!(current_runtime_state.reload_required, Some(false));
        assert!(!current_runtime_state.runtime_ack_pending);
    }

    #[test]
    fn supported_legacy_lineage_digest_fixtures_are_exact_and_not_interchangeable() {
        for (extension_version, cli_version) in [
            ("0.1.3", None),
            ("0.3.4", Some("0.1.0")),
            ("0.3.5", Some("0.1.1")),
            ("0.3.6", Some("0.1.1")),
            ("0.3.16", Some("0.2.0")),
        ] {
            let lineage = super::find_legacy_lineage(Some(extension_version), cli_version)
                .expect("published lineage");
            let entries = lineage
                .files
                .iter()
                .map(|name| (*name).to_string())
                .collect();
            assert!(super::legacy_lineage_facts_match(
                lineage,
                &entries,
                lineage.digest,
                true
            ));

            let mut missing = entries.clone();
            missing.remove("config.js");
            assert!(!super::legacy_lineage_facts_match(
                lineage,
                &missing,
                lineage.digest,
                true
            ));
            let mut extra = entries.clone();
            extra.insert("unknown.js".to_string());
            assert!(!super::legacy_lineage_facts_match(
                lineage,
                &extra,
                lineage.digest,
                true
            ));
            assert!(!super::legacy_lineage_facts_match(
                lineage,
                &entries,
                &format!("{}0", &lineage.digest[..lineage.digest.len() - 1]),
                true
            ));
            assert!(!super::legacy_lineage_facts_match(
                lineage,
                &entries,
                lineage.digest,
                false
            ));
        }

        let old = super::find_legacy_lineage(Some("0.1.3"), None).expect("old lineage");
        let modern =
            super::find_legacy_lineage(Some("0.3.4"), Some("0.1.0")).expect("modern lineage");
        let old_entries = old.files.iter().map(|name| (*name).to_string()).collect();
        assert!(!super::legacy_lineage_facts_match(
            old,
            &old_entries,
            modern.digest,
            true
        ));
        assert!(super::find_legacy_lineage(Some("0.3.3"), Some("0.1.0")).is_none());
        assert!(super::find_legacy_lineage(Some("0.3.5"), Some("0.1.0")).is_none());
        let pre_release =
            super::find_legacy_lineage(Some("0.3.6"), Some("0.1.1")).expect("pre-release lineage");
        assert_eq!(pre_release.protocol_version, 3);
        assert_eq!(pre_release.runtime_build, Some("0.3.6"));
        let current_pre_release = super::find_legacy_lineage(Some("0.3.16"), Some("0.2.0"))
            .expect("current pre-release lineage");
        assert_eq!(current_pre_release.protocol_version, 3);
        assert_eq!(current_pre_release.runtime_build, Some("0.3.16"));
    }

    #[test]
    fn force_refuses_to_replace_an_unmanaged_nonempty_directory() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("important-files");
        let config_dir = temp.path().join("config");
        fs::create_dir(&destination).expect("foreign directory");
        fs::write(destination.join("keep.txt"), "do not delete").expect("foreign file");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("foreign directory must be rejected");

        assert!(error.to_string().contains("not a Sunox-managed"));
        assert_eq!(
            fs::read_to_string(destination.join("keep.txt")).expect("preserved foreign file"),
            "do not delete"
        );
    }

    #[test]
    fn force_refuses_a_manifest_only_lookalike_directory() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("lookalike");
        let config_dir = temp.path().join("config");
        fs::create_dir(&destination).expect("lookalike directory");
        fs::write(destination.join("manifest.json"), render_manifest())
            .expect("lookalike manifest");
        fs::write(destination.join("keep.txt"), "not managed").expect("foreign file");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("partial lookalike must be rejected");

        assert!(error.to_string().contains("not a Sunox-managed"));
        assert!(destination.join("keep.txt").exists());
    }

    #[test]
    fn force_refuses_unknown_files_even_when_the_managed_sentinel_exists() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        fs::write(destination.join("keep.txt"), "user data").expect("unknown file");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("managed directory with unknown data must be rejected");

        assert!(error.to_string().contains("not a Sunox-managed"));
        assert_eq!(
            fs::read_to_string(destination.join("keep.txt")).expect("preserved unknown file"),
            "user data"
        );
    }

    #[test]
    fn force_refuses_unknown_empty_directories_in_a_managed_bundle() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        fs::create_dir(destination.join("keep-empty")).expect("unknown directory");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("managed directory with unknown directory must be rejected");

        assert!(error.to_string().contains("unsupported directory"));
        assert!(destination.join("keep-empty").is_dir());
    }

    #[test]
    fn custom_destination_cannot_replace_the_config_directory_or_its_ancestor() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("application-data");
        let config_dir = destination.join("sunox");
        fs::create_dir(&destination).expect("empty destination");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("config ancestor must never be replaceable");

        assert!(
            error
                .to_string()
                .contains("contains the Sunox configuration")
        );
        assert!(destination.is_dir());
        assert!(!config_dir.join("browser-extension-secret").exists());
    }

    #[test]
    fn custom_destination_cannot_overlap_reserved_config_metadata() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("sunox");

        for relative in [
            "browser-extension-secret",
            "browser-extension-reload-pending",
            "auth.json",
            "config.toml",
            "locks/browser-extension",
            "another-extension",
        ] {
            let destination = config_dir.join(relative);
            let error = install_bundle(&destination, &config_dir, false)
                .expect_err("only the default Browser Bridge directory is allowed in config");

            assert!(
                error
                    .to_string()
                    .contains("overlaps reserved Sunox configuration metadata"),
                "unexpected error for {relative}: {error}"
            );
            assert!(
                !destination.exists(),
                "reserved destination was created for {relative}"
            );
        }
        assert!(
            !config_dir.join("browser-extension-secret").exists(),
            "path validation must run before pairing state is created"
        );
    }

    #[test]
    fn pairing_secret_cannot_be_replaced_as_a_custom_extension_destination() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("sunox");
        let secret_path = config_dir.join(super::BRIDGE_SECRET_FILE);
        let original_secret = "a".repeat(64);
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(&secret_path, &original_secret).expect("existing pairing secret");

        let error = install_bundle(&secret_path, &config_dir, true)
            .expect_err("pairing secret must never be treated as an extension directory");

        assert!(
            error
                .to_string()
                .contains("overlaps reserved Sunox configuration metadata")
        );
        assert_eq!(
            fs::read_to_string(&secret_path).expect("pairing secret was preserved"),
            original_secret
        );
        let entries = fs::read_dir(&config_dir)
            .expect("config entries")
            .map(|entry| entry.expect("config entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(
            entries,
            vec![std::ffi::OsString::from(super::BRIDGE_SECRET_FILE)],
            "the rejected install must not leave a backup directory or lock file"
        );
    }

    #[test]
    fn parent_components_cannot_alias_a_config_directory_ancestor() {
        let temp = tempfile::tempdir().expect("temp dir");
        let alias_component = temp.path().join("alias-component");
        fs::create_dir(&alias_component).expect("alias component");
        let destination = alias_component.join("..");
        let config_dir = temp.path().join("sunox");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("lexical alias of config ancestor must be rejected");

        assert!(
            error
                .to_string()
                .contains("contains the Sunox configuration")
        );
        assert!(!config_dir.join("browser-extension-secret").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_cannot_alias_a_config_directory_ancestor() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let real = temp.path().join("real");
        let destination_real = real.join("managed");
        let config_dir = destination_real.join("sunox");
        fs::create_dir_all(&destination_real).expect("real destination");
        let alias = temp.path().join("alias");
        symlink(&real, &alias).expect("parent symlink");
        let destination = alias.join("managed");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("symlink alias of config ancestor must be rejected");

        assert!(
            error
                .to_string()
                .contains("contains the Sunox configuration")
        );
        assert!(!config_dir.join("browser-extension-secret").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_followed_by_parent_component_cannot_bypass_config_ancestor_guard() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let real = temp.path().join("real");
        let destination_real = real.join("bundle");
        let config_dir = destination_real.join("sunox");
        fs::create_dir_all(real.join("child")).expect("real symlink target");
        fs::create_dir(&destination_real).expect("empty destination");
        let alias = temp.path().join("jump");
        symlink(real.join("child"), &alias).expect("parent symlink");
        let destination = alias.join("..").join("bundle");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("symlink plus parent alias of config ancestor must be rejected");

        assert!(
            error
                .to_string()
                .contains("contains the Sunox configuration")
        );
        assert!(destination_real.is_dir());
        assert!(!config_dir.join("browser-extension-secret").exists());
    }

    #[test]
    fn default_destination_inside_the_config_directory_remains_allowed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("sunox");
        let destination = config_dir.join("browser-extension");

        let outcome =
            install_bundle(&destination, &config_dir, false).expect("default-shaped install");

        assert_eq!(outcome, InstallOutcome::Installed);
        assert!(destination.join(MANAGED_SENTINEL).is_file());
        assert!(config_dir.join("browser-extension-secret").is_file());
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("activation marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::LoadUnpacked)
        );
    }

    #[test]
    fn install_into_an_existing_empty_directory_needs_no_force_and_is_still_installed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        fs::create_dir_all(&destination).expect("empty destination");
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::Missing
        );
        assert!(!bridge_is_configured_at(&config_dir).expect("empty directory is not configured"));
        assert!(
            !installation_evidence_at(&config_dir)
                .expect("an empty default bundle alone is not installation evidence")
        );

        let outcome =
            install_bundle(&destination, &config_dir, false).expect("install into empty directory");

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("activation marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::LoadUnpacked)
        );
    }

    #[cfg(unix)]
    #[test]
    fn unrelated_or_exposed_config_directory_does_not_change_first_install_origin() {
        use std::os::unix::fs::PermissionsExt;

        for mode in [0o755, 0o775] {
            let temp = tempfile::tempdir().expect("temp dir");
            let config_dir = temp.path().join(format!("config-{mode:o}"));
            let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
            fs::create_dir(&config_dir).expect("pre-existing config directory");
            fs::write(config_dir.join("auth.json"), "{}").expect("unrelated Sunox config");
            fs::set_permissions(&config_dir, fs::Permissions::from_mode(mode))
                .expect("set config directory mode");

            assert_eq!(
                bridge_pairing_status_at(&config_dir),
                BridgePairingStatus::Missing
            );
            assert!(
                !bridge_is_configured_at(&config_dir)
                    .expect("unrelated config must not count as Bridge installation")
            );

            let outcome =
                install_bundle(&destination, &config_dir, false).expect("first Bridge install");

            assert_eq!(outcome, InstallOutcome::Installed);
            assert_eq!(
                reload_pending_at(&config_dir)
                    .expect("activation marker")
                    .map(|marker| marker.activation),
                Some(PendingActivation::LoadUnpacked)
            );
            assert_eq!(
                fs::read_to_string(config_dir.join("auth.json")).expect("unrelated config remains"),
                "{}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn exposed_windows_config_acl_does_not_change_first_install_origin() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        fs::create_dir(&config_dir).expect("pre-existing config directory");
        fs::write(config_dir.join("auth.json"), "{}").expect("unrelated Sunox config");
        super::permissions::make_world_readable_for_test(&config_dir, true);

        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::Missing
        );
        let outcome =
            install_bundle(&destination, &config_dir, false).expect("first Bridge install");

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("activation marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::LoadUnpacked)
        );
        super::permissions::assert_private_acl(&config_dir, true);
    }

    #[tokio::test]
    async fn missing_bundle_restores_as_ensure_loaded_until_exact_runtime_ack() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("initial acknowledgement")
        );
        fs::remove_dir_all(&destination).expect("simulate deleted bundle");

        let outcome = install_bundle(&destination, &config_dir, false).expect("restore bundle");
        let restored_secret = fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE))
            .expect("restored secret");
        assert_eq!(outcome, InstallOutcome::Restored);
        assert_ne!(
            restored_secret.trim(),
            secret.trim(),
            "restoring a missing bundle must invalidate the old positive acknowledgement"
        );
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("restore marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::Restore)
        );

        let old_runtime_state = runtime_state_after_probe(outcome, &config_dir, async {
            assert!(
                !acknowledge_runtime_build_at(
                    &config_dir,
                    super::BROWSER_BRIDGE_RUNTIME_BUILD,
                    secret.trim(),
                )
                .expect("old runtime acknowledgement")
            );
        })
        .await
        .expect("old runtime state");
        assert_eq!(old_runtime_state.reload_required, None);
        assert!(old_runtime_state.runtime_ack_pending);
        assert_eq!(
            activation_guidance(outcome, old_runtime_state),
            (
                Some("ensure_loaded"),
                vec!["load_unpacked_if_missing", "enable_and_reload_if_present"]
            )
        );

        let second_outcome =
            install_bundle(&destination, &config_dir, true).expect("idempotent restore check");
        let second_state = second_outcome.runtime_state(
            reload_pending_at(&config_dir)
                .expect("restore marker")
                .map(|marker| marker.activation),
        );
        assert_eq!(second_outcome, InstallOutcome::AlreadyCurrent);
        assert_eq!(
            second_state.pending_origin,
            Some(PendingActivation::Restore)
        );
        assert_eq!(
            activation_guidance(second_outcome, second_state),
            (
                Some("ensure_loaded"),
                vec!["load_unpacked_if_missing", "enable_and_reload_if_present"]
            )
        );
        assert!(
            install_next_steps(second_outcome, second_state)
                .iter()
                .any(|step| step.contains("Load unpacked"))
        );

        let exact_runtime_state = runtime_state_after_probe(second_outcome, &config_dir, async {
            assert!(
                acknowledge_runtime_build_at(
                    &config_dir,
                    super::BROWSER_BRIDGE_RUNTIME_BUILD,
                    restored_secret.trim(),
                )
                .expect("exact runtime acknowledgement")
            );
        })
        .await
        .expect("exact runtime state");
        assert_eq!(exact_runtime_state.reload_required, Some(false));
        assert!(!exact_runtime_state.runtime_ack_pending);
    }

    #[test]
    fn empty_bundle_directory_with_installation_evidence_is_restored_not_reinstalled() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("initial acknowledgement")
        );
        fs::remove_dir_all(&destination).expect("remove bundle");
        fs::create_dir(&destination).expect("empty bundle directory");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("restore empty bundle");

        assert_eq!(outcome, InstallOutcome::Restored);
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("restore marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::Restore)
        );
    }

    #[test]
    fn legacy_pairing_secret_restores_missing_or_empty_bundle_without_a_marker() {
        for secret_contents in ["a".repeat(64), "short".into()] {
            for empty_destination in [false, true] {
                let temp = tempfile::tempdir().expect("temp dir");
                let config_dir = temp.path().join("config");
                let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
                fs::create_dir(&config_dir).expect("config dir");
                super::atomic_write_private_file(
                    &config_dir.join(super::BRIDGE_SECRET_FILE),
                    secret_contents.as_bytes(),
                    "legacy pairing secret",
                )
                .expect("legacy pairing secret");
                if empty_destination {
                    fs::create_dir(&destination).expect("empty destination");
                }

                let outcome = install_bundle(&destination, &config_dir, empty_destination)
                    .expect("restore legacy installation");

                assert_eq!(outcome, InstallOutcome::Restored);
                assert_eq!(
                    reload_pending_at(&config_dir)
                        .expect("restore marker")
                        .map(|marker| marker.activation),
                    Some(PendingActivation::Restore)
                );
                let persisted = read_bridge_secret(&config_dir.join(super::BRIDGE_SECRET_FILE))
                    .expect("repaired secret must be readable")
                    .expect("repaired secret must exist");
                assert_ne!(
                    persisted, secret_contents,
                    "restoring a missing bundle must rotate every legacy pairing"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn initial_marker_commit_failure_rolls_back_the_installed_bundle() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        fs::create_dir(&config_dir).expect("config dir");
        let outside = temp.path().join("outside-marker");
        fs::write(&outside, "do not replace").expect("outside marker");
        symlink(&outside, config_dir.join(super::RELOAD_PENDING_FILE)).expect("pending symlink");

        let error = install_bundle(&destination, &config_dir, false)
            .expect_err("marker commit must fail closed");

        assert!(error.to_string().contains("symbolic link"));
        assert!(!destination.exists());
        assert_eq!(
            fs::read_to_string(&outside).expect("outside marker preserved"),
            "do not replace"
        );
    }

    #[cfg(unix)]
    #[test]
    fn installed_bundle_and_pairing_material_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);

        install_bundle(&destination, &config_dir, false).expect("initial install");
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("activation marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::LoadUnpacked)
        );

        assert_eq!(
            fs::metadata(&destination)
                .expect("bundle metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(destination.join("config.js"))
                .expect("rendered config metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(config_dir.join("browser-extension-secret"))
                .expect("pairing secret metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(config_dir.join(super::INSTALL_LOCK_FILE))
                .expect("install lock metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        for marker in [super::INSTALLATION_MARKER_FILE, super::RELOAD_PENDING_FILE] {
            assert_eq!(
                fs::metadata(config_dir.join(marker))
                    .expect("private marker metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn installed_bundle_and_all_pairing_material_have_protected_windows_dacls() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);

        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        mark_reload_pending_locked(
            &config_dir,
            super::BROWSER_BRIDGE_RUNTIME_BUILD,
            secret.trim(),
        )
        .expect("reload-pending marker");

        for directory in [&config_dir, &destination] {
            super::permissions::assert_private_acl(directory, true);
        }
        for file in [
            destination.join("config.js"),
            config_dir.join(super::BRIDGE_SECRET_FILE),
            config_dir.join(super::INSTALLATION_MARKER_FILE),
            config_dir.join(super::RELOAD_PENDING_FILE),
            config_dir.join(super::INSTALL_LOCK_FILE),
        ] {
            super::permissions::assert_private_acl(&file, false);
        }
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_temp_and_open_options_handles_can_be_reopened_for_acl_hardening() {
        let temp = tempfile::tempdir().expect("temp dir");
        let temporary = tempfile::Builder::new()
            .prefix(".ordinary-handle-")
            .tempfile_in(temp.path())
            .expect("ordinary temporary file");
        super::permissions::harden_private_file_handle(temporary.as_file(), temporary.path())
            .expect("harden ordinary file handle");
        super::permissions::assert_private_acl(temporary.path(), false);

        let opened_path = temp.path().join("open-options-handle");
        let opened = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&opened_path)
            .expect("ordinary OpenOptions file");
        super::permissions::harden_private_file_handle(&opened, &opened_path)
            .expect("harden OpenOptions file handle");
        super::permissions::assert_private_acl(&opened_path, false);
    }

    #[cfg(windows)]
    #[test]
    fn already_current_install_removes_an_explicit_everyone_ace_from_reload_marker() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);

        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        mark_reload_pending_locked(
            &config_dir,
            super::BROWSER_BRIDGE_RUNTIME_BUILD,
            secret.trim(),
        )
        .expect("reload-pending marker");
        let marker = config_dir.join(super::RELOAD_PENDING_FILE);
        super::permissions::make_world_readable_for_test(&marker, false);

        assert_eq!(
            install_bundle(&destination, &config_dir, true).expect("repair current install"),
            InstallOutcome::AlreadyCurrent
        );
        super::permissions::assert_private_acl(&marker, false);
    }

    #[cfg(windows)]
    #[test]
    fn install_lock_holds_the_same_config_directory_against_replacement() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let replacement = temp.path().join("replacement");
        let install_lock = BrowserExtensionInstallLock::acquire(&config_dir).expect("install lock");

        assert_eq!(
            super::windows_directory_identity_from_handle(&install_lock._config_dir, &config_dir)
                .expect("held directory identity"),
            install_lock.config_dir_identity
        );
        assert!(
            fs::rename(&config_dir, &replacement).is_err(),
            "the config directory was replaceable while its install lock was held"
        );

        drop(install_lock);
        fs::rename(&config_dir, &replacement).expect("rename after releasing directory handle");
    }

    #[test]
    fn explicit_destination_allows_only_the_managed_default_without_creating_state() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let default_destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        let custom_destination = temp.path().join("custom-extension");

        assert_eq!(
            super::resolve_install_destination(
                Some(default_destination.display().to_string()),
                &config_dir,
            )
            .expect("explicit default destination"),
            default_destination
        );
        let error = super::resolve_install_destination(
            Some(custom_destination.display().to_string()),
            &config_dir,
        )
        .expect_err("unsafe custom destination must fail closed");
        assert!(error.to_string().contains("no longer supported"));
        assert!(!custom_destination.exists());
        assert!(!config_dir.exists());
        assert!(!config_dir.join(super::BRIDGE_SECRET_FILE).exists());
        assert!(!config_dir.join(super::INSTALLATION_MARKER_FILE).exists());
        assert!(!config_dir.join(super::RELOAD_PENDING_FILE).exists());
        assert!(!config_dir.join(super::INSTALL_LOCK_FILE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn update_repairs_legacy_permissions_and_rotates_the_exposed_secret() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let original_secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("original secret");
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
            .expect("legacy directory permissions");
        fs::set_permissions(
            destination.join("config.js"),
            fs::Permissions::from_mode(0o644),
        )
        .expect("legacy rendered config permissions");
        fs::set_permissions(
            config_dir.join("browser-extension-secret"),
            fs::Permissions::from_mode(0o644),
        )
        .expect("legacy secret permissions");
        fs::set_permissions(
            config_dir.join(super::INSTALL_LOCK_FILE),
            fs::Permissions::from_mode(0o644),
        )
        .expect("legacy install lock permissions");
        fs::write(
            destination.join("service-worker.js"),
            format!("{SERVICE_WORKER}\n// trigger secure update"),
        )
        .expect("stale bundle");

        install_bundle(&destination, &config_dir, true).expect("secure update");

        let rotated_secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("rotated secret");
        assert_ne!(rotated_secret, original_secret);
        assert_eq!(
            fs::metadata(&destination)
                .expect("bundle metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(destination.join("config.js"))
                .expect("rendered config metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(config_dir.join("browser-extension-secret"))
                .expect("pairing secret metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(config_dir.join(super::INSTALL_LOCK_FILE))
                .expect("install lock metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn every_pairing_permission_exposure_fails_the_real_gate_and_force_rotates() {
        use std::os::unix::fs::PermissionsExt;

        for exposed_component in ["config_dir", "secret", "bundle_dir", "bundle_config"] {
            let temp = tempfile::tempdir().expect("temp dir");
            let config_dir = temp.path().join("config");
            let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
            install_bundle(&destination, &config_dir, false).expect("initial install");
            let original_secret =
                fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
            assert!(
                acknowledge_runtime_build_at(
                    &config_dir,
                    super::BROWSER_BRIDGE_RUNTIME_BUILD,
                    original_secret.trim(),
                )
                .expect("initial acknowledgement")
            );

            let (path, mode) = match exposed_component {
                "config_dir" => (config_dir.clone(), 0o775),
                "secret" => (config_dir.join(super::BRIDGE_SECRET_FILE), 0o644),
                "bundle_dir" => (destination.clone(), 0o755),
                "bundle_config" => (destination.join("config.js"), 0o644),
                _ => unreachable!(),
            };
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))
                .expect("expose pairing fixture");

            assert_eq!(
                bridge_pairing_status_at(&config_dir),
                BridgePairingStatus::Exposed,
                "{exposed_component} exposure must be detected"
            );
            assert!(
                super::trusted_bridge_secret_at(&config_dir).is_err(),
                "{exposed_component} exposure must block the real challenge secret"
            );
            assert!(
                super::bridge_is_configured_at(&config_dir).is_err(),
                "{exposed_component} exposure must not be treated as ready"
            );

            assert_eq!(
                install_bundle(&destination, &config_dir, true).expect("repair exposure"),
                InstallOutcome::Updated
            );
            let rotated_secret =
                fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("rotated");
            assert_ne!(
                rotated_secret.trim(),
                original_secret.trim(),
                "{exposed_component} exposure must invalidate the old capability"
            );
            assert_eq!(
                bridge_pairing_status_at(&config_dir),
                BridgePairingStatus::Present
            );
            assert!(
                !acknowledge_runtime_build_at(
                    &config_dir,
                    super::BROWSER_BRIDGE_RUNTIME_BUILD,
                    original_secret.trim(),
                )
                .expect("old capability rejection")
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_extended_acl_exposure_rotates_instead_of_reusing_the_secret() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret_path = config_dir.join(super::BRIDGE_SECRET_FILE);
        let original_secret = fs::read_to_string(&secret_path).expect("secret");
        let status = Command::new("/bin/chmod")
            .args(["+a", "everyone allow read"])
            .arg(&secret_path)
            .status()
            .expect("run chmod +a");
        assert!(status.success());

        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::Exposed
        );
        assert!(super::trusted_bridge_secret_at(&config_dir).is_err());

        install_bundle(&destination, &config_dir, true).expect("repair ACL exposure");
        let rotated_secret = fs::read_to_string(&secret_path).expect("rotated secret");
        assert_ne!(rotated_secret.trim(), original_secret.trim());
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::Present
        );
    }

    #[test]
    fn force_refuses_a_byte_modified_pre_sentinel_bundle() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        assert_eq!(
            install_bundle(&destination, &config_dir, false).expect("initial install"),
            InstallOutcome::Installed
        );
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("loaded runtime acknowledgement")
        );
        fs::remove_file(destination.join(MANAGED_SENTINEL)).expect("remove modern sentinel");
        fs::write(
            destination.join("service-worker.js"),
            format!("{SERVICE_WORKER}\n// historical Sunox service worker"),
        )
        .expect("historical asset");

        let error = install_bundle(&destination, &config_dir, true)
            .expect_err("byte-modified bundle must not be adopted");

        assert!(error.to_string().contains("not a Sunox-managed"));
        assert!(!destination.join(MANAGED_SENTINEL).exists());
        assert!(
            fs::read_to_string(destination.join("service-worker.js"))
                .expect("modified service worker")
                .contains("historical Sunox service worker")
        );
    }

    #[test]
    fn force_is_a_noop_when_the_generated_bundle_is_current() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        assert_eq!(
            install_bundle(&destination, &config_dir, false).expect("initial install"),
            InstallOutcome::Installed
        );
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("loaded runtime acknowledgement")
        );

        let outcome = install_bundle(&destination, &config_dir, true).expect("idempotent update");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        let runtime_state = outcome.runtime_state(
            reload_pending_at(&config_dir)
                .expect("reload state")
                .map(|marker| marker.activation),
        );
        assert_eq!(runtime_state.reload_required, Some(false));
        assert!(!runtime_state.runtime_ack_pending);
        assert!(
            super::runtime_ack_matches_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("durable acknowledgement")
        );
    }

    #[test]
    fn current_files_without_pending_or_positive_ack_return_to_unknown() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        super::remove_private_state_file(
            &config_dir.join(super::RELOAD_PENDING_FILE),
            "test pending marker",
        )
        .expect("simulate a lost pending marker");
        assert!(
            super::runtime_ack_at(&config_dir)
                .expect("ack state")
                .is_none()
        );

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("conservative recheck");
        let pending = reload_pending_at(&config_dir)
            .expect("pending state")
            .expect("missing acknowledgement must recreate pending");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert_eq!(pending.activation, PendingActivation::Restore);
        let state = outcome.runtime_state(Some(pending.activation));
        assert_eq!(state.reload_required, None);
        assert!(state.runtime_ack_pending);
    }

    #[test]
    fn exact_positive_ack_survives_a_missing_pending_marker() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("authenticated runtime")
        );
        assert!(
            reload_pending_at(&config_dir)
                .expect("pending state")
                .is_none()
        );

        let outcome = install_bundle(&destination, &config_dir, true).expect("idempotent check");
        let state = outcome.runtime_state(
            reload_pending_at(&config_dir)
                .expect("pending state")
                .map(|marker| marker.activation),
        );

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert_eq!(state.reload_required, Some(false));
        assert!(!state.runtime_ack_pending);
    }

    #[test]
    fn stale_or_wrong_build_ack_cannot_make_current_files_ready() {
        for (build_id, fingerprint) in [
            ("0.0.1".to_string(), "a".repeat(64)),
            (
                super::BROWSER_BRIDGE_RUNTIME_BUILD.to_string(),
                "b".repeat(64),
            ),
        ] {
            let temp = tempfile::tempdir().expect("temp dir");
            let config_dir = temp.path().join("config");
            let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
            install_bundle(&destination, &config_dir, false).expect("initial install");
            super::remove_private_state_file(
                &config_dir.join(super::RELOAD_PENDING_FILE),
                "test pending marker",
            )
            .expect("remove pending");
            super::atomic_write_private_file(
                &config_dir.join(super::RUNTIME_ACK_FILE),
                format!("schema=1\nruntime_build={build_id}\nsecret_fingerprint={fingerprint}\n")
                    .as_bytes(),
                "test runtime acknowledgement",
            )
            .expect("stale ack fixture");

            let outcome =
                install_bundle(&destination, &config_dir, true).expect("reject stale ack");

            assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
            assert!(
                reload_pending_at(&config_dir)
                    .expect("pending state")
                    .is_some(),
                "stale ACK must be replaced by conservative pending evidence"
            );
            assert!(
                super::runtime_ack_at(&config_dir)
                    .expect("ack state")
                    .is_none(),
                "pending write must invalidate stale positive evidence"
            );
        }
    }

    #[test]
    fn no_pending_wrong_build_runtime_cannot_create_positive_ack() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
        super::remove_private_state_file(
            &config_dir.join(super::RELOAD_PENDING_FILE),
            "test pending marker",
        )
        .expect("remove pending");

        assert!(
            !acknowledge_runtime_build_at(&config_dir, "0.0.1", secret.trim())
                .expect("wrong runtime result")
        );
        assert!(
            super::runtime_ack_at(&config_dir)
                .expect("ack state")
                .is_none()
        );
    }

    #[test]
    fn acknowledged_install_updates_through_the_reload_pending_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("initial runtime acknowledgement")
        );
        fs::write(
            destination.join("service-worker.js"),
            format!("{SERVICE_WORKER}\n// next runtime"),
        )
        .expect("stale runtime fixture");

        let outcome = install_bundle(&destination, &config_dir, true).expect("runtime update");
        let pending = reload_pending_at(&config_dir).expect("reload marker");
        let runtime_state = outcome.runtime_state(pending.map(|marker| marker.activation));

        assert_eq!(outcome, InstallOutcome::Updated);
        assert_eq!(
            runtime_state.pending_origin,
            Some(PendingActivation::Reload)
        );
        assert_eq!(runtime_state.reload_required, Some(true));
        assert_eq!(outcome.status(runtime_state), "reload_pending");
        assert_eq!(
            activation_guidance(outcome, runtime_state),
            (Some("reload"), vec!["reload"])
        );
    }

    #[test]
    fn exact_known_dev_manifest_is_a_noop_but_reserialized_json_is_an_update() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        fs::write(
            destination.join("manifest.json"),
            render_manifest_with_version_name("0.2.0"),
        )
        .expect("known development manifest");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("known manifest recheck");
        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert_eq!(
            fs::read_to_string(destination.join("manifest.json")).expect("preserved manifest"),
            render_manifest_with_version_name("0.2.0")
        );

        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(destination.join("manifest.json")).expect("development manifest"),
        )
        .expect("valid manifest");
        fs::write(
            destination.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("reserialized manifest"),
        )
        .expect("write reserialized manifest");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("reserialized manifest update");
        assert_eq!(outcome, InstallOutcome::Updated);
        assert!(
            reload_pending_at(&config_dir)
                .expect("reload marker")
                .is_some()
        );
        assert_eq!(
            fs::read_to_string(destination.join("manifest.json")).expect("normalized manifest"),
            render_manifest()
        );
    }

    #[test]
    fn a_current_legacy_bundle_only_gains_the_sentinel_and_needs_no_reload() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("loaded runtime acknowledgement")
        );
        fs::remove_file(destination.join(MANAGED_SENTINEL)).expect("remove modern sentinel");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("adopt current legacy bundle");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        let runtime_state = outcome.runtime_state(
            reload_pending_at(&config_dir)
                .expect("reload state")
                .map(|marker| marker.activation),
        );
        assert_eq!(runtime_state.reload_required, Some(false));
        assert!(!runtime_state.runtime_ack_pending);
        assert!(destination.join(MANAGED_SENTINEL).is_file());
    }

    #[test]
    fn update_rotates_secret_and_only_the_matching_runtime_pairing_clears_reload_pending() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let original_secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("original secret");
        fs::write(
            destination.join("service-worker.js"),
            format!("{SERVICE_WORKER}\n// trigger update"),
        )
        .expect("stale bundle");
        let outcome = install_bundle(&destination, &config_dir, true).expect("update bundle");
        let rotated_secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("rotated secret");
        let generated_config =
            fs::read_to_string(destination.join("config.js")).expect("updated generated config");

        assert_ne!(rotated_secret, original_secret);
        assert!(generated_config.contains(rotated_secret.trim()));
        assert!(!generated_config.contains(original_secret.trim()));
        let runtime_state = outcome.runtime_state(
            reload_pending_at(&config_dir)
                .expect("pending state")
                .map(|marker| marker.activation),
        );
        assert_eq!(outcome, InstallOutcome::Updated);
        assert_eq!(outcome.status(runtime_state), "activation_pending");
        assert_eq!(runtime_state.reload_required, None);
        assert_eq!(
            activation_guidance(outcome, runtime_state),
            (
                Some("ensure_loaded"),
                vec!["load_unpacked_if_missing", "enable_and_reload_if_present"]
            )
        );
        assert_eq!(
            reload_pending_at(&config_dir).expect("pending marker"),
            Some(super::ReloadPendingMarker {
                runtime_build: super::BROWSER_BRIDGE_RUNTIME_BUILD.to_string(),
                secret_fingerprint: Some(secret_fingerprint(rotated_secret.trim())),
                activation: PendingActivation::LoadUnpacked,
            })
        );
        assert!(
            !acknowledge_runtime_build_at(&config_dir, "0.3.5", rotated_secret.trim())
                .expect("old runtime acknowledgement")
        );
        assert!(
            reload_pending_at(&config_dir)
                .expect("pending remains")
                .is_some()
        );
        assert!(
            !acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                original_secret.trim()
            )
            .expect("old pairing acknowledgement")
        );
        assert!(
            reload_pending_at(&config_dir)
                .expect("pending survives old pairing")
                .is_some()
        );
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                rotated_secret.trim()
            )
            .expect("current runtime acknowledgement")
        );
        assert_eq!(
            reload_pending_at(&config_dir).expect("pending cleared"),
            None
        );
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                rotated_secret.trim()
            )
            .expect("idempotent acknowledgement remains valid")
        );
    }

    #[test]
    fn already_current_install_advances_a_stale_pending_build_identity() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("pairing secret");
        mark_reload_pending_locked(&config_dir, "0.3.5", secret.trim())
            .expect("stale pending marker");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("current bundle recheck");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert_eq!(
            reload_pending_at(&config_dir).expect("advanced marker"),
            Some(super::ReloadPendingMarker {
                runtime_build: super::BROWSER_BRIDGE_RUNTIME_BUILD.to_string(),
                secret_fingerprint: Some(secret_fingerprint(secret.trim())),
                activation: PendingActivation::Reload,
            })
        );
    }

    #[test]
    fn legacy_one_line_reload_marker_remains_acknowledgeable() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
        super::atomic_write_private_file(
            &config_dir.join(super::RELOAD_PENDING_FILE),
            format!("{}\n", super::BROWSER_BRIDGE_RUNTIME_BUILD).as_bytes(),
            "legacy pending marker",
        )
        .expect("legacy marker");

        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim()
            )
            .expect("legacy acknowledgement")
        );
        assert_eq!(
            reload_pending_at(&config_dir).expect("legacy marker cleared"),
            None
        );
    }

    #[test]
    fn schema_one_pending_marker_migrates_as_reload_origin() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        let secret = "a".repeat(64);
        super::atomic_write_private_file(
            &config_dir.join(super::RELOAD_PENDING_FILE),
            format!(
                "schema=1\nruntime_build={}\nsecret_fingerprint={}\n",
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret_fingerprint(&secret)
            )
            .as_bytes(),
            "schema-one pending marker",
        )
        .expect("schema one marker");

        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("legacy marker")
                .map(|marker| marker.activation),
            Some(PendingActivation::Reload)
        );
    }

    #[test]
    fn failed_bundle_swap_conservatively_keeps_reload_pending() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        fs::create_dir(&config_dir).expect("config dir");
        fs::create_dir(&destination).expect("destination");
        fs::write(destination.join("old"), "old").expect("old bundle");
        let previous_snapshot =
            capture_stable_directory_snapshot(&destination).expect("previous snapshot");
        let staged = temp.path().join("staged");
        fs::create_dir(&staged).expect("staged");
        fs::write(staged.join("new"), "new").expect("new bundle");
        let staged_snapshot = capture_stable_directory_snapshot(&staged).expect("staged snapshot");
        let backup = temp.path().join("backup");
        let failed = temp.path().join("failed");
        let secret = "a".repeat(64);
        mark_reload_pending_locked(&config_dir, super::BROWSER_BRIDGE_RUNTIME_BUILD, &secret)
            .expect("pending marker");

        let error = replace_directory_paths(
            &staged,
            &destination,
            &backup,
            &failed,
            Some(&previous_snapshot),
            &staged_snapshot,
            |_, _| Err(std::io::Error::other("swap failed")),
            super::directory_matches_snapshot,
            |_, _| Ok(()),
            || Ok(()),
        )
        .expect_err("injected swap failure");

        assert!(error.to_string().contains("swap failed"));
        assert_eq!(
            reload_pending_at(&config_dir).expect("conservative marker"),
            Some(super::ReloadPendingMarker {
                runtime_build: super::BROWSER_BRIDGE_RUNTIME_BUILD.to_string(),
                secret_fingerprint: Some(secret_fingerprint(&secret)),
                activation: PendingActivation::Reload,
            })
        );
    }

    #[test]
    fn bridge_secret_distinguishes_missing_invalid_and_unreadable_paths() {
        let temp = tempfile::tempdir().expect("temp dir");
        let missing = temp.path().join("missing-secret");
        assert_eq!(
            read_bridge_secret(&missing).expect("missing is allowed"),
            None
        );

        let invalid = temp.path().join("invalid-secret");
        fs::write(&invalid, "short").expect("invalid secret fixture");
        assert!(read_bridge_secret(&invalid).is_err());

        let not_a_file = temp.path().join("secret-directory");
        fs::create_dir(&not_a_file).expect("secret directory fixture");
        assert!(read_bridge_secret(&not_a_file).is_err());
    }

    #[test]
    fn pairing_status_distinguishes_repairable_values_from_unsafe_entries() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        let secret_path = config_dir.join(super::BRIDGE_SECRET_FILE);

        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::Missing
        );
        super::atomic_write_private_file(&secret_path, b"short", "repairable corrupt secret")
            .expect("repairable corrupt secret");
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::Corrupt
        );
        super::atomic_write_private_file(&secret_path, &[0xff, 0xfe], "non-UTF8 secret")
            .expect("non-UTF8 secret");
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::UnsafeOrInaccessible
        );
        super::atomic_write_private_file(&secret_path, "a".repeat(64).as_bytes(), "valid secret")
            .expect("valid secret");
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::BundleMissing
        );
        fs::remove_file(&secret_path).expect("remove secret");
        fs::create_dir(&secret_path).expect("directory placeholder");
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::UnsafeOrInaccessible
        );
    }

    #[cfg(unix)]
    #[test]
    fn pairing_status_rejects_symlinked_and_unreadable_secret_entries() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        let secret_path = config_dir.join(super::BRIDGE_SECRET_FILE);
        let outside = temp.path().join("outside-secret");
        fs::write(&outside, "a".repeat(64)).expect("outside secret");
        symlink(&outside, &secret_path).expect("secret symlink");
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::UnsafeOrInaccessible
        );

        fs::remove_file(&secret_path).expect("remove symlink");
        fs::write(&secret_path, "a".repeat(64)).expect("secret");
        fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o000))
            .expect("make secret unreadable");
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(
                bridge_pairing_status_at(&config_dir),
                BridgePairingStatus::UnsafeOrInaccessible
            );
        }
        fs::set_permissions(&secret_path, fs::Permissions::from_mode(0o600))
            .expect("restore secret permissions");
    }

    #[test]
    fn installation_evidence_distinguishes_new_custom_and_legacy_default_installs() {
        let temp = tempfile::tempdir().expect("temp dir");
        let clean_config = temp.path().join("clean-config");
        assert!(!installation_evidence_at(&clean_config).expect("new install state"));

        let custom_config = temp.path().join("custom-config");
        let custom_destination = temp.path().join("custom-extension");
        install_bundle(&custom_destination, &custom_config, false).expect("custom install");
        fs::remove_file(custom_config.join(super::BRIDGE_SECRET_FILE)).expect("remove secret");
        assert!(
            installation_evidence_at(&custom_config)
                .expect("custom install marker survives a missing secret")
        );

        let legacy_config = temp.path().join("legacy-config");
        let default_destination = legacy_config.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&default_destination, &legacy_config, false).expect("default install");
        fs::remove_file(legacy_config.join(super::BRIDGE_SECRET_FILE)).expect("remove secret");
        fs::remove_file(legacy_config.join(super::INSTALLATION_MARKER_FILE))
            .expect("remove modern marker");
        assert!(
            installation_evidence_at(&legacy_config)
                .expect("managed default bundle is legacy installation evidence")
        );
    }

    #[test]
    fn corrupt_owned_installation_marker_is_conservative_and_force_rebuilds_it() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        fs::create_dir(&config_dir).expect("config dir");
        super::atomic_write_private_file(
            &config_dir.join(super::INSTALLATION_MARKER_FILE),
            b"corrupt",
            "corrupt installation marker",
        )
        .expect("corrupt marker");

        assert!(
            installation_evidence_at(&config_dir)
                .expect("owned regular marker remains historical evidence")
        );
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::PairingMissing
        );

        assert_eq!(
            install_bundle(&destination, &config_dir, true).expect("force rebuild marker"),
            InstallOutcome::Restored
        );
        assert_eq!(
            fs::read_to_string(config_dir.join(super::INSTALLATION_MARKER_FILE))
                .expect("rebuilt installation marker"),
            super::INSTALLATION_MARKER_CONTENT
        );
    }

    #[test]
    fn corrupt_runtime_ack_marker_fails_closed_without_false_force_promise() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        super::atomic_write_private_file(
            &config_dir.join(super::RELOAD_PENDING_FILE),
            b"schema=broken",
            "corrupt runtime marker",
        )
        .expect("corrupt runtime marker");

        let error = reload_pending_at(&config_dir).expect_err("corrupt marker must fail closed");

        assert!(error.to_string().contains("runtime-ack marker"));
        assert!(error.to_string().contains("will not"));
        assert!(error.to_string().contains("only after verifying"));
    }

    #[cfg(unix)]
    #[test]
    fn exposed_state_markers_never_create_positive_readiness() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");

        let pending_path = config_dir.join(super::RELOAD_PENDING_FILE);
        fs::set_permissions(&pending_path, fs::Permissions::from_mode(0o644))
            .expect("expose pending marker");
        assert_eq!(
            super::reload_pending_state_at(&config_dir).expect("pending security state"),
            super::ReloadPendingState::Exposed
        );
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("conservative pending")
                .map(|marker| marker.activation),
            Some(PendingActivation::Restore)
        );
        assert!(
            !acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("exposed pending must reject ACK")
        );
        install_bundle(&destination, &config_dir, true).expect("rewrite pending safely");
        assert_eq!(
            fs::metadata(&pending_path)
                .expect("pending metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(
            super::runtime_ack_at(&config_dir)
                .expect("ACK state")
                .is_none()
        );

        let installation_marker = config_dir.join(super::INSTALLATION_MARKER_FILE);
        fs::set_permissions(&installation_marker, fs::Permissions::from_mode(0o644))
            .expect("expose installation marker");
        assert!(
            !super::installation_marker_recorded_at(&config_dir)
                .expect("exposed installation evidence is ignored")
        );
    }

    #[cfg(unix)]
    #[test]
    fn exposed_positive_ack_is_treated_as_absent_and_replaced_by_pending() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("initial ACK")
        );
        let ack_path = config_dir.join(super::RUNTIME_ACK_FILE);
        fs::set_permissions(&ack_path, fs::Permissions::from_mode(0o644))
            .expect("expose positive ACK");

        let outcome = install_bundle(&destination, &config_dir, true).expect("safe recheck");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert!(
            super::runtime_ack_at(&config_dir)
                .expect("ACK state")
                .is_none()
        );
        assert_eq!(
            reload_pending_at(&config_dir)
                .expect("pending state")
                .map(|marker| marker.activation),
            Some(PendingActivation::Restore)
        );
    }

    #[test]
    fn missing_or_modified_generated_assets_block_ack_and_are_force_repaired() {
        for mutation in ["missing_service_worker", "modified_manifest"] {
            let temp = tempfile::tempdir().expect("temp dir");
            let config_dir = temp.path().join("config");
            let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
            install_bundle(&destination, &config_dir, false).expect("initial install");
            let original_secret =
                fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
            assert!(
                acknowledge_runtime_build_at(
                    &config_dir,
                    super::BROWSER_BRIDGE_RUNTIME_BUILD,
                    original_secret.trim(),
                )
                .expect("initial ACK")
            );
            match mutation {
                "missing_service_worker" => {
                    fs::remove_file(destination.join("service-worker.js"))
                        .expect("remove generated asset");
                }
                "modified_manifest" => {
                    fs::write(destination.join("manifest.json"), b"{}")
                        .expect("modify generated manifest");
                }
                _ => unreachable!(),
            }

            assert_eq!(
                bridge_pairing_status_at(&config_dir),
                BridgePairingStatus::BundleCorrupt
            );
            assert!(super::trusted_bridge_secret_at(&config_dir).is_err());
            assert!(
                !acknowledge_runtime_build_at(
                    &config_dir,
                    super::BROWSER_BRIDGE_RUNTIME_BUILD,
                    original_secret.trim(),
                )
                .expect("corrupt bundle must not ACK")
            );

            assert_eq!(
                install_bundle(&destination, &config_dir, true).expect("repair generated bundle"),
                InstallOutcome::Updated
            );
            assert_eq!(
                bridge_pairing_status_at(&config_dir),
                BridgePairingStatus::Present
            );
            let rotated =
                fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("rotated");
            assert_ne!(rotated.trim(), original_secret.trim());
            assert!(
                reload_pending_at(&config_dir)
                    .expect("pending state")
                    .is_some()
            );
        }
    }

    #[test]
    fn invalid_reserved_default_bundle_path_fails_closed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(
            config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY),
            "not a managed extension",
        )
        .expect("invalid reserved path");

        let error = installation_evidence_at(&config_dir)
            .expect_err("invalid reserved path must fail closed");

        assert!(error.to_string().contains("reserved Browser Bridge path"));
    }

    #[cfg(unix)]
    #[test]
    fn install_refuses_to_follow_a_secret_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        let target = temp.path().join("sensitive");
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(&target, "a".repeat(64)).expect("symlink target");
        symlink(&target, config_dir.join("browser-extension-secret")).expect("secret symlink");

        let error = install_bundle(&destination, &config_dir, false)
            .expect_err("secret symlink must be rejected");

        assert!(error.to_string().contains("symbolic link"));
        assert_eq!(
            fs::read_to_string(&target).expect("target remains readable"),
            "a".repeat(64)
        );
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn installation_marker_symlink_fails_closed() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let target = temp.path().join("target");
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(&target, super::INSTALLATION_MARKER_CONTENT).expect("marker target");
        symlink(&target, config_dir.join(super::INSTALLATION_MARKER_FILE)).expect("marker symlink");

        let error =
            installation_evidence_at(&config_dir).expect_err("marker symlink must fail closed");

        assert!(error.to_string().contains("symbolic link"));
    }

    #[cfg(unix)]
    #[test]
    fn reload_pending_marker_symlink_fails_closed() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        let target = temp.path().join("sensitive");
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("secret");
        super::remove_private_state_file(
            &config_dir.join(super::RELOAD_PENDING_FILE),
            "test pending marker",
        )
        .expect("remove original pending");
        fs::write(&target, super::BROWSER_BRIDGE_RUNTIME_BUILD).expect("symlink target");
        symlink(&target, config_dir.join(super::RELOAD_PENDING_FILE)).expect("pending symlink");

        let error = reload_pending_at(&config_dir).expect_err("symlink must not be read");
        assert!(error.to_string().contains("symbolic link"));
        assert!(
            !acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                secret.trim(),
            )
            .expect("unsafe pending state must reject acknowledgement")
        );
        assert_eq!(
            bridge_pairing_status_at(&config_dir),
            BridgePairingStatus::UnsafeOrInaccessible
        );
        assert_eq!(
            fs::read_to_string(&target).expect("target remains"),
            super::BROWSER_BRIDGE_RUNTIME_BUILD
        );
    }

    #[test]
    fn cross_process_install_worker() {
        if std::env::var_os("SUNOX_TEST_BROWSER_EXTENSION_WORKER").is_none() {
            return;
        }
        let destination = PathBuf::from(
            std::env::var_os("SUNOX_TEST_BROWSER_EXTENSION_DESTINATION")
                .expect("worker destination"),
        );
        let config_dir = PathBuf::from(
            std::env::var_os("SUNOX_TEST_BROWSER_EXTENSION_CONFIG").expect("worker config"),
        );
        let ready =
            PathBuf::from(std::env::var_os("SUNOX_TEST_BROWSER_EXTENSION_READY").expect("ready"));
        let attempting = PathBuf::from(
            std::env::var_os("SUNOX_TEST_BROWSER_EXTENSION_ATTEMPTING").expect("attempting"),
        );
        let go = PathBuf::from(std::env::var_os("SUNOX_TEST_BROWSER_EXTENSION_GO").expect("go"));
        fs::write(&ready, "ready").expect("announce worker readiness");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !go.exists() {
            assert!(Instant::now() < deadline, "worker start barrier timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(attempting, "attempting").expect("announce install attempt");
        install_bundle(&destination, &config_dir, true).expect("worker install");
    }

    #[test]
    fn concurrent_process_installs_keep_secret_and_bundle_config_consistent() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        let destination = config_dir.join(super::DEFAULT_EXTENSION_DIRECTORY);
        let go = temp.path().join("go");
        let ready_one = temp.path().join("ready-one");
        let ready_two = temp.path().join("ready-two");
        let attempting_one = temp.path().join("attempting-one");
        let attempting_two = temp.path().join("attempting-two");
        let guard = BrowserExtensionInstallLock::acquire(&config_dir).expect("hold installer lock");
        let executable = std::env::current_exe().expect("current test executable");
        let spawn_worker = |ready: &Path, attempting: &Path| {
            Command::new(&executable)
                .arg("--exact")
                .arg("commands::browser_extension::tests::cross_process_install_worker")
                .arg("--nocapture")
                .env("SUNOX_TEST_BROWSER_EXTENSION_WORKER", "1")
                .env("SUNOX_TEST_BROWSER_EXTENSION_DESTINATION", &destination)
                .env("SUNOX_TEST_BROWSER_EXTENSION_CONFIG", &config_dir)
                .env("SUNOX_TEST_BROWSER_EXTENSION_READY", ready)
                .env("SUNOX_TEST_BROWSER_EXTENSION_ATTEMPTING", attempting)
                .env("SUNOX_TEST_BROWSER_EXTENSION_GO", &go)
                .spawn()
                .expect("spawn install worker")
        };
        let first = spawn_worker(&ready_one, &attempting_one);
        let second = spawn_worker(&ready_two, &attempting_two);
        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while !ready_one.exists() || !ready_two.exists() {
            assert!(
                Instant::now() < ready_deadline,
                "install workers did not become ready"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(&go, "go").expect("release start barrier");
        let attempt_deadline = Instant::now() + Duration::from_secs(5);
        while !attempting_one.exists() || !attempting_two.exists() {
            assert!(
                Instant::now() < attempt_deadline,
                "install workers did not attempt to acquire the lock"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(100));

        assert!(
            !config_dir.join("browser-extension-secret").exists(),
            "worker wrote the secret before the cross-process lock was released"
        );
        assert!(
            !destination.exists(),
            "worker replaced the bundle before the cross-process lock was released"
        );
        drop(guard);

        for child in [first, second] {
            let output = child.wait_with_output().expect("wait for install worker");
            assert!(
                output.status.success(),
                "install worker failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("installed secret");
        let generated_config =
            fs::read_to_string(destination.join("config.js")).expect("generated config");
        assert!(generated_config.contains(&format!("sharedSecret: \"{}\"", secret.trim())));
    }

    #[test]
    fn file_injected_after_backup_rename_is_restored_and_never_deleted() {
        let temp = tempfile::tempdir().expect("temp dir");
        let staged = temp.path().join("staged");
        let destination = temp.path().join("browser-extension");
        let backup = temp.path().join("browser-extension.backup-known");
        let failed = temp.path().join("browser-extension.failed-known");
        fs::create_dir(&staged).expect("staged");
        fs::write(staged.join("new"), "new").expect("staged file");
        fs::create_dir(&destination).expect("destination");
        fs::write(destination.join("old"), "old").expect("destination file");
        let staged_snapshot = capture_stable_directory_snapshot(&staged).expect("staged snapshot");
        let previous_snapshot =
            capture_stable_directory_snapshot(&destination).expect("previous snapshot");
        let mut validation_call = 0;

        let error = replace_directory_paths(
            &staged,
            &destination,
            &backup,
            &failed,
            Some(&previous_snapshot),
            &staged_snapshot,
            |from, to| fs::rename(from, to),
            |path, snapshot| {
                validation_call += 1;
                if validation_call == 2 {
                    fs::write(path.join("injected-user-data"), "preserve me")
                        .expect("inject concurrent user data");
                }
                super::directory_matches_snapshot(path, snapshot)
            },
            remove_snapshot_tree,
            || Ok(()),
        )
        .expect_err("changed backup must be rolled back");

        assert!(error.to_string().contains("changed during the update"));
        assert_eq!(
            fs::read_to_string(destination.join("injected-user-data"))
                .expect("injected data was restored"),
            "preserve me"
        );
        assert_eq!(
            fs::read_to_string(destination.join("old")).expect("old bundle was restored"),
            "old"
        );
        assert!(!backup.exists());
        assert!(!staged.exists());
    }

    #[test]
    fn file_injected_before_backup_cleanup_is_preserved_and_never_deleted() {
        let temp = tempfile::tempdir().expect("temp dir");
        let staged = temp.path().join("staged");
        let destination = temp.path().join("browser-extension");
        let backup = temp.path().join("browser-extension.backup-known");
        let failed = temp.path().join("browser-extension.failed-known");
        fs::create_dir(&staged).expect("staged");
        fs::write(staged.join("new"), "new").expect("staged file");
        fs::create_dir(&destination).expect("destination");
        fs::write(destination.join("old"), "old").expect("destination file");
        let staged_snapshot = capture_stable_directory_snapshot(&staged).expect("staged snapshot");
        let previous_snapshot =
            capture_stable_directory_snapshot(&destination).expect("previous snapshot");
        let mut validation_call = 0;

        let error = replace_directory_paths(
            &staged,
            &destination,
            &backup,
            &failed,
            Some(&previous_snapshot),
            &staged_snapshot,
            |from, to| fs::rename(from, to),
            |path, snapshot| {
                validation_call += 1;
                if validation_call == 3 {
                    fs::write(path.join("injected-user-data"), "preserve me")
                        .expect("inject concurrent user data before cleanup");
                }
                super::directory_matches_snapshot(path, snapshot)
            },
            remove_snapshot_tree,
            || Ok(()),
        )
        .expect_err("changed backup must be preserved before cleanup");

        assert!(error.to_string().contains("changed before cleanup"));
        assert_eq!(
            fs::read_to_string(destination.join("new")).expect("new bundle remains installed"),
            "new"
        );
        assert_eq!(
            fs::read_to_string(backup.join("injected-user-data"))
                .expect("injected data was preserved"),
            "preserve me"
        );
        assert_eq!(
            fs::read_to_string(backup.join("old")).expect("old bundle was preserved"),
            "old"
        );
    }

    #[test]
    fn replacement_created_after_cleanup_hash_is_never_unlinked() {
        let temp = tempfile::tempdir().expect("temp dir");
        let backup = temp.path().join("browser-extension.backup-known");
        fs::create_dir(&backup).expect("backup");
        fs::write(backup.join("old"), "known old bundle").expect("known file");
        let snapshot = capture_stable_directory_snapshot(&backup).expect("snapshot");
        let mut injected = false;

        let error = remove_snapshot_tree_paths(&backup, &snapshot, |isolated| {
            if !injected {
                assert_ne!(
                    isolated,
                    backup.join("old"),
                    "cleanup must unlink only an atomically isolated entry"
                );
                fs::create_dir(&backup).expect("replacement directory");
                fs::write(backup.join("old"), "preserve replacement").expect("replacement file");
                injected = true;
            }
            fs::remove_file(isolated)
        })
        .expect_err("a replacement cleanup path must be preserved and reported");

        assert!(error.to_string().contains("appeared during cleanup"));
        assert_eq!(
            fs::read_to_string(backup.join("old")).expect("replacement was preserved"),
            "preserve replacement"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_identity_is_stable_across_rename() {
        let temp = tempfile::tempdir().expect("temp dir");
        let before = temp.path().join("before");
        let after = temp.path().join("after");
        fs::create_dir(&before).expect("directory");

        let expected = super::directory_identity(&before).expect("identity before rename");
        fs::rename(&before, &after).expect("rename");

        assert_eq!(
            super::directory_identity(&after).expect("identity after rename"),
            expected
        );
    }

    #[test]
    fn secret_commit_failure_restores_previous_bundle_without_deleting_new_data() {
        let temp = tempfile::tempdir().expect("temp dir");
        let staged = temp.path().join("staged");
        let destination = temp.path().join("browser-extension");
        let backup = temp.path().join("browser-extension.backup-known");
        let failed = temp.path().join("browser-extension.failed-known");
        fs::create_dir(&staged).expect("staged");
        fs::write(staged.join("new"), "new").expect("staged file");
        fs::create_dir(&destination).expect("destination");
        fs::write(destination.join("old"), "old").expect("destination file");
        let staged_snapshot = capture_stable_directory_snapshot(&staged).expect("staged snapshot");
        let previous_snapshot =
            capture_stable_directory_snapshot(&destination).expect("previous snapshot");

        let error = replace_directory_paths(
            &staged,
            &destination,
            &backup,
            &failed,
            Some(&previous_snapshot),
            &staged_snapshot,
            |from, to| fs::rename(from, to),
            super::directory_matches_snapshot,
            remove_snapshot_tree,
            || {
                Err(super::CliError::Config(
                    "injected secret commit failure".into(),
                ))
            },
        )
        .expect_err("secret commit failure must roll the bundle back");

        assert!(error.to_string().contains("injected secret commit failure"));
        assert_eq!(
            fs::read_to_string(destination.join("old")).expect("old bundle restored"),
            "old"
        );
        assert!(!destination.join("new").exists());
        assert!(!backup.exists());
        assert!(!failed.exists());
        assert!(!staged.exists());
    }

    #[test]
    fn failed_install_and_failed_rollback_report_both_errors_and_backup_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let staged = temp.path().join("staged");
        let destination = temp.path().join("browser-extension");
        let backup = temp.path().join("browser-extension.backup-known");
        let failed = temp.path().join("browser-extension.failed-known");
        fs::create_dir(&staged).expect("staged");
        fs::write(staged.join("new"), "new").expect("staged file");
        fs::create_dir(&destination).expect("destination");
        fs::write(destination.join("old"), "old").expect("destination file");
        let staged_snapshot = capture_stable_directory_snapshot(&staged).expect("staged snapshot");
        let previous_snapshot =
            capture_stable_directory_snapshot(&destination).expect("previous snapshot");
        let mut rename_call = 0;

        let error = replace_directory_paths(
            &staged,
            &destination,
            &backup,
            &failed,
            Some(&previous_snapshot),
            &staged_snapshot,
            |_, _| {
                rename_call += 1;
                match rename_call {
                    1 => Ok(()),
                    2 => Err(std::io::Error::other("install rename failed")),
                    3 => Err(std::io::Error::other("rollback rename failed")),
                    _ => unreachable!("unexpected rename"),
                }
            },
            |_, _| Ok(true),
            |_, _| Ok(()),
            || Ok(()),
        )
        .expect_err("double rename failure must remain actionable");

        let message = error.to_string();
        assert!(message.contains("install rename failed"));
        assert!(message.contains("rollback rename failed"));
        assert!(message.contains(&backup.display().to_string()));
    }
}

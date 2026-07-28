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
const BRIDGE_SECRET_FILE: &str = "browser-extension-secret";
const DEFAULT_EXTENSION_DIRECTORY: &str = "browser-extension";
const INSTALL_BEHAVIOR_GUIDANCE: &str = "No Suno tab or browser window is opened. For a required challenge, the bridge creates a nonce-bound Suno iframe inside Chrome's invisible offscreen document, waits for the clean canonical page to stabilize, executes one silent challenge, removes the frame on every terminal path, and fails closed instead of creating a visible or minimized fallback; once paired, auto also fails closed instead of launching an isolated browser.";

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
}

struct LoadedBridgeSecret {
    value: String,
    created: bool,
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
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallOutcome {
    Installed,
    Updated,
    AlreadyCurrent,
}

impl InstallOutcome {
    fn status(self, reload_required: bool) -> &'static str {
        if reload_required {
            return "reload_pending";
        }
        match self {
            Self::Installed => "installed",
            Self::Updated => "updated",
            Self::AlreadyCurrent => "already_current",
        }
    }

    fn reload_required(self, pending: bool) -> bool {
        !matches!(self, Self::Installed) && pending
    }
}

struct BrowserExtensionInstallLock {
    file: File,
    #[cfg(windows)]
    _config_dir: File,
    #[cfg(windows)]
    config_dir_identity: DirectoryIdentity,
}

impl BrowserExtensionInstallLock {
    fn acquire(config_dir: &Path) -> Result<Self, CliError> {
        std::fs::create_dir_all(config_dir)?;
        #[cfg(windows)]
        let config_dir_handle = permissions::open_and_harden_locked_directory(config_dir)?;
        #[cfg(windows)]
        let config_dir_identity =
            windows_directory_identity_from_handle(&config_dir_handle, config_dir)?;
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
        Ok(Self {
            file,
            #[cfg(windows)]
            _config_dir: config_dir_handle,
            #[cfg(windows)]
            config_dir_identity,
        })
    }

    #[cfg(windows)]
    fn verify_config_directory(&self, config_dir: &Path) -> Result<(), CliError> {
        if directory_identity(config_dir)? == self.config_dir_identity {
            return Ok(());
        }
        Err(CliError::Config(format!(
            "the Sunox configuration directory was replaced while acquiring the Browser Bridge install lock at {}",
            config_dir.display()
        )))
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
    let reload_required = outcome.reload_required(reload_pending()?);
    let next_steps = install_next_steps(outcome, reload_required);

    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "installed": true,
            "status": outcome.status(reload_required),
            "path": destination.display().to_string(),
            "reload_required": reload_required,
            "next_steps": next_steps,
        })),
        OutputFormat::Table => {
            match (outcome, reload_required) {
                (InstallOutcome::Installed, _) => {
                    eprintln!(
                        "Extracted the Sunox Browser Bridge to: {}",
                        destination.display()
                    );
                    eprintln!(
                        "Open chrome://extensions, enable Developer mode, choose Load unpacked, and select that directory."
                    );
                }
                (InstallOutcome::Updated, true) => {
                    eprintln!(
                        "Updated the Sunox Browser Bridge at: {} (reload_required=true)",
                        destination.display()
                    );
                    eprintln!(
                        "Open chrome://extensions and click Reload on the existing Sunox Browser Bridge."
                    );
                }
                (InstallOutcome::AlreadyCurrent, true) => {
                    eprintln!(
                        "Sunox Browser Bridge files are current at: {}, but Chrome still needs Reload (reload_required=true)",
                        destination.display()
                    );
                    eprintln!(
                        "Open chrome://extensions and click Reload on the existing Sunox Browser Bridge."
                    );
                }
                (InstallOutcome::AlreadyCurrent, false) => {
                    eprintln!(
                        "Sunox Browser Bridge files are already current at: {} (reload_required=false)",
                        destination.display()
                    );
                    eprintln!("No Chrome reload is required.");
                }
                (InstallOutcome::Updated, false) => {
                    eprintln!(
                        "Updated the Sunox Browser Bridge at: {} (reload_required=false)",
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

fn install_next_steps(outcome: InstallOutcome, reload_required: bool) -> Vec<&'static str> {
    match (outcome, reload_required) {
        (InstallOutcome::Installed, _) => vec![
            "Open chrome://extensions",
            "Enable Developer mode",
            "Choose Load unpacked and select the reported path",
            "No Suno tab or browser window is created",
        ],
        (_, true) => vec![
            "Open chrome://extensions",
            "Click Reload on the existing extension",
            "No Suno tab or browser window is created",
        ],
        (InstallOutcome::AlreadyCurrent, false) => vec![
            "No Chrome reload is required",
            "Keep the extension enabled",
            "No Suno tab or browser window is created",
        ],
        (InstallOutcome::Updated, false) => vec![
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
    reject_destination_symlink(destination)?;
    let (_, locked_config_dir) = resolve_and_validate_install_paths(destination, config_dir)?;
    let _lock = BrowserExtensionInstallLock::acquire(&locked_config_dir)?;
    // Resolve again after acquiring the lock. This closes the window where a
    // path component could be replaced with a symlink while this process was
    // waiting for another installer.
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
    #[cfg(windows)]
    _lock.verify_config_directory(&resolved_config_dir)?;
    let destination = resolved_destination.as_path();

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
        if !force {
            return Err(CliError::Config(format!(
                "{} already exists — pass --force to update it",
                destination.display()
            )));
        }
        if !directory_is_empty(destination)? {
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

    let loaded_secret = load_or_create_secret(&resolved_config_dir)?;
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
                mark_reload_pending_locked(
                    &resolved_config_dir,
                    BROWSER_BRIDGE_RUNTIME_BUILD,
                    &loaded_secret.value,
                )?;
            }
        }
        record_installation_locked(&resolved_config_dir)?;
        return Ok(InstallOutcome::AlreadyCurrent);
    }

    if had_existing_bundle {
        let (install_secret, persist_after_swap) = if loaded_secret.created {
            (loaded_secret.value, false)
        } else {
            (new_bridge_secret(), true)
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
        )?;
    } else {
        replace_directory(staging, destination, existing_snapshot.as_ref(), || Ok(()))?;
    }
    harden_bundle_permissions(destination)?;
    record_installation_locked(&resolved_config_dir)?;
    Ok(if had_existing_bundle {
        InstallOutcome::Updated
    } else {
        InstallOutcome::Installed
    })
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

    if extension_version == Some(BROWSER_BRIDGE_RUNTIME_BUILD)
        && cli_version == Some(env!("CARGO_PKG_VERSION"))
    {
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
                if std::fs::read(directory.join(asset.path))? != render_manifest().as_bytes() {
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
        if std::fs::read(expected.join(&name))? != std::fs::read(actual.join(&name))? {
            return Ok(false);
        }
    }
    Ok(true)
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
        .replace(
            "__SUNOX_BRIDGE_RUNTIME_BUILD__",
            BROWSER_BRIDGE_RUNTIME_BUILD,
        )
        .replace("__SUNOX_BRIDGE_SECRET__", secret)
}

fn render_manifest() -> String {
    MANIFEST
        .replace(
            "__SUNOX_BRIDGE_RUNTIME_BUILD__",
            BROWSER_BRIDGE_RUNTIME_BUILD,
        )
        .replace("__SUNOX_VERSION__", env!("CARGO_PKG_VERSION"))
}

fn load_or_create_secret(config_dir: &Path) -> Result<LoadedBridgeSecret, CliError> {
    std::fs::create_dir_all(config_dir)?;
    #[cfg(windows)]
    permissions::harden_private_directory(config_dir)?;
    let path = config_dir.join(BRIDGE_SECRET_FILE);
    if let Some(secret) = read_file_without_following_symlink(&path, "browser extension secret")? {
        let secret = secret.trim();
        if valid_secret(secret) {
            permissions::harden_private_file(&path)?;
            return Ok(LoadedBridgeSecret {
                value: secret.to_string(),
                created: false,
            });
        }
    }

    let secret = new_bridge_secret();
    atomic_write_private_file(&path, secret.as_bytes(), "browser extension secret")?;
    Ok(LoadedBridgeSecret {
        value: secret,
        created: true,
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
    let secret = match read_file_without_following_symlink(path, "browser extension secret")? {
        Some(secret) => secret,
        None => return Ok(None),
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

fn installation_evidence_at(config_dir: &Path) -> Result<bool, CliError> {
    let marker = config_dir.join(INSTALLATION_MARKER_FILE);
    match read_file_without_following_symlink(&marker, "Browser Bridge installation marker")? {
        Some(contents) if contents == INSTALLATION_MARKER_CONTENT => return Ok(true),
        Some(_) => {
            return Err(CliError::Config(format!(
                "Browser Bridge installation marker at {} is corrupt; run `sunox install-browser-extension --force`",
                marker.display()
            )));
        }
        None => {}
    }

    // v0.1.x did not persist the installation marker. Safely recognize its
    // default managed bundle so an upgraded installation still fails closed
    // if the pairing secret is later lost.
    let default_bundle = config_dir.join(DEFAULT_EXTENSION_DIRECTORY);
    match std::fs::symlink_metadata(&default_bundle) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(CliError::Config(format!(
                "reserved Browser Bridge path {} exists but is not a regular managed directory",
                default_bundle.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(CliError::Config(format!(
                "could not inspect Browser Bridge installation evidence at {}: {error}",
                default_bundle.display()
            )));
        }
    }
    if is_managed_bundle(&default_bundle)? {
        return Ok(true);
    }
    Err(CliError::Config(format!(
        "reserved Browser Bridge path {} exists but its managed installation evidence is invalid; run `sunox install-browser-extension --force`",
        default_bundle.display()
    )))
}

pub(crate) fn bridge_secret() -> Result<Option<String>, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(None);
    };
    let path = config_dir.join(BRIDGE_SECRET_FILE);
    read_bridge_secret(&path)
}

pub(crate) fn bridge_is_configured() -> Result<bool, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(false);
    };
    if read_bridge_secret(&config_dir.join(BRIDGE_SECRET_FILE))?.is_some() {
        return Ok(true);
    }
    installation_evidence_at(&config_dir)
}

fn reload_pending_at(config_dir: &Path) -> Result<Option<ReloadPendingMarker>, CliError> {
    let path = config_dir.join(RELOAD_PENDING_FILE);
    let Some(contents) =
        read_file_without_following_symlink(&path, "Browser Bridge reload-pending marker")?
    else {
        return Ok(None);
    };
    let contents = contents.trim();
    if valid_build_id(contents) {
        return Ok(Some(ReloadPendingMarker {
            runtime_build: contents.to_string(),
            secret_fingerprint: None,
        }));
    }

    let mut lines = contents.lines();
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
        Some(ReloadPendingMarker {
            runtime_build: runtime_build.to_string(),
            secret_fingerprint: Some(secret_fingerprint.to_string()),
        })
    })();
    let Some(marker) = parsed else {
        return Err(CliError::Config(format!(
            "Browser Bridge reload-pending marker at {} is corrupt",
            path.display()
        )));
    };
    Ok(Some(marker))
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

fn mark_reload_pending_locked(
    config_dir: &Path,
    build_id: &str,
    secret: &str,
) -> Result<(), CliError> {
    let path = config_dir.join(RELOAD_PENDING_FILE);
    let marker = format!(
        "schema=1\nruntime_build={build_id}\nsecret_fingerprint={}\n",
        secret_fingerprint(secret)
    );
    atomic_write_private_file(
        &path,
        marker.as_bytes(),
        "Browser Bridge reload-pending marker",
    )
}

pub(crate) fn reload_pending() -> Result<bool, CliError> {
    let Some(config_dir) = crate::core::project_config_dir() else {
        return Ok(false);
    };
    Ok(reload_pending_at(&config_dir)?.is_some())
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
    if !config_dir.exists() {
        return Ok(false);
    }
    let resolved_config_dir = resolve_path_with_missing_tail(config_dir)?;
    let _lock = BrowserExtensionInstallLock::acquire(&resolved_config_dir)?;
    #[cfg(windows)]
    _lock.verify_config_directory(&resolved_config_dir)?;
    let Some(expected) = reload_pending_at(&resolved_config_dir)? else {
        return Ok(false);
    };
    if expected.runtime_build != build_id
        || expected
            .secret_fingerprint
            .as_deref()
            .is_some_and(|fingerprint| fingerprint != secret_fingerprint(authenticated_secret))
    {
        return Ok(false);
    }

    let marker = resolved_config_dir.join(RELOAD_PENDING_FILE);
    reject_symlink(&marker, "Browser Bridge reload-pending marker")?;
    std::fs::remove_file(&marker)?;
    #[cfg(unix)]
    File::open(&resolved_config_dir)?.sync_all()?;
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
) -> Result<(), CliError> {
    // Persist the expectation before touching the live directory. If swapping
    // the bundle fails, keeping a conservative pending marker is safer than a
    // false negative after a successful swap whose marker could not be written.
    mark_reload_pending_locked(config_dir, BROWSER_BRIDGE_RUNTIME_BUILD, install_secret)?;
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
            "installed Browser Bridge files but could not commit their paired secret: {commit_error}; quarantine_error={}; rollback_error={}; cleanup_error={}; previous bundle path={}; new bundle quarantine={}",
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
                "Browser Bridge was installed at {}, but the previous directory changed before cleanup; it was preserved at {} and reload remains pending",
                destination.display(),
                backup.display()
            )));
        }
        if let Err(error) = remove_snapshot(backup, previous_snapshot) {
            return Err(CliError::Config(format!(
                "Browser Bridge was installed at {}, but its validated previous bundle could not be removed safely from {}: {error}; reload remains pending",
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
        BRIDGE, BrowserExtensionInstallLock, CONFIG_TEMPLATE, INSTALL_BEHAVIOR_GUIDANCE,
        InstallOutcome, LOOPBACK_TRANSPORT, MANAGED_SENTINEL, MANIFEST, OFFSCREEN, OFFSCREEN_HTML,
        PAGE, POLL_WORKER, SERVICE_WORKER, SHARED, acknowledge_runtime_build_at,
        capture_stable_directory_snapshot, current_bundle_file_set, install_bundle,
        install_next_steps, installation_evidence_at, mark_reload_pending_locked,
        read_bridge_secret, reload_pending_at, remove_snapshot_tree, remove_snapshot_tree_paths,
        render_config, render_manifest, replace_directory_paths, secret_fingerprint,
    };

    #[test]
    fn extension_assets_share_the_bridge_contract() {
        assert!(MANIFEST.contains("https://suno.com/*"));
        assert!(MANIFEST.contains("http://127.0.0.1/*"));
        assert!(MANIFEST.contains("\"version\": \"__SUNOX_BRIDGE_RUNTIME_BUILD__\""));
        assert!(MANIFEST.contains("\"version_name\": \"__SUNOX_VERSION__\""));
        assert!(MANIFEST.contains("\"alarms\""));
        assert!(!MANIFEST.contains("\"declarativeNetRequestFeedback\""));
        assert!(MANIFEST.contains("\"declarativeNetRequestWithHostAccess\""));
        assert!(MANIFEST.contains("\"offscreen\""));
        assert!(!MANIFEST.contains("\"activeTab\""));
        assert!(!MANIFEST.contains("\"storage\""));
        assert!(!MANIFEST.contains("\"system.display\""));
        assert!(!MANIFEST.contains("\"tabs\""));
        assert!(MANIFEST.contains("\"all_frames\": true"));
        assert!(MANIFEST.contains("icons/icon-16.png"));
        assert!(MANIFEST.contains("icons/icon-128.png"));
        assert!(SERVICE_WORKER.contains("chrome.offscreen.createDocument"));
        assert!(SERVICE_WORKER.contains("chrome.declarativeNetRequest.updateDynamicRules"));
        assert!(SERVICE_WORKER.contains("chrome.declarativeNetRequest.updateSessionRules"));
        assert!(SERVICE_WORKER.contains("addRules"));
        assert!(SERVICE_WORKER.contains("content-security-policy"));
        assert!(SERVICE_WORKER.contains("x-frame-options"));
        assert!(SERVICE_WORKER.contains("cspWithoutFrameAncestors"));
        assert!(SERVICE_WORKER.contains("reasons: [\"IFRAME_SCRIPTING\", \"WORKERS\"]"));
        assert!(SERVICE_WORKER.contains("chrome.alarms"));
        assert!(SERVICE_WORKER.contains("chrome.tabs.TAB_ID_NONE"));
        assert!(!SERVICE_WORKER.contains("chrome.windows"));
        assert!(!SERVICE_WORKER.contains("chrome.system.display"));
        assert!(!SERVICE_WORKER.contains("chrome.tabs.create"));
        assert!(OFFSCREEN_HTML.contains("transport-loopback.js"));
        assert!(OFFSCREEN_HTML.contains("offscreen.js"));
        assert!(OFFSCREEN_HTML.contains("shared.js"));
        assert!(OFFSCREEN.contains("transport.claimChallenge"));
        assert!(OFFSCREEN.contains("transport.submitResult"));
        assert!(OFFSCREEN.contains("document.createElement(\"iframe\")"));
        assert!(OFFSCREEN.contains("sunox-managed-frame-execute-v2"));
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
    fn rendered_manifest_displays_the_cli_version() {
        let manifest: serde_json::Value =
            serde_json::from_str(&render_manifest()).expect("rendered extension manifest");

        assert_eq!(manifest["version"], super::BROWSER_BRIDGE_RUNTIME_BUILD);
        assert_eq!(manifest["version_name"], env!("CARGO_PKG_VERSION"));
        assert!(!render_manifest().contains("__SUNOX_VERSION__"));
        assert!(!render_manifest().contains("__SUNOX_BRIDGE_RUNTIME_BUILD__"));
    }

    #[test]
    fn installed_current_bundle_entries_come_from_the_asset_registry() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");

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

        assert_eq!(outcome.status(false), "updated");
        assert!(!outcome.reload_required(false));
        let next_steps = install_next_steps(outcome, false);
        assert!(next_steps.contains(&"No Chrome reload is required"));
    }

    #[test]
    fn supported_legacy_lineage_digest_fixtures_are_exact_and_not_interchangeable() {
        for (extension_version, cli_version) in [
            ("0.1.3", None),
            ("0.3.4", Some("0.1.0")),
            ("0.3.5", Some("0.1.1")),
            ("0.3.6", Some("0.1.1")),
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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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
    }

    #[test]
    fn force_install_into_an_existing_empty_directory_is_still_installed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
        fs::create_dir(&destination).expect("empty destination");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("install into empty directory");

        assert_eq!(outcome, InstallOutcome::Installed);
    }

    #[cfg(unix)]
    #[test]
    fn installed_bundle_and_pairing_material_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");

        install_bundle(&destination, &config_dir, false).expect("initial install");
        let secret =
            fs::read_to_string(config_dir.join(super::BRIDGE_SECRET_FILE)).expect("pairing secret");
        mark_reload_pending_locked(
            &config_dir,
            super::BROWSER_BRIDGE_RUNTIME_BUILD,
            secret.trim(),
        )
        .expect("reload-pending marker");

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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");

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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");

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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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

    #[test]
    fn force_refuses_a_byte_modified_pre_sentinel_bundle() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
        assert_eq!(
            install_bundle(&destination, &config_dir, false).expect("initial install"),
            InstallOutcome::Installed
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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
        assert_eq!(
            install_bundle(&destination, &config_dir, false).expect("initial install"),
            InstallOutcome::Installed
        );

        let outcome = install_bundle(&destination, &config_dir, true).expect("idempotent update");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert!(
            !outcome.reload_required(
                reload_pending_at(&config_dir)
                    .expect("reload state")
                    .is_some()
            )
        );
    }

    #[test]
    fn a_current_legacy_bundle_only_gains_the_sentinel_and_needs_no_reload() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
        install_bundle(&destination, &config_dir, false).expect("initial install");
        fs::remove_file(destination.join(MANAGED_SENTINEL)).expect("remove modern sentinel");

        let outcome =
            install_bundle(&destination, &config_dir, true).expect("adopt current legacy bundle");

        assert_eq!(outcome, InstallOutcome::AlreadyCurrent);
        assert!(
            !outcome.reload_required(
                reload_pending_at(&config_dir)
                    .expect("reload state")
                    .is_some()
            )
        );
        assert!(destination.join(MANAGED_SENTINEL).is_file());
    }

    #[test]
    fn update_rotates_secret_and_only_the_matching_runtime_pairing_clears_reload_pending() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
        install_bundle(&destination, &config_dir, false).expect("initial install");
        let original_secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("original secret");
        fs::write(
            destination.join("service-worker.js"),
            format!("{SERVICE_WORKER}\n// trigger update"),
        )
        .expect("stale bundle");
        install_bundle(&destination, &config_dir, true).expect("update bundle");
        let rotated_secret = fs::read_to_string(config_dir.join("browser-extension-secret"))
            .expect("rotated secret");
        let generated_config =
            fs::read_to_string(destination.join("config.js")).expect("updated generated config");

        assert_ne!(rotated_secret, original_secret);
        assert!(generated_config.contains(rotated_secret.trim()));
        assert!(!generated_config.contains(original_secret.trim()));
        assert_eq!(
            reload_pending_at(&config_dir).expect("pending marker"),
            Some(super::ReloadPendingMarker {
                runtime_build: super::BROWSER_BRIDGE_RUNTIME_BUILD.to_string(),
                secret_fingerprint: Some(secret_fingerprint(rotated_secret.trim())),
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
            !acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                rotated_secret.trim()
            )
            .expect("idempotent acknowledgement")
        );
    }

    #[test]
    fn already_current_install_advances_a_stale_pending_build_identity() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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
            })
        );
    }

    #[test]
    fn legacy_one_line_reload_marker_remains_acknowledgeable() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(
            config_dir.join(super::RELOAD_PENDING_FILE),
            format!("{}\n", super::BROWSER_BRIDGE_RUNTIME_BUILD),
        )
        .expect("legacy marker");

        assert!(
            acknowledge_runtime_build_at(
                &config_dir,
                super::BROWSER_BRIDGE_RUNTIME_BUILD,
                &"a".repeat(64)
            )
            .expect("legacy acknowledgement")
        );
        assert_eq!(
            reload_pending_at(&config_dir).expect("legacy marker cleared"),
            None
        );
    }

    #[test]
    fn failed_bundle_swap_conservatively_keeps_reload_pending() {
        let temp = tempfile::tempdir().expect("temp dir");
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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
    fn corrupt_installation_evidence_fails_closed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let config_dir = temp.path().join("config");
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(config_dir.join(super::INSTALLATION_MARKER_FILE), "corrupt")
            .expect("corrupt marker");

        let error =
            installation_evidence_at(&config_dir).expect_err("corrupt marker must fail closed");

        assert!(error.to_string().contains("installation marker"));
        assert!(error.to_string().contains("corrupt"));
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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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
        let target = temp.path().join("sensitive");
        fs::create_dir(&config_dir).expect("config dir");
        fs::write(&target, super::BROWSER_BRIDGE_RUNTIME_BUILD).expect("symlink target");
        symlink(&target, config_dir.join(super::RELOAD_PENDING_FILE)).expect("pending symlink");

        let error = reload_pending_at(&config_dir).expect_err("symlink must not be read");
        assert!(error.to_string().contains("symbolic link"));
        let error = acknowledge_runtime_build_at(
            &config_dir,
            super::BROWSER_BRIDGE_RUNTIME_BUILD,
            &"a".repeat(64),
        )
        .expect_err("symlink must not be acknowledged");
        assert!(error.to_string().contains("symbolic link"));
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
        let destination = temp.path().join("browser-extension");
        let config_dir = temp.path().join("config");
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

# Changelog

All notable changes to Sunox are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.17] - 2026-07-17

### Added

- Added `subtle`, `normal`, and `high` variation controls for remaster operations while preserving
  `normal` as the default.
- Documented recently released non-Studio Suno capabilities and the evidence required before
  implementing private Web API workflows.

### Fixed

- Preserved complete `metadata`, `relationship`, and `stats` objects from deferred playlist detail
  responses while retaining the existing normalized JSON fields for compatibility.
- Updated playlist parsing for the current deferred detail schema without dropping unknown response
  fields, and added contract coverage for the live snake_case list schema.
- Enforced remaster variation values at the API boundary instead of accepting arbitrary strings.

## [0.0.16] - 2026-07-13

### Added

- Added safe stdin authentication inputs, paginated/all library search, strict network diagnostics,
  account-aware remaster discovery, bounded downloads, structured supplemental download warnings,
  and graceful Ctrl-C cleanup.
- Added protected release approvals, release-tag rules, glibc 2.28 builds, unsigned platform
  archives, SBOM generation, artifact attestations, and scheduled security audits.

### Fixed

- Made persona creation private by default and validated vocal ranges before mutation.
- Prevented local CDP hijacking and captcha profile leakage by using an invocation-owned random
  loopback endpoint, clearing injected cookies, and deleting the temporary browser profile.
- Prevented verified auth writes from undoing concurrent logout/account switches, bounded Clerk
  requests, and removed Clerk session identifiers from transport errors.
- Preserved description-mode exclusions, rejected empty successful generation responses, aligned
  404 exit semantics, fixed redirected LRC output, and made diagnostic results independent of
  output format.
- Resolved account defaults for cover/remaster operations, surfaced remaster availability without
  treating missing `can_use` as false, and removed stale agent command metadata.
- Published unsigned platform archives so releases do not require paid Apple or Windows signing
  certificates; checksums, SBOM, and provenance attestations remain enabled.

## [0.0.15] - 2026-07-11

### Added

- Added `sunox doctor --network` for structured DNS, direct TCP, and HTTPS reachability diagnostics, with overall health based on the actual HTTPS path so proxy-only networks are represented correctly.

### Fixed

- Preserve transport, server, and transient failures during Clerk refresh; report Clerk rate limits separately instead of treating every failure as expired authentication.

## [0.0.14] - 2026-07-11

### Added

- Added `v4.5-all` / `chirp-auk-turbo` as a selectable generation model.
- Added live-captured single-source `sunox clip inspire` generation.
- Added Rust 1.88 as the declared minimum supported Rust version.
- Added release verification, dependency auditing, cross-platform tests, and self-update checksum verification.

### Fixed

- Acquire the account mutation lock before obtaining a generation challenge token so queued
  processes do not submit stale tokens.
- Serialize writes and deletes of the shared auth state globally, and prevent a stale refresh from
  overwriting a newly active account or recreating credentials after logout.
- Serialize concurrent config updates and replace `config.toml` atomically so one agent cannot
  silently discard another agent's setting or leave a partially written file.
- Keep CLI smoke tests isolated and platform-neutral on Windows as well as Unix.
- Honor explicit `XDG_CONFIG_HOME` and `HOME` paths before platform directory discovery so
  containers and Windows automation can isolate Sunox state consistently.
- Bind each mutation lock, API client, and browser challenge solve to one authentication snapshot.
- Reject non-finite or out-of-range generation controls and invalid extend timestamps locally.
- Keep generic `invalid token` responses on the JWT refresh path for ordinary API requests while
  preserving them as structured challenge errors when a generation request carries a solved token.
- Compare self-update versions semantically and avoid reporting an older release as an update.
- Resolve the default generation model and field limits from the current account, with an explicit fallback only when billing information is unavailable.
- Preserve Suno HTTP status, retryability, and structured error details instead of treating API failures as network errors.
- Use the project-specific `SUNOX_*` environment prefix and reject unresolved auth storage paths.
- Removed known vulnerable transitive dependency versions from the release build.

## [0.0.13] - 2026-07-09

### Added

- Added live-validated clip editing, playlist management, upload, download, browser login, and
  agent-oriented JSON workflows.
- Added account-scoped serial mutation control with an explicit `--parallel` override.

### Fixed

- Hardened authentication refresh and cross-process state writes.
- Fixed Windows self-update support for release zip archives.

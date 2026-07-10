# Changelog

All notable changes to Sunox are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

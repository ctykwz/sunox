# Changelog

All notable changes to Sunox are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.22] - 2026-07-20

### Changed

- Reject `--instrumental` together with `--lyrics` or `--lyrics-file` instead of silently
  discarding the lyrics input, and document bracket-only `[Instrumental]` structure as the mode for
  controlling instrumental sections, rhythm, edit points, or arrangement.
- Clarify unattended challenge handling: when Browser Bridge installation is confirmed, omit
  `--no-captcha` and use fail-closed `challenge_browser=existing`; when it is not installed or its
  installation is unknown, keep `--no-captcha`.

## [0.0.21] - 2026-07-19

### Added

- Added an optional paired Chrome Browser Bridge that executes required invisible hCaptcha or
  Turnstile checks inside an existing `suno.com` tab without opening another browser window.
- Added `sunox install-browser-extension` and the `challenge_browser=auto|existing|isolated`
  setting on macOS and Windows. The default `auto` path preserves reliability by falling back to
  the isolated browser.

## [0.0.20] - 2026-07-19

### Fixed

- Switched single-clip reads from the legacy feed IDs route to the current `GET /api/clip/{id}`
  contract across status polling, edits, cover, extend, stems, downloads, and song-page reads.
- Made the current aligned-lyrics v3 start/poll workflow primary, with v2 retained only as an
  explicit compatibility fallback when source lyrics are unavailable or v3 cannot serve the clip.
- Made `PATCH /api/playlist/v2/{id}` the primary playlist name/description mutation; the legacy
  metadata route remains only for arbitrary external cover URLs that v2 cannot represent.
- Resynchronized create requests with the current Suno Web contract: custom lyrics use `prompt`,
  while simple descriptions use `gpt_description_prompt`, `create_mode=simple`, and the current
  tag/persona override metadata; custom vocal gender uses the native `metadata.vocal_gender`
  field, and omitted custom title/tags use the Web contract's empty strings.
- Matched the flat audio-upload initialization body and current per-persona trash, restore, and
  purge route.
- Matched current cover, concat, extend, tag-upsample, visibility, upload-finish, and uploaded clip
  cover fields, including exact-ID feed v3 polling for multi-clip reads.
- Recovered a missing same-account browser device ID when possible and omitted the header when it
  is unavailable instead of fabricating an all-zero identity.
- Reported partial progress for current per-ID persona trash, restore, and purge mutations instead
  of hiding earlier successful writes when a later request fails.
- Preserved newly added billing, model, clip, clip-metadata, feed, playlist, persona, and upload
  status response fields at their original JSON level instead of silently dropping them.
- Matched Suno Web's generation challenge lifecycle by automatically solving required invisible
  hCaptcha or Cloudflare Turnstile checks and preserving the detected `token_provider`.
- Prioritized the verified stored account cookies and matching recorded browser source during
  silent challenge verification, while retaining `--captcha`, `--no-captcha`, and external-token
  overrides.

## [0.0.19] - 2026-07-19

### Fixed

- Recovered missing browser source, user-agent, accepted-language, and client-hint metadata from
  the matching local browser/profile before authenticated requests, including legacy auth files;
  fresh fields override stored ones, stored values survive failed probes, and hardcoded values are
  only the final per-field fallback for Clerk and Suno API requests.
- Prevented concurrent JWT refreshes from overwriting newly recovered browser request metadata.
- Preserved Stable/Beta/Dev/Canary/Developer/Nightly channel identity when pairing profile settings
  with runtime headers, bounded metadata probes, avoided unrelated Keychain lookups, and skipped
  destructive live Chromium cookie-database reads on Windows while keeping Firefox read-only.
- Prevented concurrent JWT refreshes from rolling back newer device or browser metadata and avoided
  repeating the same browser-environment recovery twice in one auth command.

## [0.0.18] - 2026-07-18

### Added

- Added Windows Internet Settings proxy discovery for Suno API, diagnostics, and self-update
  requests while keeping loopback CDP traffic direct.
- Added discovery for common Chrome, Edge, Brave, and Chromium stable/preview installations across
  Windows, macOS, and Linux, including custom Windows browser profiles.
- Added Windows ARM64 release artifacts and CI verification for static Visual C++ runtime linkage.

### Fixed

- Made interactive login wait for a verified Clerk JWT and successful Suno API validation before
  reporting success or closing its dedicated browser session.
- Reused a verified dedicated login profile across runs, opened manual Google login without remote
  debugging, and shut down only browser processes owned by that profile.
- Preserved detailed browser-cookie probe failures instead of silently discarding profile,
  database, and decryption errors.
- Protected stored Windows authentication with DPAPI while continuing to read and migrate legacy
  plaintext auth state.
- Kept browser discovery, Windows path construction, and conditional imports portable across
  Linux, macOS, and Windows builds.
- Reduced interactive-login process polling on Windows to avoid repeated full process-table scans
  while waiting for the dedicated browser window to close.

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
- Updated `tar` to 0.4.46 to fix PAX header desynchronization when processing crafted archives.

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

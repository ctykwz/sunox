# Capability test matrix

This matrix records automated evidence, not a claim that every Suno entitlement is available to
every account. Account/model availability remains billing-readback driven and fail-closed.

| Capability area | Type/validation | HTTP contract | Workflow/readback | Public CLI | Live account boundary |
| --- | --- | --- | --- | --- | --- |
| Account, billing, capabilities, models | yes | yes | read-only projection | smoke | read-only on request |
| Clip list/search/info/status/actions | yes | yes | pagination and exact-ID checks | smoke + CLI-to-HTTP | read-only on request |
| Create, extend, cover, inspire | yes | yes | submit/poll/readback, challenge, ambiguity | smoke | mutation only with explicit scope |
| Remaster, speed, crop, replace, concat | yes | yes | submit/poll/readback and ambiguity | smoke | mutation only with explicit scope |
| Lyrics, timed lyrics, lyrics editor/projects | yes | yes | fallback/schema drift/readback | smoke | mutation only with explicit scope |
| Upload audio/image and clip metadata | yes | yes | S3 stages, auth freshness, moderation, checkpoint/readback | smoke | mutation only with explicit scope |
| Download audio/video/stems | yes | yes | authorization, billing reconciliation, atomic files | smoke | account quota write only with explicit scope |
| Clip reaction, visibility, trash/restore/purge | yes | yes | single-shot/partial mutation | smoke + CLI-to-HTTP | mutation only with explicit scope |
| Playlist lifecycle and membership | yes | yes | exact target/readback/partial mutation | smoke | mutation only with explicit scope |
| Persona and custom model lifecycle | yes | yes | ownership/visibility/readback | smoke | mutation only with explicit scope |
| Voice creation | yes | yes | two uploads, checkpoints, verification, private persona readback | smoke | mutation only with explicit scope |
| Cover-art image/video batches | yes | yes | model/cost gates, full batch identity, explicit apply | smoke | mutation only with explicit scope |
| Browser Bridge and challenge transport | yes | Rust + browser fixtures | pairing, permissions, runtime acknowledgement | smoke | local browser state only |
| Update/install/package | yes | release mock server | checksum, rollback, platform selection | smoke | release CI/package proof |

The automated suite intentionally excludes real-account mutation tests. A passing mock suite proves
our current request/response and safety contracts; it cannot prove that an undocumented Suno rollout
has reached a particular Pro account. `sunox account capabilities --json` and billing/model readback
are the authority for that account at execution time.

## Evidence index

- Exact Suno HTTP contracts: `src/api/endpoint_tests.rs`; test names state the operation and
  expected protocol property, including no-replay, ambiguity, schema drift, ownership, billing,
  model/cost gates, and readback.
- Multi-stage recovery: `src/workflow/*::tests` plus the workflow cases in
  `src/api/endpoint_tests.rs` for upload, Voice, visual, playlist, and generation flows.
- Public CLI surface and local safety validation: `tests/cli_smoke.rs`.
- Compiled CLI through dispatch to HTTP: `tests/cli_http.rs`, specifically
  `clip_list_dispatches_through_the_public_cli_to_feed_v3` and
  `clip_visibility_dispatches_auth_preflight_and_one_write_from_public_cli`.
- Browser Bridge Rust state/permission/installer coverage: `src/browser_bridge/mod.rs::tests` and
  `src/browser_bridge/permissions.rs::tests`; browser-runtime fixtures:
  `tests/browser_extension.test.mjs` in the CI `check` job.
- Release/update/package behavior: `src/commands/update.rs::tests`, `cargo package` in the release
  workflow, and platform jobs in `.github/workflows/ci.yml`.
- Suite-wide enforcement: the CI `check`, `coverage`, `platform-tests`, `security`, and
  `release-linux-smoke` jobs. The `coverage` job rejects line coverage below 72%.

# Capability test matrix

This matrix records automated evidence, not a claim that every Suno entitlement is available to
every account. Account/model availability remains billing-readback driven and fail-closed.

| Capability area | Type/validation | HTTP contract | Workflow/readback | Public CLI | Live account boundary |
| --- | --- | --- | --- | --- | --- |
| Account, billing, capabilities, models | yes | yes | read-only projection | smoke | read-only on request |
| Clip list/search/info/status/actions | yes | yes | pagination and exact-ID checks | smoke + CLI-to-HTTP | read-only on request |
| v6 create controls, reuse, extend, cover, inspire | yes | yes | submit/poll/readback, challenge, ambiguity, account lock | smoke + CLI-to-HTTP + SIGINT | mutation only with explicit scope |
| Underpaint, Overpaint, Remaster, speed, crop, replace, concat | yes | yes | eligibility, ownership, submit/poll/readback and ambiguity | smoke + CLI-to-HTTP | mutation only with explicit scope |
| Lyrics, timed lyrics, lyrics editor/projects | yes | yes | fallback/schema drift/readback | smoke | mutation only with explicit scope |
| Upload audio/image and clip metadata | yes | yes | S3 stages, auth freshness, moderation, checkpoint/readback | smoke + upload SIGINT | mutation only with explicit scope |
| Download audio/video/stems | yes | yes | authorization, billing reconciliation, validated media, atomic files | smoke + CLI-to-HTTP | account quota write only with explicit scope |
| Clip reaction, visibility, trash/restore/purge | yes | yes | single-shot/partial mutation | smoke + CLI-to-HTTP + SIGINT | mutation only with explicit scope |
| Playlist lifecycle and membership | yes | yes | exact target/readback/partial mutation | smoke + SIGINT | mutation only with explicit scope |
| Persona and custom model lifecycle | yes | yes | ownership/visibility/readback | smoke | mutation only with explicit scope |
| Voice creation | yes | yes | two uploads, checkpoints, verification, private persona readback | smoke | mutation only with explicit scope |
| Cover-art image/video batches | yes | yes | model/cost gates, full batch identity, explicit apply, read auth refresh | smoke + CLI-to-HTTP | mutation only with explicit scope |
| Browser Bridge and challenge transport | yes | Rust + browser fixtures | pairing, permissions, runtime acknowledgement | smoke | local browser state only |
| Update/install/package | yes | release mock server | checksum, rollback, platform selection | smoke | release CI/package proof |

The automated suite intentionally excludes real-account mutation tests. A passing mock suite proves
our current request/response and safety contracts; it cannot prove that an undocumented Suno rollout
has reached a particular Pro account. `sunox capabilities --json` and billing/model readback
are the authority for that account at execution time.

## September 11 v6 evidence

| Boundary | Automated contract | Current account evidence | Result |
| --- | --- | --- | --- |
| Default generation model | config and CLI tests | billing exposes usable default `chirp-hawk` | v6 Pro is the concrete install default; live availability is still required |
| v6 Custom duration | whole seconds 10..360, default 180 | short Custom request honored; description request carrying 10 seconds completed near 73/80 seconds | duration remains Custom-only; description mode fails closed |
| Variety | integer 0..4, v6 model, and `aug-creativity` session gate | gate present; integer preserved; fractional request rejected without a clip | supported on this account |
| Mumble | model feature plus `mumble-mode` session flag | model feature present, session flag absent; earlier ungated request was cleared | unavailable on this account and rejected before write |
| Max Mode | entitlement plus `max-mode` session flag plus model | all gates present; flag preserved in final clips | supported; final duration remains server-authoritative |
| v6 Remaster | typed variation/profile values and defaults | `chirp-halibut` high + clarity completed with both fields preserved | supported; variation defaults normal and profile defaults boost |
| Credits semantics | response decoding only | observed total balance 2502 -> 2500 after generation -> 2502 after later completion | operation-specific settlement is authoritative; unlimited role is not interpreted as no accounting |

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
- Interrupted writes: `tests/cli_interrupt.rs` covers pre-write cancellation, in-flight generation,
  upload identities, nested playlist creation responses, and discovered empty-trash targets.
  `src/core/operation.rs::tests` checks persistence, redaction, write prevention on checkpoint
  failure, and cleanup after success; `src/core/operation/identities.rs::tests` checks route aliases
  and resource-specific recovery identities.
- Public CLI regressions: `tests/generation_lock.rs` uses real file locks;
  `tests/cli_mutation_auth.rs` checks first-write authentication refresh, no replay on a write 401,
  and revalidation after generation preparation exceeds the production 30-second budget;
  `tests/cli_cover_art_auth.rs` covers read authentication refresh without replaying paid writes;
  `tests/browser_policy.rs` uses a fake browser executable; `tests/config_recovery.rs` covers
  config repair, precedence, and diagnostic exit codes.
- Media acceptance: `src/media/download.rs::tests` uses synthetic media fixtures under
  `tests/fixtures/download`, rejects empty/error/truncated payloads, and preserves existing files.
  `tests/cli_media_integrity.rs` checks Opus page/comment truncation and RF64 declared-length
  mismatches through the public CLI with `--force`, alongside complete-file controls.
- Browser Bridge Rust state/permission/installer coverage: `src/browser_bridge/mod.rs::tests` and
  `src/browser_bridge/permissions.rs::tests`; browser-runtime fixtures:
  `tests/browser_extension.test.mjs` in the CI `check` job.
- Release/update/package behavior: `src/commands/update.rs::tests`, `cargo package` in the release
  workflow, and platform jobs in `.github/workflows/ci.yml`.
- Suite-wide enforcement: the CI `check`, `coverage`, `platform-tests`, `security`, and
  `release-linux-smoke` jobs. The `coverage` job rejects line coverage below 72%.

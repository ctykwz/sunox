# Architecture

Sunox is a single Rust binary organized around one dependency direction:

```text
cli -> app/dispatch -> commands -> workflow -> api -> types/core/net
                              \-> output
```

`cli` owns argument shapes only. `app` builds the command context and dispatches. `commands`
owns user-facing orchestration and presentation. `workflow` owns multi-stage operations and
durable recovery context. `api` owns the observed Suno HTTP contracts. `core`, `types`, and `net`
must not depend on command handlers.

The Browser Bridge is a cross-cutting runtime boundary in `src/browser_bridge/`. It owns bundle,
pairing, permission, activation, and runtime state. `commands::browser_extension` converts CLI
arguments and renders the domain report. Captcha, doctor, and update code depend on
`browser_bridge`, never on another command handler.

## Protocol and mutation invariants

- Read requests may retry only where the route is explicitly idempotent.
- Account writes are sent once with redirects disabled. Transport loss, redirects, response-body
  loss after acceptance, and server errors are reported as ambiguous mutations, never replayed.
- Production clients preflight the active account through billing before their first write, even
  when the caller has only acquired the account lock. Workflows that spend more than 30 seconds
  preparing, uploading, or polling revalidate before a later write. Every raw
  no-redirect mutation builder must pass through `prepare_mutation_request`; higher-level writes
  use `send_mutation_once`, which applies the same preparation centrally. The local active account
  is checked before and after auth preflight and before each write; switching accounts or logging
  out cannot silently move an in-progress request to another account.
- Multi-stage writes preserve durable IDs and expose read-only inspection or recovery guidance.
- The CLI scopes each command with `core::operation` recovery state. Mutation preparation saves
  request identities before sending; successful JSON write responses add resource identities before
  the next stage. Failure and Ctrl+C preserve this journal and report possible remote effects;
  successful commands remove it only when no unresolved mutation was downgraded to a warning.
  Such warnings retain their recovery details and journal even when a local download succeeds.
  The journal stores allowlisted IDs, never request payloads or auth,
  and provides inspection rather than automatic replay. Route-specific adapters preserve known
  response wrappers and request aliases, and distinguish audio from image upload identities.
- Generation holds its account write lock before prompt enhancement and through submission.
- Background auth metadata recovery honors the application browser launch policy. Reusable login
  candidates are validated before selection; only explicit credential rejection advances candidates.
- Batch downloads stop account requests on authentication changes or rate limiting, including
  causes wrapped by an ambiguous write result, while preserving completed files and recovery evidence.
- Local image uploads are prepared before authentication and account locking: only nonempty regular
  files are read, bounded by a 64 MiB local memory safety limit rather than an assumed Suno quota.
- Downloads validate nonempty content and basic media container structure before atomic commit.
  Opus validation walks every page and packet boundary, including continued comments; WAV
  validation honors finite RIFF/RF64 sizes, extended chunk tables, and PCM frame alignment.
  Neither path performs a full codec decode.
- Unknown model, plan, entitlement, response, or protocol shapes fail closed.
- Tests never use a real Suno account. The debug-only API override accepts only loopback HTTP
  origins and is absent from release behavior.

## Test layers

1. Type and validation tests protect serialization, parsing, limits, and fail-closed behavior.
2. API contract tests assert method, path, query, request body, response shape, redirect behavior,
   and single-shot mutation semantics against a loopback server.
3. Workflow tests cover polling, partial completion, ambiguity, checkpoints, and readback.
4. CLI smoke tests cover the public command surface and local validation.
5. CLI-to-HTTP tests execute the compiled binary with isolated fake auth and a loopback Suno
   server, proving dispatch, auth preflight, exact HTTP contracts, and structured output together.

Live account checks are a separate manual release activity: read-only capability/account
readback is allowed when explicitly requested; mutations require explicit scoped authorization.

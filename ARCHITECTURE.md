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
- Commands preflight the active account through billing before their first write. Workflows that
  can spend more than 30 seconds uploading or polling revalidate before a later write. Every raw
  no-redirect mutation builder must pass through `prepare_mutation_request`; higher-level writes
  use `send_mutation_once`, which applies the same preparation centrally.
- Multi-stage writes preserve durable IDs and expose read-only inspection or recovery guidance.
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

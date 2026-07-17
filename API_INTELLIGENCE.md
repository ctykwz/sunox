# Suno API Intelligence — Reverse-Engineered April 6, 2026

Implementation notes in this file were refreshed for the Rust CLI structure on
June 30, 2026. Non-Studio page-load traffic was recaptured from the user's
logged-in local Chrome with NetLog on June 30, 2026, and Suno frontend chunks
loaded by that browser session were scanned for endpoint schemas. The current
Suno create bundle was scanned again on July 10, 2026 for non-generation edit
and download contracts. Live endpoint behavior can drift; recapture requests
before changing schemas.

## Capture Scope (June 30, 2026)

Captured Chrome NetLog URL/method evidence from:
- `/create`
- `/discover`
- `/explore`
- `/me`
- `/notifications`
- `/labs`
- `/account`

Studio was intentionally excluded from the NetLog pass. The initial NetLog
capture did not click generation submit, cover/remaster, or stems actions
because they can mutate account state or spend credits. NetLog does not include
JSON POST bodies, so body schemas in this document come from either local HARs,
current Rust endpoint tests, or Suno frontend bundle code. Audio upload was
later live-verified through the CLI for the generic `file_upload` flow. Local
HARs outside this repository were re-audited for `studio-api-prod.suno.com` API
traffic; `13suno-labs-nostudio-20260630.har` contains live generation submit,
challenge-check, tag upsample, stem-task, clip-reaction, fade, speed-adjust,
and upsample request bodies. `14suno-labs-nostudio-20260630.har` adds a live
playlist-conditioned generation request and another speed-adjust request.

Chrome DevTools Protocol was not available in this run even when Chrome was
launched with `--remote-debugging-port=9222`; NetLog and bundle analysis were
used instead.

## Local HAR Evidence Audit

Audited local HAR evidence, not committed to this repository:
- `suno-create-20260630.har`
- `suno-create-all-20260630.har`
- `suno-discover-nostudio-20260630.har`
- `suno-explore-nostudio-20260630.har`
- `suno-me-nostudio-20260630.har`
- `suno-account-nostudio-20260630.har`
- `suno-notifications-nostudio-20260630.har`
- `suno-labs-nostudio-20260630.har`
- `1suno-labs-nostudio-20260630.har`
- `12suno-labs-nostudio-20260630.har`
- `13suno-labs-nostudio-20260630.har`
- `14suno-labs-nostudio-20260630.har`

Live request-body evidence found:
- `POST /api/c/check`
- `POST /api/generate/v2-web/` for custom lyrics, instrumental custom,
  `gen_stem`, and playlist-conditioned generation variants
- `POST /api/prompts/upsample`
- `POST /api/generate/upsample`
- `POST /api/feed/v3`
- `POST /api/unified/homepage`
- `POST /api/unified/homepage/explore`
- `POST /api/clips/adjust-speed/`
- `POST /api/edit/fade/{clip_id}/`
- `POST /api/gen/{clip_id}/update_reaction_type/`
- `POST /api/mango/rights`
- `POST /api/studio/render-state-multitrack`
- `PUT /api/persona/edit-persona/{persona_id}/`
- `POST /api/persona/{persona_id}/toggle_love/` with an empty body
- `PUT /api/persona/set_visibility/{persona_id}/?is_public=true|false` with an empty body
- `PUT /api/persona/bulk-trash-personas/`

No live request-body evidence found in those HARs:
- `POST /api/generate/v2-web/` cover request variant
- `POST /api/generate/concat/v2/`
- `POST /api/edit/stems/{clip_id}`; current web stem extraction was observed
  as a `POST /api/generate/v2-web/` `task: "gen_stem"` request instead
- `POST /api/gen/trash`
- `POST /api/gen/{clip_id}/set_metadata/`
- `POST /api/gen/{clip_id}/set_visibility/`
- playlist create/set/add/remove/visibility/reorder/save/reaction/trash routes

## Auth
- **Base URL**: `https://studio-api-prod.suno.com`
- **Auth**: Clerk-based. The browser uses Clerk session cookies and calls `auth.suno.com`; this CLI extracts the Clerk cookie, exchanges it for a JWT, then uses `Authorization: Bearer <jwt>` for direct API calls.
- **Current web headers observed on non-Studio page-load API calls**:
  - `device-id: <uuid>` (from browser, persisted)
  - `browser-token: {"token":"<base64({"timestamp":<ms>})>"}` (dynamic, generated per-request)
  - `user-agent: <browser runtime user agent>` when captured, otherwise a
    Chromium fallback
  - `accept-language: <browser languages>` when captured, otherwise `en`
  - `sec-ch-ua`, `sec-ch-ua-mobile`, and `sec-ch-ua-platform` on Chromium-like
    user agents
  - `accept: */*`
  - `sec-fetch-site: same-site`
  - `sec-fetch-mode: cors`
  - `sec-fetch-dest: empty`
  - `priority: u=1, i`
  - `origin: https://suno.com`
  - `referer: https://suno.com/`
- **CLI-only direct-call header**:
  - `authorization: Bearer <jwt>`
- **JWT lifetime**: ~1 hour. Auto-refreshed by Clerk SDK in browser.
- **Clerk session ID**: Found in JWT `sid` claim.
- **Clerk versions observed**: `__clerk_api_version=2025-11-10`, `_clerk_js_version=5.117.0`.
- **Captcha/challenge observed**: Clerk heartbeat posts `captcha_widget_type=invisible`, `captcha_action=heartbeat`; page load uses Cloudflare Turnstile assets from `challenges.cloudflare.com`. `13suno-labs-nostudio-20260630.har` captured `POST /api/c/check` returning both `required: false` and `required: true`; when a generation token was present, the submit body used `token_provider: 1`.
- **Dynamic request fields observed across the captured Suno API calls**:
  `browser-token` contains only a base64 JSON timestamp, generation bodies use
  `transaction_uuid` and `metadata.create_session_token`, `metadata.user_tier`
  matches `/api/billing/info/` `plan.id`, and tag upsample flows can carry the
  upstream `metadata.last_tags_generation.request_id`. No separate body-level
  fingerprint, timezone, locale, or browser runtime blob was found in the
  captured generation submit path.
- **Response-to-request carry-over scan**: all captured Suno API responses were
  scanned for scalar values that appeared in later request headers, query
  strings, or JSON bodies. The relevant non-resource carry-over is
  `/api/billing/info/` `plan.id` into generation `metadata.user_tier`, which the
  CLI now fills when available. The optional tag-enhancement flow carries
  `/api/prompts/upsample` `request_id` and `upsampled` tags into
  `metadata.last_tags_generation`, while the observed `personalization_enabled`
  flag is part of the captured submit shape rather than the upsample response.
  This metadata is sent only when the upsample request actually ran;
  `--enhance-tags` runs this flow explicitly and the CLI must not fabricate it
  otherwise. Other matches were normal resource
  chaining such as clip IDs, feed cursors, user IDs, remaster model keys, and an
  echoed/persisted `device-id`, not hidden validation nonces.

## Page-Load Endpoint Map (Non-Studio)

All pages below also send shared bootstrap requests such as:
- `GET /api/session/`
- `GET /api/billing/info/`
- `GET /api/billing/usage-plan-descriptions/`
- `GET /api/billing/usage-plan-web-table-comparison/`
- `GET /api/billing/usage-plan-faq/`
- `GET /api/user/tos_acceptance`
- `GET /api/user/get_user_session_id/`
- `POST /api/user/user_config/` with `{}`
- `POST /api/statsig/experiment/`
- `POST /api/video_gen/pending_batches` with `{}`
- `GET /api/notification/v2`
- `GET /api/notification/v2/badge-count`
- `GET /api/realtime/discover`
- `GET /api/profiles/pinned-clips`
- `GET /api/prompts/v2`
- `GET /api/lyrics-projects`
- `GET /api/custom-model/pending/`
- `GET /api/contests/`
- `GET /api/cms/nudges/share-nudge`
- `GET /api/cms/nudges/publish-nudge`
- `GET /api/share/stats?content_type=song`

Page-specific requests observed:
- `/create`
  - `GET /api/modals`
  - `GET /api/project/me?page=1&sort=max_created_at_last_updated_clip&show_trashed=false&exclude_shared=false`
  - `GET /api/project/default`
  - `GET /api/project/default/pinned-clips`
  - `GET /api/prompts/suggestions`
  - `GET /api/challenge/progress`
  - `POST /api/feed/v3` using the default workspace filter.
- `/discover`
  - `POST /api/unified/homepage` with `{"cursor": null}`.
  - Response top-level keys: `feeds`.
- `/explore`
  - `POST /api/unified/homepage/explore` with `{"cursor": null}`.
  - Response top-level keys: `feeds`, `next_cursor`.
- `/me`
  - `POST /api/feed/v3` using a `user` filter.
- `/notifications`
  - Shared notification bootstrap endpoints only in this pass:
    `GET /api/notification/v2` and `GET /api/notification/v2/badge-count`.
- `/labs`
  - `GET /api/labs/configs`.
  - Response is an array. Element keys observed:
    `lab_id`, `cover_image_url`, `description_override`, `enabled_ga`,
    `has_statsig_segment`, `name_override`, `staff_only`.
- `/account`
  - `GET /api/billing/default-currency`.
  - Shared billing endpoints listed above.

## Bundle-Discovered Surfaces (Not Verified)

The same browser session loaded Suno frontend bundle code. A string scan found
233 `/api/...` paths. The routes below were discovered from bundle strings, not
from actual clicked requests in this pass. Treat them as pointers for future
DevTools captures, not as confirmed request schemas.

Studio routes also appeared in the bundle, but they are excluded by scope.

Agent-facing capability metadata should expose known non-implemented or
unverified surfaces instead of advertising an empty gap list. As of this pass,
`sunox agent-info --json` reports video upload, `update_feedback_state`,
social/profile/project/video surfaces, stale voice-verification routes, and Studio export surfaces under
`unsupported_surfaces`. Playlist-conditioned generation is implemented as
`sunox clip inspire`. Image upload is implemented for clip and playlist cover
replacement. Fade is now exposed as `sunox clip fade`; reverse,
crop/remove-section, and official download formats were added from the July 10,
2026 current bundle scan.

Read-oriented surfaces worth capturing next:
- Search: `/api/unified/search/omnisearch`,
  `/api/unified/search/suggest`, `/api/search/`, `/api/search/users`.
- Clip detail and lyrics: `/api/clip/{clip_id}`,
  `/api/clips/get_songs_by_ids`, `/api/gen/{clip_id}/aligned_lyrics/v2`,
  `/api/gen/{clip_id}/aligned_lyrics/v3`,
  `/api/gen/{clip_id}/downbeats`,
  `/api/gen/{clip_id}/waveform-aggregates`.
- Profiles: `/api/profiles/{handle}`, `/api/profiles/{handle}/info`,
  `/api/profiles/listen-history`, `/api/profiles/mutual-followers`.
- Playlists: `/api/playlist/me`, `/api/playlist/v2/{playlist_id}`,
  `/api/living_radio/{station_id}/song-list`.
- Social feeds: `/api/social/following-feed`, `/api/unified/feed`.
- Labs and challenges: `/api/labs/configs`, `/api/challenge/progress`.

Mutation or credit-risk surfaces that need explicit confirmation before capture:
- Generation adjuncts: `/api/generate/matrix`,
  `/api/generate/get_recommend_styles`, plus the unrecaptured cover
  `POST /api/generate/v2-web/` variant.
- Clip mutation: `/api/gen/trash`, `/api/gen/{gen_id}/set_metadata/`,
  `/api/gen/{gen_id}/set_visibility/`,
  `/api/gen/{gen_id}/update_feedback_state/`,
  `/api/gen/{gen_id}/update_reaction_type/`.
- Playlists/projects: `/api/playlist/create/`,
  `/api/playlist/update_clips/`, `/api/project`,
  `/api/project/{project_id}/metadata`.
- Uploads: `/api/uploads/audio/`, `/api/uploads/audio/{upload_id}/`,
  `/api/uploads/audio/{upload_id}/upload-finish/`,
  `/api/uploads/image/`, `/api/uploads/video/`.
- Billing: `/api/billing/create-session/`, `/api/billing/change-plan/`,
  `/api/billing/cancel-sub/`, `/api/billing/pause-sub/`,
  `/api/billing/set-default-payment-method/`.
- Social/comment actions: `/api/comment/{comment_id}/reaction`,
  `/api/profiles/follow`, `/api/profiles/block`, `/api/share/event`.
- Video generation/hooks: `/api/video_gen/image/generate`,
  `/api/video_gen/text/generate`, `/api/video_gen/video/generate`,
  `/api/video/hooks/create`, `/api/video/hooks/{hook_id}/reaction`.

## Account Response
- `/api/billing/info/` returns the active plan, remaining credits, usage period, feature flags, model list, and model limits.
- Do not commit live account-specific credit balances to this file; they drift quickly and are not useful as implementation evidence.

## Models (from /api/billing/info/)

Read-only account response rechecked on 2026-07-10. Availability and defaults are account-specific;
the length limits below are the values returned by that response.

| Display Name | External Key | Default | Max Prompt | Max Tags | Max Neg Tags | Max GPT Desc |
|---|---|---|---|---|---|---|
| **v5.5** | `chirp-fenix` | **YES** | 5000 | 1000 | 1000 | 3000 |
| v5 | `chirp-crow` | No | 5000 | 1000 | 1000 | 3000 |
| v4.5+ | `chirp-bluejay` | No | 5000 | 1000 | 1000 | 3000 |
| v4.5 | `chirp-auk` | No | 5000 | 1000 | 1000 | 3000 |
| v4.5-all | `chirp-auk-turbo` | Free model | 5000 | 1000 | 1000 | 3000 |
| v4 | `chirp-v4` | No | 3000 | 200 | 1000 | 3000 |
| v3.5 | `chirp-v3-5` | No | 3000 | 200 | 1000 | 3000 |
| v3 | `chirp-v3-0` | No | 3000 | 200 | 1000 | 3000 |
| v2 | `chirp-v2-xxl-alpha` | No | 3000 | 200 | 1000 | 3000 |

### Remaster Models
| Name | Key |
|---|---|
| v5.5 (default) | `chirp-flounder` |
| v5 | `chirp-carp` |
| v4.5+ | `chirp-bass` |

## Verified Endpoints

### GET /api/billing/info/
Returns full account info, credits, plan, models, features, limits.

### POST /api/generate/lyrics/
**Request**: `{"prompt": "description of song"}`
**Response**: `{"id": "<uuid>"}` (async — poll for result)

### GET /api/generate/lyrics/{id}
**Response** (when complete):
```json
{
  "text": "[Verse 1]\n...\n[Chorus]\n...",
  "title": "Generated Title",
  "status": "complete",
  "error_message": "",
  "tags": ["style description auto-generated by Suno"]
}
```

### POST /api/generate/v2-web/
**Generate music**. Current CLI implementation posts to this route using
`src/api/types/generation.rs::GenerateRequest`.

Custom create submit payload was live-recaptured from
`13suno-labs-nostudio-20260630.har` on June 30, 2026. For custom lyrics, the
current web body keeps `prompt` empty and sends lyrics in
`gpt_description_prompt`:
```json
{
  "token": null,
  "token_provider": null,
  "generation_type": "TEXT",
  "title": "Summer Vibes",
  "tags": "pop, upbeat, synths",
  "negative_tags": "metal, heavy, dark",
  "mv": "chirp-fenix",
  "prompt": "",
  "gpt_description_prompt": "[Verse]\\n...",
  "make_instrumental": false,
  "user_uploaded_images_b64": null,
  "metadata": {
    "web_client_pathname": "/create",
    "is_max_mode": false,
    "is_mumble": false,
    "create_mode": "custom",
    "user_tier": "<account plan uuid>",
    "create_session_token": "<uuid>",
    "disable_volume_normalization": false,
    "lyrics_model": "default"
  },
  "override_fields": [],
  "cover_clip_id": null,
  "cover_start_s": null,
  "cover_end_s": null,
  "persona_id": null,
  "artist_clip_id": null,
  "artist_start_s": null,
  "artist_end_s": null,
  "continue_clip_id": null,
  "continued_aligned_prompt": null,
  "continue_at": null,
  "transaction_uuid": "<uuid>"
}
```

When custom instrumental generation is submitted, the web body omits
`gpt_description_prompt` and `metadata.lyrics_model`, even if the previous form
state contained lyrics. `15suno-labs-nostudio-20260630.har` reconfirmed that
the web instrumental toggle submits `metadata.create_mode = "custom"`,
`make_instrumental = true`, and an empty `prompt`; CLI positional prompts for
`sunox create --instrumental <prompt>` are therefore folded into `tags` instead
of being sent through inspiration mode.

When the web tag upsample flow is used first, `metadata.last_tags_generation`
is copied from `POST /api/prompts/upsample` and `override_fields` is
`["tags"]` in the July 3, 2026 `15suno-labs-nostudio-20260630.har` capture.
The CLI runs this flow only for explicit `--enhance-tags` requests and does not
fabricate this metadata because its tags and `request_id` are tied to the
upsample response. Captured submits also set
`personalization_enabled: true`; that flag was not observed in the upsample
response itself.

The same July 3 capture included `metadata.lyrics_model: "remi-v1"` because
that lyrics model was selected in the web UI. Do not treat that as a default
drift from the prior `lyrics_model: "default"` capture unless a fresh default
submit without a user-selected lyrics model confirms it.

**Challenge handling**: The web calls `POST /api/c/check` with
`{"ctype":"generation"}` before submit. Rust CLI commands that submit through
`/api/generate/v2-web/` mirror that preflight: if the response does not require
a challenge, the submit body uses `token: null` and `token_provider: null`; if
it requires a challenge and stored Clerk refresh material exists, the CLI
refreshes the JWT once and repeats the preflight before surfacing the challenge.
If the challenge is still required, the CLI stops before submit unless the user
supplied `--token` or explicitly opted into `--captcha`. When a solved token is
present, the submit body carries `token_provider: 1`. The user-facing create,
cover, extend, and stems commands expose these challenge controls.
On generation submits that carry a solved challenge token, a generic `invalid token`
response is preserved as a structured challenge/API error; ordinary API requests keep
that phrase on the JWT refresh path.

**Two modes**:
1. **Description mode** (`metadata.create_mode = "inspiration"`, `prompt` is the description) — Suno writes lyrics from description
2. **Custom mode** (`metadata.create_mode = "custom"`, `prompt` stays empty, lyrics go in `gpt_description_prompt`, `tags` + `title` + `negative_tags` set)

**Response**: `{"clips": [...], "metadata": {...}, "status": "..."}`

### POST /api/c/check
Captured from `13suno-labs-nostudio-20260630.har` before generation submit:
```json
{"ctype": "generation"}
```
Observed responses include:
```json
{"required": false, "captcha_version": 1}
```
and:
```json
{"required": true, "captcha_version": 1}
```

### POST /api/prompts/upsample
Captured before custom generation when the web enhanced empty style tags:
```json
{"original_tags": "", "is_instrumental": false}
```
Response:
```json
{
  "upsampled": "<style tags>",
  "request_id": "<uuid>"
}
```
If this response is used, generation submit sends the returned tags and embeds
`metadata.last_tags_generation` with response-derived `tags` and `request_id`,
the request's `original_tags`, and the captured submit field
`personalization_enabled: true`.

### GET /api/feed/?ids={clip_id_1},{clip_id_2}
Batch clip lookup used by `status`, `wait`, and post-submit polling. The CLI
batches IDs in pairs to avoid oversized query strings and expects the response
to be a JSON array of clip objects. Ordinary status/download lookups treat an
empty or partial response for requested IDs as `NotFound`. `clip wait` tolerates
temporarily missing IDs until its configured deadline because newly submitted
clips can become feed-visible asynchronously; missing IDs at the deadline are
then reported as `NotFound`.

### POST /api/generate/concat/v2/
Concatenate clips into a full song. `{"clip_id": "<id>"}` A July 10, 2026
live CLI submission completed from an original `metadata.type="gen"` source;
an `edit_fade` source was rejected by Suno with `Bad history.` This endpoint
therefore requires usable original generation history, not an arbitrary edited
clip.

### POST /api/generate/v2-web/ for clip extend
Continue an existing clip from a timestamp. Current CLI fetches the source clip
first with `GET /api/feed/?ids={clip_id}` because Suno rejects `title: null`
with `422 params.title should be a valid string`; the submitted title defaults
to `source.title` unless `--title` overrides it. `GET /api/feed/?ids` may omit
source style metadata that the web create page gets from `POST /api/feed/v3`;
Sunox therefore searches feed/v3 by `source.title` and merges only the exact
source clip id when tags, negative tags, or make_instrumental are missing.
`tags` defaults to `source.metadata.tags`, `negative_tags` defaults to
`source.metadata.negative_tags` when available, and `make_instrumental` defaults
to `source.metadata.make_instrumental` when available unless `--instrumental` or
`--no-instrumental` overrides it. Live CLI verification on July 2, 2026
confirmed `clip extend` succeeds with the captured web fields below.

```json
{
  "token": null,
  "task": "extend",
  "generation_type": "TEXT",
  "title": "<source title>",
  "tags": "<extension or source style tags>",
  "negative_tags": "<extension or source exclude tags>",
  "mv": "chirp-fenix",
  "prompt": "<extension lyrics or empty string>",
  "make_instrumental": true,
  "metadata": {
    "web_client_pathname": "/create",
    "is_max_mode": false,
    "is_mumble": false,
    "create_mode": "custom",
    "user_tier": "<account plan uuid or empty string>",
    "create_session_token": "<uuid>",
    "disable_volume_normalization": false,
    "lyrics_updated": true,
    "is_remix": true
  },
  "override_fields": [],
  "continue_clip_id": "<source clip id>",
  "continued_aligned_prompt": "<source continuation context or empty string>",
  "continue_at": 118,
  "transaction_uuid": "<uuid>",
  "token_provider": null
}
```

### POST /api/generate/upsample
Current web remaster route, captured from
`13suno-labs-nostudio-20260630.har`:
```json
{
  "clip_id": "<source clip id>",
  "model_name": "chirp-flounder",
  "variation_category": "normal"
}
```
The July 15, 2026 first-party web bundle still posts the selected value as
`variation_category`. Suno's current official UI exposes Subtle, Normal
(default), and High; sunox sends the corresponding lowercase values
`subtle|normal|high`. The existing HAR directly captures `normal`; the other
two values were verified from the current first-party UI plus its direct
pass-through code path, without submitting a paid remaster job.
Response shape matches generation response with two submitted remaster clips,
top-level `metadata`, `status`, `batch_size`, and `created_at`.

### POST /api/generate/v2-web/ with `task: "gen_stem"`
Captured from `13suno-labs-nostudio-20260630.har`. Current web stem extraction
uses the generation endpoint, not `/api/edit/stems/{clip_id}`:
```json
{
  "token": null,
  "token_provider": null,
  "task": "gen_stem",
  "generation_type": "TEXT",
  "title": "<source title>",
  "tags": "",
  "negative_tags": "",
  "mv": "chirp-v3-0",
  "prompt": "",
  "make_instrumental": true,
  "metadata": {
    "web_client_pathname": "/create",
    "create_mode": "custom",
    "create_session_token": "<uuid>",
    "disable_volume_normalization": false,
    "is_remix": true
  },
  "override_fields": [],
  "continue_clip_id": "<source clip id>",
  "stem_type_id": 91,
  "stem_type_group_name": "Twelve",
  "stem_task": "twelve",
  "transaction_uuid": "<uuid>"
}
```
Observed response shape matches generation response with `clips`, `status`, and
`batch_size`; one capture returned 24 submitted `chirp-stem` clips.

### POST /api/generate/v2-web/ with `task: "playlist_condition"`
Captured from `14suno-labs-nostudio-20260630.har`. This is the "Use as
Inspiration" playlist-conditioned generation variant, not concat and not cover:
```json
{
  "token": null,
  "token_provider": null,
  "task": "playlist_condition",
  "generation_type": "TEXT",
  "title": "<new title>",
  "tags": "<style tags>",
  "negative_tags": "",
  "mv": "chirp-fenix",
  "prompt": "<lyrics>",
  "make_instrumental": false,
  "metadata": {
    "web_client_pathname": "/create",
    "create_mode": "custom",
    "control_sliders": {
      "weirdness_constraint": 0.4
    },
    "last_tags_generation": {
      "tags": "<style tags>",
      "request_id": "<uuid from /api/prompts/upsample>",
      "original_tags": "",
      "personalization_enabled": true
    }
  },
  "override_fields": [],
  "playlist_id": "inspiration",
  "playlist_clip_ids": ["<source clip id>"],
  "transaction_uuid": "<uuid>"
}
```

Important difference from ordinary custom create: this variant put the lyrics
in `prompt` and did not include `gpt_description_prompt`. Do not apply the
custom-create `gpt_description_prompt` rule to `task: "playlist_condition"`.
The response uses the normal generation response shape with `clips`,
`metadata`, `status`, `batch_size`, and `created_at`.

Current CLI exposure is `sunox clip inspire <clip_id> --title <title> --tags
<tags> --lyrics-file <path>`. It deliberately accepts one source clip and vocal
lyrics only: multi-source and instrumental variants have not been live-captured.
The command runs `/api/prompts/upsample`, carries the real response into
`metadata.last_tags_generation`, and preserves the captured empty
`override_fields` array.

### POST /api/feed/v3
**Request** captured from `/create`:
```json
{
  "cursor": null,
  "limit": 20,
  "filters": {
    "disliked": "False",
    "trashed": "False",
    "fromStudioProject": { "presence": "False" },
    "stem": { "presence": "False" },
    "workspace": { "presence": "True", "workspaceId": "default" }
  }
}
```

Subsequent pages use `cursor: "<next_cursor>"`, not a numeric page index.
**Response**: `{"clips": [...], "next_cursor": "...", "has_more": true}`

`/me` uses the same endpoint and pagination shape with a user filter:
```json
{
  "cursor": null,
  "limit": 20,
  "filters": {
    "disliked": "False",
    "trashed": "False",
    "fromStudioProject": { "presence": "False" },
    "stem": { "presence": "False" },
    "user": { "presence": "True", "userId": "<user_id>" }
  }
}
```

The current web UI also uses `feed/v3` as an ID-filtered batch lookup after
generation/edit submits:
```json
{
  "filters": {
    "ids": {
      "presence": "True",
      "clipIds": ["<clip id>", "<clip id>"]
    }
  },
  "limit": 2
}
```

The July 3, 2026 `15suno-labs-nostudio-20260630.har` capture also confirmed
query-only list filters that are now exposed on `sunox clip list`:
```json
{
  "cursor": null,
  "limit": 20,
  "filters": {
    "liked": "True",
    "public": "True",
    "upload": "True",
    "trashed": "False",
    "fromStudioProject": { "presence": "False" },
    "stem": { "presence": "False" },
    "cover": { "presence": "True" },
    "extend": { "presence": "True" },
    "workspace": { "presence": "True", "workspaceId": "default" },
    "sort": { "sortBy": "upvote_count", "sortDirection": "desc" }
  }
}
```
These filters are for remote listing/search only. They are not a local library
sync or mirror contract.

Clip structure:
```
id, title, status, model_name, audio_url, video_url, image_url,
image_large_url, created_at, play_count, upvote_count, display_name,
handle, user_id, media_urls, action_config, ownership,
metadata: { tags, prompt, duration, negative_tags, model_badges,
            has_stem, is_mumble, is_remix, make_instrumental, type,
            can_remix, priority, stream, uses_latest_model, refund_credits }
```

### Song page read routes
Captured in `15suno-labs-nostudio-20260630.har` on July 3, 2026 and wired into
`sunox clip info` as non-mutating enrichment reads:

```
GET /api/clips/{clip_id}/attribution
Response: {"source_clips": [{"clip_id": "...", "title": "...", "image_url": "...", "audio_url": "...", "is_deleted": true, "relationship": "COV|EX", "user": {...}}]}

GET /api/gen/{clip_id}/comments?order=most_liked
Response: {"results": [...], "allow_comment": true, "total_count": 11}

GET /api/clips/direct_children_count?clip_id={clip_id}
Response: {"count": 3}

GET /api/clips/get_similar/?id={clip_id}
Response: {"similar_clips": [<clip-like objects>]}
```

These are page-inspection APIs. They do not create resources and should not be
treated as generation/edit flows. `sunox clip info` keeps the base clip usable
when a non-auth, non-rate-limit supplemental read fails; JSON output then
includes `supplemental_errors` entries with `field`, `code`, and `message`.
Authentication and rate-limit errors still abort so callers preserve normal
retry/auth handling.

### POST /api/unified/homepage
Discover feed. Request: `{"cursor": null}`.
Response top-level: `feeds`. Each feed item includes `feed_id`, `feed_title`,
`feed_container_type`, `items`, `logging_context`, and `presentation`.

### POST /api/unified/homepage/explore
Explore feed. Request: `{"cursor": null}`.
Response top-level: `feeds`, `next_cursor`.

### GET /api/labs/configs
Labs index config. Returns an array of lab config objects with keys such as
`lab_id`, `cover_image_url`, `description_override`, `enabled_ga`,
`has_statsig_segment`, `name_override`, and `staff_only`.

### GET /api/playlist/me?page={page}
User's playlists. Returns `{"num_total_results": N, "current_page": N, "playlists": [...]}`.

### GET /api/playlist/v2/{playlist_id}
The live July 17, 2026 detail response is deferred: its top-level keys are
`bio`, `deferred_fields`, `metadata`, `relationship`, and `stats`. Identity,
name, cover, visibility, and `song_count` are inside `metadata`; trash state is
inside `relationship`; `stats.track_count` is the count fallback. `playlist
info --json` keeps the existing normalized top-level fields and also returns
the three nested objects without rebuilding them, preserving unknown fields,
explicit nulls, and their original nesting. Other unknown top-level fields are
kept under `extra`.

### Playlist management routes
Suno Web bundle exposes these non-Studio playlist operations:

```
POST /api/playlist/create/
Body: {"name": "Untitled"}

POST /api/playlist/set_metadata
Body: {"playlist_id": "...", "name": "...", "description": "...", "image_url": "..."}

PATCH /api/playlist/v2/{playlist_id}
Body for uploaded playlist covers:
{"metadata":{"cover_url":"https://cdn2.suno.ai/image_<upload_id>.jpeg","cover_image_s3_id":"image_<upload_id>","cover_is_user_set":true}}

POST /api/playlist/v2/{playlist_id}/tracks/add
Body: {"clip_ids": ["..."]}

POST /api/playlist/v2/{playlist_id}/tracks/remove
Body: {"clip_ids": ["..."]}

Live CLI testing showed that larger batch remove requests can return a Suno
500 even when removing the same clips one by one succeeds. `sunox playlist
remove` therefore accepts multiple clip IDs but submits one remove request per
clip ID. If a later single-clip remove fails, the command stops and returns a
`partial_mutation` JSON error whose `error.details` includes
`requested_clip_ids`, `succeeded_clip_ids`, `failed`, and
`not_attempted_clip_ids`; callers should inspect those fields before retrying.

PATCH /api/playlist/v2/{playlist_id}
Body: {"metadata": {"is_public": true}}

POST /api/playlist/v2/{playlist_id}/save
Body: empty

DELETE /api/playlist/v2/{playlist_id}/save
Body: empty

POST /api/playlist/v2/{playlist_id}/tracks/reorder-by-index
Body: {"positions": [{"clip_id": "...", "index": 0}]}

POST /api/playlist/v2/{playlist_id}/trash
Body: {"undo": false}

POST /api/playlist/v2/{playlist_id}/trash
Body: {"undo": true}

POST /api/playlist_reaction/{playlist_id}/update_reaction_type/
Body: {"reaction": "LIKE"}

POST /api/playlist_reaction/{playlist_id}/update_reaction_type/
Body: {"reaction": "DISLIKE"}

POST /api/playlist_reaction/{playlist_id}/update_reaction_type/
Body: {"reaction": null}
```

Current CLI implements list/info/create/set/add/remove/publish/reorder/save/unsave/like/dislike/restore/delete.

### Clip management routes
Suno Web bundle exposes these clip mutation operations:

```
POST /api/gen/trash
Body: {"trash": true, "clip_ids": ["..."]}

POST /api/gen/trash
Body: {"trash": false, "clip_ids": ["..."]}

POST /api/clips/delete/
Body: {"ids": ["..."]}

POST /api/gen/{gen_id}/update_reaction_type/
Body: {"reaction": "LIKE", "recommendation_metadata": {}}

POST /api/gen/{gen_id}/update_reaction_type/
Body: {"reaction": "DISLIKE", "recommendation_metadata": {}}

POST /api/gen/{gen_id}/update_reaction_type/
Body: {"reaction": null, "recommendation_metadata": {}}
```

The bundle also exposes `/api/gen/{gen_id}/update_feedback_state/`, but the
feedback reason/state contract is intentionally out of scope for now. Current
CLI implements clip delete/restore/purge/empty-trash and like/dislike/clear-reaction.
`clip purge` permanently deletes specified trashed clips in serial batches of
20; `clip empty-trash`
pages through `feed/v3` with `filters.trashed = "True"` and permanently
deletes every returned clip. The trash query intentionally omits normal-library
defaults such as `disliked`, `fromStudioProject`, `stem`, and `workspace`, matching
the current web trash filter and preventing valid trashed clips from being hidden.
Both commands require explicit `-y/--yes`. Permanent delete
was confirmed from the current frontend and live-verified through the CLI on
July 10, 2026.

`clip empty-trash` enumerates the complete trash before starting permanent
deletion, then uses the same serial batch executor. For either command, if the
first batch fails, the CLI
preserves the original semantic error such as `rate_limited` or `auth_expired`.
If a later batch fails after earlier batches succeeded, it returns
`partial_mutation` with `purged_clip_ids`, a structured `failed` object, and
`not_attempted_clip_ids`.

`POST /api/gen/{clip_id}/update_reaction_type/` with
`{"reaction":"LIKE"|"DISLIKE"|null,"recommendation_metadata":{}}` was also
live-observed in `13suno-labs-nostudio-20260630.har`.

Multi-clip visibility and reaction commands submit one request per clip under
the account mutation lock. A first failure preserves its semantic error. If a
later request fails after earlier clips succeeded, the CLI returns
`partial_mutation` with `operation`, `succeeded_clip_ids`, `failed`, and
`not_attempted_clip_ids` so callers can retry only unresolved clips.

Mutation locks, API clients, and browser challenge solving are derived from one
authentication snapshot per command. The shared local `auth.json` uses a global
state lock for writes and logout, while JWT refresh remains account-scoped and
uses a compare-before-write check so a stale refresh cannot replace a newly
active account.

Playlist create/set, image upload and cover assignment, and audio upload are
multi-step workflows rather than atomic endpoints. Their workflow orchestration
records the playlist/upload/clip identity and completed steps. Once any
server-side mutation succeeds, a later failure returns `partial_mutation` with
`completed_steps` and a structured `failed` step so callers do not create
duplicate playlists or uploads. API playlist helpers perform one mutation each;
the playlist workflow owns follow-up metadata, cover, and readback steps.
Presigned audio bytes are streamed with a dedicated transfer client instead of
inheriting the 30-second API deadline. When audio upload changes clip metadata,
the workflow polls until the requested title, lyrics, and image fields are
visible. Partial workflow errors include `recovery.resumable`; safe recovery
paths provide a structured command and arguments, while mutations whose retry
idempotency is not live-verified are explicitly marked non-resumable. The
read-only `clip upload-status` command queries an existing audio upload without
replaying any mutation.

### Additional live edit bodies from `13suno-labs-nostudio-20260630.har`
and `14suno-labs-nostudio-20260630.har`

```http
POST /api/clips/adjust-speed/
Body: {"clip_id":"...","speed_multiplier":0.9439,"keep_pitch":true,"title":"... (0.94x)"}

POST /api/edit/fade/{clip_id}/
Body: {"fade_out_time":79.6,"title":"..."}

POST /api/mango/rights
Body: {"content_params":{"content_id":"...","content_type":"clip"}}
```

`POST /api/edit/fade/{clip_id}/` returns `{"action_clip_id":"..."}`; the web
polls `GET /api/edit/action/{action_clip_id}/` until `status: "complete"`, then
loads the resulting clip. A July 10, 2026 live browser submission completed
through this flow.
`POST /api/clips/adjust-speed/` returns a processing clip directly and is now
exposed as `sunox clip speed <clip_id> --multiplier <n>`.

### Additional current bundle edit and download contracts from July 10, 2026

The Reverse, Crop, and Remove Section flows completed through the CLI on July
10, 2026. Their request-body schemas below remain bundle-derived because a
fresh browser wire capture was not retained:

```http
POST /api/clips/reverse-clip/
Body: {"clip_id":"...","title":"..."}

POST /api/edit/crop/{clip_id}/
Body: {"crop_start_s":12.5,"crop_end_s":74.0,"is_crop_remove":false,"title":"...","ui_surface":"song_actions"}

POST /api/edit/crop/{clip_id}/
Body: {"crop_start_s":30.0,"crop_end_s":45.0,"is_crop_remove":true,"title":"...","ui_surface":"song_actions"}

POST /api/edit/fade/{clip_id}/
Body: {"fade_in_time":2.0,"fade_out_time":78.5,"title":"..."}
```

Crop and Fade return `{"action_clip_id":"..."}` and require polling
`GET /api/edit/action/{action_clip_id}/` until complete before loading the
resulting clip. Reverse returns a clip object directly. These are exposed as:

```bash
sunox clip reverse <clip_id>
sunox clip crop <clip_id> --start <seconds> --end <seconds>
sunox clip crop <clip_id> --start <seconds> --end <seconds> --remove-section
sunox clip fade <clip_id> --in <seconds> --out <seconds>
```

Official download routes observed in the current bundle:

```http
GET /api/download/clip/{clip_id}?format=mp3
GET /api/download/clip/{clip_id}?format=m4a

POST /api/gen/{clip_id}/convert_wav/
GET /api/gen/{clip_id}/wav_file/

GET /api/gen/{clip_id}/opus_file/
POST /api/gen/{clip_id}/convert_opus
```

MP3 and M4A return a prepared download response with `download_url` and can be
`processing`; WAV uses convert-then-poll for `wav_file_url`; OPUS reads an
existing `opus_file_url` first and starts conversion only when absent. The CLI
defaults to the existing `audio_url` MP3, while explicit
`--format mp3|m4a|wav|opus` uses these official format routes. Preparation and
edit-action polling use the configured `poll_timeout_secs` and
`poll_interval_secs`; CDN file transfer has a bounded connection timeout but
no total body deadline, while a 60-second no-progress timeout prevents a
connected but stalled response from hanging the CLI. WAV conversion is
serialized as account-scoped mutations. OPUS checks for an existing file while
holding that lock and only requests conversion when the URL is absent.

### Studio multitrack stem export
Captured from `13suno-labs-nostudio-20260630.har` and the downloaded
local artifact `测试描述模式 Stems (129BPM).zip`.

The export flow is Studio-scoped and is not the same as ordinary clip audio
download:

```http
GET /api/clip/{source_clip_id}/stems/pages
```

Observed response:
```json
{"pages": 0}
```

For each source/stem clip that participates in the render, the web calls:
```http
POST /api/mango/rights
Body: {"content_params":{"content_id":"<clip id>","content_type":"clip"}}
```

Observed response:
```json
{"key": "<base64>", "iv": "<base64>"}
```

The final render call posts a full Studio arrangement state:
```http
POST /api/studio/render-state-multitrack
```

Important body constraints observed:
- top-level `title`, `lyrics`, `tags`, `negative_tags`, `style_summary`,
  `caption`, `start_beats`, `end_beats`, `web_client_pathname`, `downbeats`,
  and `format`.
- `format` was `wav_s16`.
- `state.timing.type` was `manual`, with `bps: 2.15` for a 129 BPM export.
- `state.tracks[]` contained seven audio tracks named `Lead Vocals`,
  `Backing Vocals`, `Drums`, `Bass`, `Keyboard`, `Percussion`, and `Synth`.
- each track clip referenced an asset as `{"type":"clip","id":"<stem clip id>"}`.

Response:
```json
{"download_url": "https://suno-ai--studio-bounce-prod-web.modal.run/render_streaming/<id>"}
```

The downloaded zip contained seven stereo 48 kHz 16-bit WAV files:
`0 Lead Vocals.wav`, `1 Backing Vocals.wav`, `2 Drums.wav`, `3 Bass.wav`,
`4 Keyboard.wav`, `5 Percussion.wav`, and `6 Synth.wav`.

This should remain documented, not implemented as a normal non-Studio download
command, until the required Studio state construction and rights-key usage are
modeled explicitly.

### Persona management routes
Suno Web bundle exposes:

```
GET /api/persona/get-personas/?page=1
GET /api/persona/get-loved-personas/?page=1
GET /api/persona/get-followed-personas/?page=1
GET /api/persona/get-persona/{persona_id}/
GET /api/persona/get-persona-paginated/{persona_id}/?page=1
POST /api/persona/{persona_id}/toggle_love/
POST /api/persona/create/
PUT /api/persona/edit-persona/{persona_id}/
GET /api/processed_clip/{processed_clip_id}
PUT /api/persona/set_visibility/{persona_id}/?is_public=true
PUT /api/persona/set_visibility/{persona_id}/?is_public=false
PUT /api/persona/bulk-trash-personas/
```

Persona create request shape from current Suno Web bundle:

```
{
  "root_clip_id": "...",
  "name": "...",
  "description": "...",
  "image_s3_id": "...",
  "is_public": true,
  "is_suno_persona": true,
  "persona_type": "...",
  "vox_audio_id": "...",
  "vocal_start_s": 0,
  "vocal_end_s": 30,
  "user_input_styles": "...",
  "source": "...",
  "singer_skill_level": "...",
  "clips": [],
  "is_voice_recording": true,
  "voice_recording_id": "...",
  "verification_id": "..."
}
```

Persona delete from `Library -> Voices -> My Voices -> Move to trash` was
captured in local HAR evidence `1suno-labs-nostudio-20260630.har`:

```
PUT /api/persona/bulk-trash-personas/
Body: {"persona_ids":["..."],"undo":false,"hide":false}
Response: {"updated_persona_ids":["..."],"voice_persona_count":4,"max_voice_personas":1000}
```

The same page bundle defines the bulk modes:

```
trash:   {"undo": false, "hide": false}
restore: {"undo": true,  "hide": false}
delete:  {"undo": false, "hide": true}
```

### Persona love toggle

Captured from `12suno-labs-nostudio-20260630.har` on June 30, 2026:

```http
POST /api/persona/{persona_id}/toggle_love/
```

No JSON body is sent. The response returns the updated love state.

### Persona detail page clips

Captured from `12suno-labs-nostudio-20260630.har` on June 30, 2026:

```http
GET /api/persona/get-persona-paginated/{persona_id}/?page=1
```

Response contains `persona`, `total_results`, `current_page`, and `is_following`.
The nested `persona.persona_clips[]` entries wrap song objects as `{ "clip": ... }`.

### Persona visibility

Captured from `12suno-labs-nostudio-20260630.har` on June 30, 2026:

```http
PUT /api/persona/set_visibility/{persona_id}/?is_public=true
PUT /api/persona/set_visibility/{persona_id}/?is_public=false
```

No JSON body is sent. The response is the updated Persona object.

### Persona edit

Captured from `12suno-labs-nostudio-20260630.har` on June 30, 2026:

```http
PUT /api/persona/edit-persona/{persona_id}/
```

Observed body:

```json
{
  "persona_id": "...",
  "name": "My Voice - Apr 61",
  "description": "test",
  "is_public": false,
  "persona_type": "vox",
  "user_input_styles": "test",
  "vox_audio_id": "fd11f004-a4f9-4156-b36f-a36866bd9302",
  "vocal_start_s": 0.4359633027522936,
  "vocal_end_s": 22.56
}
```

Response is the updated Persona object.

### Processed vocal clip

Captured from `12suno-labs-nostudio-20260630.har` on June 30, 2026:

```http
GET /api/processed_clip/{processed_clip_id}
```

Observed response fields: `id`, `status`, `vocal_start_s`, `vocal_end_s`, `vocal_audio_url`.

Current CLI implements persona list/info/clips/create/set/processed-clip/publish/unpublish/love/unlove/toggle-love/delete/restore/purge.

### GET /api/trending/
Trending clips. Returns playlist-like structure.

### POST /api/edit/stems/{clip_id}
Older/bundle-discovered stem separation route. No live request body was found in
the June 30 HAR audit; current web stem extraction was observed as
`POST /api/generate/v2-web/` with `task: "gen_stem"`.

### POST /api/generate/v2-web/
Cover generation. Current CLI implementation first loads the source through
`GET /api/feed/?ids={clip_id}`, then uses the unified web generation route with
`metadata.create_mode = "cover"`, `cover_clip_id`, and the source title as a
non-null string. Live CLI submit and completion were verified on July 10, 2026;
the fresh browser mutation body remains unrecaptured.

### POST /api/generate/v2-web/
Older/bundle-discovered remaster variant. The current CLI uses the live-captured
`POST /api/generate/upsample` route instead.

## Audio Upload Flow (bundle-derived, live-verified June 30, 2026)

The current non-Studio web bundle exposes a standard presigned S3 upload flow.
The CLI live-verified the generic `file_upload` flow on June 30, 2026.

### Step 1: Initialize audio upload
```
POST /api/uploads/audio/
Body: {"spec": {"extension": "mp3", "is_stem_mix": false, "upload_type": "file_upload"}}
```

Accepted `upload_type` enum values observed from Suno validation:
`file_upload`, `studio_file_upload`, `audio_recording`, `voice_recording`,
`video_recording`, `marketplace_submission`, `stem_mix`, and
`external_daw_sample`.

Response includes an upload ID plus S3 form fields:
```json
{
  "id": "<upload_id>",
  "url": "https://...",
  "fields": {
    "key": "...",
    "policy": "...",
    "x-amz-signature": "..."
  }
}
```

### Step 2: Upload bytes to S3
The browser uploads the local file to the returned `url` using the returned
form `fields`. This request is not sent to Suno's API host.

### Step 3: Finish upload
```
POST /api/uploads/audio/{upload_id}/upload-finish/
Body: {"upload_type": "...", "upload_filename": "song.mp3"}
```

### Step 4: Poll processing status
```
GET /api/uploads/audio/{upload_id}/
```

The bundle polls roughly every 4 seconds after `upload-finish` until status is
`complete` or `error`. Completion data used by the web UI includes fields such
as `title`, `image_url`, `has_vocal`, `inferred_description`, and
`copyright_muted`.

### Step 5: Initialize uploaded clip
```
POST /api/uploads/audio/{upload_id}/initialize-clip/
Body examples:
{"downbeats": [...]}
{"user_reviewed_tags": true}
{}
```

After the clip is initialized, the web UI calls clip metadata update with
`is_audio_upload_tos_accepted: true`, `image_url`, `title`, and optional
lyrics.

Image upload was live-verified for clip and playlist cover replacement:
initialize with `POST /api/uploads/image/` and the file extension (`png`,
`jpg`, `jpeg`, or `webp`), upload bytes to the returned presigned S3 form,
finish with
`POST /api/uploads/image/{upload_id}/upload-finish/` and `{}`, and require
`moderation_status: "approved"`. Clip cover replacement uses
`POST /api/gen/{clip_id}/set_metadata/` with
`{"image_url":"https://cdn2.suno.ai/image_<upload_id>.jpeg"}`. Playlist cover
replacement uses the same CDN URL plus `cover_image_s3_id:
"image_<upload_id>"` in the playlist v2 patch above. The legacy
`POST /api/playlist/set_metadata` `image_url` path can return `Failed to upload
image` for freshly uploaded Suno images. Clip `remove_video_cover: true` was
also live-verified through `POST /api/gen/{clip_id}/set_metadata/`.
Related video upload routes also appear in the current bundle:
- `POST /api/uploads/video/`

## Voices / Persona Creation Flow (older capture, out of scope)

The older capture below showed a voice-persona flow. The current June 30, 2026
non-Studio bundle scan did not find `/api/processed_clip/voice-vox-stem` or
`/api/voice-verification/`. Treat those routes as stale or flow-specific. This
workflow is not tracked as a current CLI gap.

Full pipeline for creating a Voice persona from audio:

### Step 1: Upload initial voice sample
The S3 presigned upload happens first (not captured here), then:
```
POST /api/uploads/audio/{upload_id}/upload-finish/
```
Response: `200 OK` (empty body, content-length: 2)

### Step 2: Poll upload status
```
GET /api/uploads/audio/{upload_id}/
```
Response: JSON with processing status.

### Step 3: Extract vocal stem
```
POST /api/processed_clip/voice-vox-stem
Content-Length: ~90 bytes
```
Extracts clean vocals from uploaded audio. Body likely: `{"upload_id": "<id>"}`.
Called multiple times — once per upload (sample + verification).

### Step 4: Record & upload verification phrase
User reads: "Listening to the melody of a gentle summer breeze"
Second upload goes through the same upload-finish flow with a new upload_id.

### Step 5: Voice verification
```
POST /api/voice-verification/
Content-Length: 179 bytes
```
Verifies the voice matches. Body likely includes both upload IDs + verification text.

### Step 6: Create persona
```
POST /api/persona/create/
Content-Length: 47261 bytes (large — likely includes audio data as base64)
```
Creates the voice persona from the verified audio clips.

### Endpoints summary:
- `POST /api/uploads/audio/{id}/upload-finish/` — mark upload complete
- `GET /api/uploads/audio/{id}/` — poll upload processing
- `POST /api/processed_clip/voice-vox-stem` — extract vocals
- `POST /api/voice-verification/` — verify voice sample
- `POST /api/persona/create/` — create voice persona (47KB payload)

The generic audio upload flow above is current bundle evidence; voice-specific
processing is not.

## Key Insights for Rust CLI

1. **Captcha/challenge is conditional** — `POST /api/c/check` with `{"ctype":"generation"}` decides whether generation needs a solved token. The CLI mirrors this preflight before `/api/generate/v2-web/` submits. If the preflight reports a challenge and stored Clerk refresh material exists, the CLI refreshes the JWT once and repeats the preflight before surfacing the challenge. Captured submits use `token_provider: 1` only when a solved `token` is present; normal authenticated submits use `token: null` and `token_provider: null`.
2. **Lyrics generation is free and easy** — no captcha needed, just JWT auth
3. **JWT refresh** — need Clerk cookie exchange or session keepalive
4. **Browser-token header** — dynamically generated from current timestamp, base64-encoded
5. **Browser environment** — browser-cookie extraction records a stable browser source id (`chrome`, `arc`, `brave`, `firefox`, or `edge`) and best-effort public profile settings such as `accept-language`; it does not fabricate a `user-agent` from that label. Interactive login captures stable runtime headers such as `user-agent` and `accept-language`. API calls reuse captured fields independently, derive Chromium client hints from the selected `user-agent`, send the stable browser fetch metadata headers observed in HARs, and fall back field-by-field when unavailable.
6. **Cookie-based approach** — store Clerk session cookies, exchange for JWT via `auth.suno.com/v1/client/sessions/<session_id>/tokens`
7. **`feed/v3` is cursor-based** — the current web request uses `cursor`, `limit`, and scenario-specific filters, not numeric pages
8. **Two auth strategies**:
   a. Cookie-based: store the Clerk client cookie and auto-refresh JWTs
   b. Direct JWT: User pastes JWT, works for ~1 hour (simpler but expires)

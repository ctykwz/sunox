# Suno API Intelligence — Reverse-Engineered through August 30, 2026

Implementation notes in this file were refreshed for the Rust CLI structure on
June 30, 2026. Non-Studio page-load traffic was recaptured from the user's
logged-in local Chrome with NetLog on June 30, 2026, and Suno frontend chunks
loaded by that browser session were scanned for endpoint schemas. The current
Suno create bundle was scanned again on July 10, 2026 for non-generation edit
and download contracts, then the complete non-Studio endpoint and generation
payload surface was rescanned on July 26, 2026. Live endpoint behavior can
drift; recapture requests before changing schemas.

The `/create` bundle was checked again on August 23–24, 2026. The generation
route and response envelope remain stable, while the request builder now uses
the actual `/create` pathname, distinguishes uploaded-audio continuation as
`task: "upload_extend"`, exposes Cowrite model discovery at
`GET /api/generate/cowrite-lyrics/models/`, and supports `audio_weight` plus
account-gated `aug_creativity` in `metadata.control_sliders`. Sunox implements
the applicable `audio_weight` control and leaves the gated control unexposed.

A deeper August 23 audit also compared account model capabilities, exact
condition combinations, playlist v2 bodies, flat playlist responses, and audio
upload guards. Extend and Inspiration now resolve the configured/account model
instead of hard-coding v5.5; Cover/Extend/upload-Extend/Inspiration validate
task plus condition compatibility before submission. Uploaded playlist covers
send only `metadata.cover_image_s3_id`, flat cover fields no longer duplicate
normalized JSON keys, and audio uploads reject formats outside
`mp3,m4a,wav,flac,ogg,aac` or sizes above 524,288,000 bytes before presign.
Authenticated readback showed intermittent resets in both protocol directions:
some GETs reset under normal negotiation and succeeded over HTTP/1.1, while
later probes did the reverse. Timing was not controlled, so this establishes
recoverability by retry rather than a causal protocol preference. Explicitly
idempotent Persona, playlist, billing/model, and Cowrite-model GETs therefore
use a bounded transport fallback rather than a route-family downgrade. Mutation
transports retain normal negotiation because no account write was performed to
justify changing them.
Persona mine/loved/followed, detail, and paginated-clips responses all decoded
successfully during the audit, although later Persona commands occasionally
exhausted the bounded retries before the same route succeeded again. The retry
boundary includes the complete JSON body read, and a runtime method guard
rejects non-GET requests before any network I/O; it does not claim deterministic
upstream availability.

The logged-in `/create` dependency closure was rescanned on August 30. Current
downloads now have a clip-level gate: only `is_download_unlocked === true` skips
`POST /api/download/authorize`; a successful authorization then permits one or
more prepared `mp3|m4a|wav|mp4` requests for that source. Authorization can be
metered, is not replay-safe, and must not be retried after an ambiguous transport
result. Billing now advertises period download usage, additional remaining
downloads, and optional download-credit packs. The CLI decodes these live fields
and does not use the public Pro/Premier quota numbers as runtime constants.

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
social/profile/project/video surfaces, and Studio export surfaces under
`unsupported_surfaces`. The current Voice verification protocol is confirmed
separately in the August 24 lazy-chunk audit below. Playlist-conditioned generation is implemented as
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

### POST /api/generate/cowrite-lyrics/
Standalone whole-lyrics route captured in the July 26, 2026 Web bundle and a
live authenticated request, then re-confirmed by the current first-party
interaction chunk and one authorized minimal submission during the preceding
August 24 protocol audit. That submission predates this 0.3.0 implementation
pass; implementation and verification for this release made no account writes.
The editor calls its empty UI state
`fresh_generate`, but that value was not an API mode: the final request sent the
user's request as `instruction` with `mode: "apply_user_request"`:

```json
{
  "selected": "",
  "context_before": "",
  "context_after": "",
  "instruction": "description of song",
  "title": "",
  "style": "",
  "mode": "apply_user_request",
  "references": [],
  "num_variants": null,
  "lyricist_id": null,
  "metadata": {
    "lyrics_model": "default",
    "enable_thinking": false
  },
  "create_session_token": null,
  "lyrics_project_id": null
}
```

The synchronous response includes `edited_lyrics`, `lyrics_request_id`, `lyrics_id`, `variants`,
`artist_to_tag_mapping`, `next_prompts`, and may add further fields.

The older `POST /api/generate/lyrics/` submit route no longer appears in the current Web bundle.
`GET /api/generate/lyrics/{lyrics_id}` still exists for the separate lyrics-mashup polling flow;
Sunox standalone lyrics generation does not use either legacy transport.

The current Web bundle and live API expose
`GET /api/generate/cowrite-lyrics/models/`. Each model includes `id`,
`display_name`, `family`, and `supports_thinking`. Before using the current
submit route, Sunox resolves the requested model against that
current response, preserves the Web literal `default` selection when no model
is requested, and refuses `--thinking` for a model that does not advertise
support.

### POST /api/generate/v2-web/
**Generate music**. Current CLI implementation posts to this route using
`src/api/types/generation.rs::GenerateRequest`.

Custom create submit payload was live-recaptured from
`13suno-labs-nostudio-20260630.har` on June 30, 2026, then its payload builder
was rechecked in the July 26, 2026 Web bundle. Ordinary custom lyrics are now
sent in `prompt`; `gpt_description_prompt` is omitted unless the Web client is
using a separate lyrics-subject or hybrid-rewrite flow:
```json
{
  "generation_type": "TEXT",
  "title": "Summer Vibes",
  "tags": "pop, upbeat, synths",
  "negative_tags": "metal, heavy, dark",
  "mv": "chirp-fenix",
  "prompt": "[Verse]\\n...",
  "make_instrumental": false,
  "user_uploaded_images_b64": null,
  "metadata": {
    "web_client_pathname": "/create",
    "is_max_mode": false,
    "is_mumble": false,
    "create_mode": "custom",
    "user_tier": "<account plan uuid>",
    "create_session_token": "<uuid>",
    "disable_volume_normalization": false
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

The same July 3 capture included `metadata.lyrics_model: "remi-v1"` because a
lyrics-subject flow selected that model. The current ordinary custom-lyrics
builder does not send `metadata.lyrics_model`; simple description mode defaults
it to `"default"`.

**Challenge handling**: The web calls `POST /api/c/check` with
`{"ctype":"generation"}` before submit. Rust CLI commands that submit through
`/api/generate/v2-web/` mirror that preflight: if the response does not require
a challenge, the submit body omits `token` and `token_provider`; if
it requires a challenge and stored Clerk refresh material exists, the CLI
refreshes the JWT once and repeats the preflight. If a challenge remains, the
CLI silently runs browser verification and follows the current web mapping:
captcha version 2 uses Cloudflare Turnstile with `token_provider: 2`, while
other or missing versions use hCaptcha with `token_provider: 1`. Stored verified
account cookies and the matching recorded browser source take priority. Use
`--token` for an external token, `--captcha` to force verification, or
`--no-captcha` to surface a required challenge without running the solver. The
user-facing create, cover, inspire, extend, and stems commands expose these controls.
On generation submits that carry a solved challenge token, a generic `invalid token`
response is preserved as a structured challenge/API error; ordinary API requests keep
that phrase on the JWT refresh path.

**Two modes**:
1. **Description mode** (`metadata.create_mode = "simple"`, description in `gpt_description_prompt`, `prompt` empty, `metadata.lyrics_model = "default"`)
2. **Custom mode** (`metadata.create_mode = "custom"`, lyrics in `prompt`, ordinary requests omit `gpt_description_prompt` and `metadata.lyrics_model`, `tags` + `title` + `negative_tags` set)

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

### GET /api/clip/{clip_id} and POST /api/feed/v3
Single-clip reads use the current direct `GET /api/clip/{clip_id}` contract.
Multi-clip status, wait, and post-submit polling uses `POST /api/feed/v3` with
`filters.ids.presence = "True"` and `filters.ids.clipIds` containing the exact
requested IDs. Results are restored to input order. Ordinary status/download
lookups treat missing requested IDs as `NotFound`; `clip wait` tolerates
temporarily missing IDs until its configured deadline because newly submitted
clips can become feed-visible asynchronously.

### POST /api/generate/concat/v2/
Concatenate clips into a full song. `{"clip_id": "<id>"}` A July 10, 2026
live CLI submission completed from an original `metadata.type="gen"` source;
an `edit_fade` source was rejected by Suno with `Bad history.` This endpoint
therefore requires usable original generation history, not an arbitrary edited
clip.

### POST /api/generate/v2-web/ for clip extend
Continue an existing clip from a timestamp. Current CLI fetches the source clip
first with `GET /api/clip/{clip_id}` because Suno rejects `title: null`
with `422 params.title should be a valid string`; the submitted title defaults
to `source.title` unless `--title` overrides it. The direct response may omit
source style metadata that the Web create page gets from `POST /api/feed/v3`;
Sunox therefore enriches through feed/v3 and merges only the exact source clip
ID when tags, negative tags, or `make_instrumental` are missing.
`tags` defaults to `source.metadata.tags`, `negative_tags` defaults to
`source.metadata.negative_tags` when available, and `make_instrumental` defaults
to `source.metadata.make_instrumental` when available unless `--instrumental` or
`--no-instrumental` overrides it. Live CLI verification on July 2, 2026
confirmed `clip extend` succeeds with the captured web fields below.

```json
{
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
  "transaction_uuid": "<uuid>"
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
For `chirp-flounder` and `chirp-carp`, current Web posts the selected
`variation_category`; Suno exposes Subtle, Normal (default), and High. The
`chirp-bass` request omits that field entirely. Before submitting, current Web
also requires a complete, non-trashed, non-infill source no longer than 960
seconds whose server `action_config` exposes Remaster as visible and enabled.
Sunox mirrors these gates and rejects an explicit `--variation` for
`chirp-bass`.
Response shape matches generation response with two submitted remaster clips,
top-level `metadata`, `status`, `batch_size`, and `created_at`.

### POST /api/generate/v2-web/ with `task: "gen_stem"`
Captured from `13suno-labs-nostudio-20260630.har`. Current web stem extraction
uses the generation endpoint, not `/api/edit/stems/{clip_id}`:
```json
{
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

Like ordinary current custom create, this variant puts lyrics in `prompt` and
does not include `gpt_description_prompt`; its distinguishing fields are
`task`, `playlist_id`, `playlist_clip_ids`, and the captured tag-generation
metadata.
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

GET /api/clips/remixes/count?clip_id={clip_id}
Response: {"count": 3, "is_capped": false}

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
{"metadata":{"cover_image_s3_id":"image_<upload_id>"}}

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

The July routes below have been superseded as the primary path by the August 30
clip-unlock contract. A source is downloadable without a write only when its
decoded `is_download_unlocked` value is exactly `true`. Missing, `null`, and
`false` all require the following request before any file fetch:

```http
POST /api/download/authorize
Content-Type: application/json

{"item_id":"<clip_id>","item_type":"clip"}
```

The current Web reads `ok`, `reason`, `message`, and `credit_deducted` from the
response. `ok != true` stops the download. The POST is potentially metered and
has no confirmed idempotency key, so Sunox sends it at most once and never
blindly replays an ambiguous result. Its transport also disables automatic
redirect following so a 307/308 cannot repeat the POST and body. A read-only
invocation fails closed unless the source was already explicitly unlocked. A
returned redirect is itself an ambiguous mutation result and triggers exact
clip/billing readback before any retry decision.

Prepared routes observed in the August 30 bundle:

```http
GET /api/download/clip/{clip_id}?format=mp3
GET /api/download/clip/{clip_id}?format=m4a
GET /api/download/clip/{clip_id}?format=wav
GET /api/download/clip/{clip_id}?format=mp4
```

Older compatibility routes still present in the bundle are:

```http

POST /api/gen/{clip_id}/convert_wav/
GET /api/gen/{clip_id}/wav_file/

GET /api/gen/{clip_id}/opus_file/
POST /api/gen/{clip_id}/convert_opus
```

MP3, M4A, WAV, and video use prepared-format routes first. Legacy WAV
convert-then-poll and direct `clip.video_url` fallback are permitted only after
the same source-unlock gate; OPUS is retained solely as legacy compatibility
because the current Web download chooser no longer exposes it. The CLI keeps
`--format mp3|m4a|wav|opus`; `--no-convert` refuses a missing legacy conversion.
Global `--read-only` additionally refuses authorization for a locked or
unknown source. Preparation and
edit-action polling use the configured `poll_timeout_secs` and
`poll_interval_secs`; CDN file transfer has a bounded connection timeout but
no total body deadline, while a 60-second no-progress timeout prevents a
connected but stalled response from hanging the CLI. WAV conversion is
serialized as account-scoped mutations. OPUS checks for an existing file while
holding that lock and only requests conversion when the URL is absent.

Existing stem results share their parent song's accounting boundary. Sunox
authorizes the parent source once and reuses that unlock for all hydrated stem
clips and formats; it never authorizes individual stem IDs. Billing exposes:

```json
{
  "download_usage": {
    "current_period_downloads_limit": 20,
    "current_period_downloads_used": 0,
    "additional_download_remaining": 0
  },
  "download_credit_packs": []
}
```

Those numbers are illustrative captured fields, not runtime policy. Sunox
preserves the live values and unknown fields; it does not infer quota from the
plan name and does not implement credit-pack purchase in this migration. Raw
`credits --json` preserves unknown response fields for protocol inspection;
sanitized `capabilities` and mutation-recovery output projects only confirmed
download fields.

### Stored stem-result pages and separate Studio multitrack export
Captured from `13suno-labs-nostudio-20260630.har` and the downloaded
local artifact `测试描述模式 Stems (129BPM).zip`.

The stored result-page lookup is now also confirmed in the current non-Studio
stems modal and is not itself a Studio render:

```http
GET /api/clip/{source_clip_id}/stems/pages
```

The older observed response was:
```json
{"pages": 0}
```

The current non-Studio modal follows that page count with
`GET /api/clip/{source_clip_id}/stems?page=<zero-based page>` and reads the
response `stems` array. The CLI exposes these two reads through
`clip get-stems`; it does not start extraction. The following rights and render
steps are the separate Studio-only export flow.

For each source/stem clip that participates in the Studio render, the web calls:
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
PUT /api/persona/set_visibility/{persona_id}/?is_public=true
PUT /api/persona/set_visibility/{persona_id}/?is_public=false
PUT /api/persona/trash-persona/{persona_id}/?undo={true|false}&hide={true|false}
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

### Processed vocal clip (historical, removed from CLI)

Captured from `12suno-labs-nostudio-20260630.har` on June 30, 2026:

```http
GET /api/processed_clip/{processed_clip_id}
```

Observed response fields: `id`, `status`, `vocal_start_s`, `vocal_end_s`, `vocal_audio_url`.

The July 26, 2026 Web bundle no longer calls this status/preview route. Its only remaining
`processed_clip` read is `/api/processed_clip/{processed_clip_id}/waveform-aggregates`, which is
not behaviorally equivalent. Sunox therefore removed `persona processed-clip`; `persona info`
continues to expose current `vocal_clip_id`, `vocal_start_s`, and `vocal_end_s` fields.

Current CLI implements persona list/info/clips/create/set/publish/unpublish/love/unlove/toggle-love/delete/restore/purge.

### GET /api/trending/
Trending clips. Returns playlist-like structure.

### POST /api/edit/stems/{clip_id}
Older/bundle-discovered stem separation route. No live request body was found in
the June 30 HAR audit; current web stem extraction was observed as
`POST /api/generate/v2-web/` with `task: "gen_stem"`.

### POST /api/generate/v2-web/
Cover generation. Current CLI implementation first loads the source through
`GET /api/clip/{clip_id}`, then uses the unified web generation route with
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
Body: {"extension": "mp3", "is_stem_mix": false, "upload_type": "file_upload"}
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
Body: {"upload_type": "...", "upload_filename": "song.mp3", "agreed_to_vip_upload_terms": false}
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
replacement extracts that upload identity and sends only
`cover_image_s3_id: "image_<upload_id>"` in the playlist v2 patch above. The legacy
`POST /api/playlist/set_metadata` `image_url` path can return `Failed to upload
image` for freshly uploaded Suno images. Clip `remove_video_cover: true` was
also live-verified through `POST /api/gen/{clip_id}/set_metadata/`.
Related video upload routes also appear in the current bundle:
- `POST /api/uploads/video/`

## Voices / Persona Creation Flow (older capture, superseded)

The older content-length-only capture originally left the Voice preprocessing
bodies uncertain. The August 24 current-page lazy chunk
`155krfj46sodd.js` now confirms the exact vocal processing, phrase,
verification, polling, image-prompt, and final Persona bodies. The authoritative
current contract is recorded in the dated audit below; do not use the former
content-length guesses or the old fixed verification phrase.

## August 24, 2026: Advanced Stems, Voices, Custom Models, Lyrics 2.0, and Cover Art

This section is the implementation boundary for the four newer Pro surfaces
audited on August 24. Evidence is labeled so that a route found in a bundle is
not accidentally presented as a live mutation capture:

- **LIVE-READ**: a safe authenticated read against the user's account. No
  create, train, generate, download, archive, or delete action was submitted.
- **CURRENT-BUNDLE**: code in the first-party `/create` HTML and JavaScript
  chunks fetched on August 24 (the bundle reports deploy build `201f1db`),
  including the current-page dynamic lazy chunk `155krfj46sodd.js`.
- **OFFICIAL**: current Suno Help Center documentation.
- **OLD/STALE**: an older HAR or bundle pointer that is useful for provenance
  but is not enough to implement a current mutation.
- **INFERENCE**: a conclusion derived from the preceding evidence, not an
  observed request.

The account read succeeded earlier in this audit and identified an active Pro
plan with `get_stems`, `custom_models`, `generate_song_image`, and
`generate_song_video` enabled. Two later repetitions failed while reading
`/api/billing/info/` because the upstream connection reset. That does not
change the successful entitlement read, but it reinforces the existing bounded
retry requirement for idempotent reads. No private song/model names, balances,
auth material, or other account-specific content is recorded here.

### 1. Get Stems is still a `gen_stem` asynchronous operation

**OFFICIAL.** [Advanced Stem Separation](https://help.suno.com/en/articles/12702337)
currently distinguishes three workflows:

- Pro and Premier: **Auto Split**, up to 12 stems, 50 credits.
- Pro and Premier: **Split from Mix**, one selected target plus its complement,
  10 credits for each extraction and 20 credits for the pair.
- Premier only: **Advanced Split**, selection from nearly 100 instruments,
  with the same 10-per-extraction / 20-per-pair pricing per stem.

The same article warns that asking for an instrument that is not present may
still consume credits. [Suno's download policy](https://help.suno.com/en/articles/13614785)
says that starting September 3, 2026, a song and its stems count once per source
song for download accounting; repeated downloads and alternate formats do not
add another count, while failed or interrupted downloads do not count.

**LIVE-READ / CURRENT-BUNDLE.** The Pro account has the `get_stems` plan
feature. The menu also evaluates the per-clip `get_stems` action configuration,
ownership, `status == "complete"`, not trashed, not a Suno Short, and download
availability. The advanced-selection UI has an additional Premier/bypass gate;
therefore Pro must not be offered arbitrary Advanced Split instruments merely
because the canonical instrument catalogue exists in the bundle.

**CURRENT-BUNDLE.** There is no distinct current first-load route such as
`/api/edit/stems` for this operation. The current request builder still submits
advanced separation through:

```http
POST /api/generate/v2-web/
```

The stem-specific part of the normal generation body is:

```json
{
  "task": "gen_stem",
  "mv": "chirp-v3-0",
  "continue_clip_id": "<source clip id>",
  "stem_type_id": 91,
  "stem_type_group_name": "<optional StemGroup>",
  "stem_task": "<StemTask>",
  "stem_name": "<optional canonical instrument name>",
  "metadata": {
    "is_remix": true
  }
}
```

The normal generation metadata (`transaction_uuid`,
`create_session_token`, client surface, user tier, and so on) remains required
by the shared builder. `GenStem` forces `mv: "chirp-v3-0"`; it is not a normal
v5.5 song generation. Its response is the ordinary generation envelope:

```json
{
  "id": "<request id>",
  "clips": ["<normal clip objects>"],
  "clip_review_prompt_id": "<optional>"
}
```

Result clips are classified from `metadata.stem_task`,
`metadata.stem_type_group_name`, and `metadata.stem_name` (falling back to the
legacy `metadata.stem`). A result is a complement when
`stem_task == "remove"` or its group is `Instrumental`; `Instrumental` resolves
its base stem to `Lead Vocal`.

The exact current `StemTask` values are:

```text
extract, remove, two, eight, twelve, add, dry, wet
```

The exact current `StemGroup` values are:

```text
Vocals, Backing_Vocals, Drums, Bass, Guitar, Keyboard, Percussion,
Strings, Synth, FX, Brass, Woodwinds, Instrumental
```

Only `extract` and `remove` are classified as user-picked tasks. The current
bundle contains 240 canonical stem names. Default group-to-name resolution is:

| Group | Canonical default name |
|---|---|
| `Vocals` | `Lead Vocal` |
| `Backing_Vocals` | `Backing Vocals` |
| `Drums` | `Drum Kit` |
| `Bass` | `Bass` |
| `Guitar` | `Guitar` |
| `Keyboard` | `Keyboards` |
| `Percussion` | `Percussion` |
| `Strings` | `String Section` |
| `Synth` | `Synth` |
| `FX` | `Sound Effects` |
| `Brass` | `Brass Section` |
| `Woodwinds` | `Woodwinds` |

The 240-name catalogue is the protocol's canonical spelling source; a CLI
should copy/generate it from captured evidence rather than accept a guessed
free-form instrument and silently spend credits. Examples include
`12-String Guitar`, `808`, `Acoustic Guitar`, `Backing Vocals`, `Drum Kit`,
`Lead Vocal`, `String Section`, `Sound Effects`, `Synth`, and `Woodwinds`.

**CURRENT-BUNDLE, current lazy modal.** The current `2jel9qnsyfn3j.js` stem
modal confirms that every shown generation reference uses
`StemTypeId.FX == 91`. Auto invokes the builder as group `Twelve`, task
`twelve`, and expects banks of 12. Split from Mix invokes it with the selected
group, task `extract`, an expected target/complement pair of 2, and first
normalizes the selected group through the canonical `resolveExtractName`
mapping above. The current CLI therefore does not accept a guessed free-form
stem name. The Premier-only advanced loop also uses this same generation
reference shape, but its broader 240-name selection remains intentionally
unexposed to a Pro account.

**Recovery boundary.** This is a credit-bearing asynchronous generation, not a
pure file export. A lost response, invalid 2xx body, or disconnect after the
request may mean that the job was accepted. Preserve the transaction UUID,
recover via read-only request/clip/feed state, and never blindly replay. For
Pro, a safe first implementation is limited to bundle-proven Auto Split and
target/complement semantics; arbitrary Advanced Split must remain Premier-
gated until its current selection-to-ID request is captured.

### 2. Voices: complete current lazy-loaded workflow

**OFFICIAL.** [Voices](https://help.suno.com/en/articles/11362369) supports
three sources: a song already in the user's library, a real-time recording, or
an uploaded recording. Upload/record input is described as 15 seconds to four
minutes, with at most the best two minutes selected. If the source contains
backing music, Suno extracts a vocal stem. The user then reads a displayed
phrase and Suno compares both the voice and spoken words. The user may supply a
name, image, and singer skill level, must affirm the rights, and must satisfy
the age and geographic restrictions.

[Voices FAQ](https://help.suno.com/en/articles/11362433) says Voices replace
the old Create-menu Personas (Style Personas remain), require v5.5 for song
creation, and recommend high Audio Influence. Only the Voice creator can create
new songs with it; sharing/remixing has separate restrictions.

**LIVE-READ.** The billing response exposes these server limits:

```text
audio_upload_limits: min 6 s, max 1800 s
voice_record_limits: min 10 s, max 240 s
voice_upload_limits: min 10 s, max 900 s
```

These are not identical to the 15-second/four-minute Voice workflow documented
by the product. More importantly, the current bundle exports
`VOX_MIN_SECONDS=10` and `VOX_MAX_SECONDS=240`, uses those constants in the
active trimmer, and labels the maximum as four minutes. Its upload handler
accepts a source at three seconds before opening that trimmer; for a source
shorter than ten seconds the complete source is selected. Therefore the
official “best two minutes” wording is stale relative to the current Web
implementation: this CLI follows the current 3-second source / dynamic
10-to-240-second trimmer behavior and still treats the server as authoritative.
The UI is additionally gated by `voices-geo`, an underage check, and the
`personas-audio-upload` availability flag.

**CURRENT-BUNDLE, current-page lazy chunk.** The dynamically loaded
`155krfj46sodd.js` chunk confirms the preprocessing and verification workflow
that was absent from the first-load chunks.

The singing/source sample is first trimmed locally. Only that trimmed buffer is
converted to a WAV `File` and uploaded through the existing audio upload
workflow with `type: "voice_recording"`; the original untrimmed audio is not
uploaded. The client then requests vocal processing:

```http
POST /api/processed_clip/voice-vox-stem
Content-Type: application/json

{
  "upload_id": "<voice source upload id>",
  "vocal_start_s": 0,
  "vocal_end_s": 120.0
}
```

`120.0` above is an illustrative numeric duration; the actual value is the
selected duration rounded to two decimal places.

The response fields required by the flow are `id` (the processed audio ID) and
`voice_recording_id`. It polls the processed audio once per second, at most 120
times:

```http
GET /api/processed_clip/{processed_clip_id}
```

`completed` and `complete` are terminal success values; `failed` and `error`
are terminal failures. Exhausting 120 polls is a processing failure.

The verification phrase is server-selected for the chosen language:

```http
GET /api/voice-verification/phrase/?language=<language code>
```

The flow requires both `phrase_text` for display and `phrase_id` for the later
verification request. The exact language codes offered by this chunk are `en`,
`es`, `fr`, `pt`, `de`, `ja`, `ko`, `zh`, `hi`, and `ru`. The UI records for
15 seconds, converts the recording to WAV, and uploads it with
`type: "voice_recording"`. It then creates the verification recording through
the same processor with a distinct body:

```http
POST /api/processed_clip/voice-vox-stem
Content-Type: application/json

{
  "upload_id": "<verification upload id>",
  "recording_type": "verification"
}
```

The required response field for this branch is `voice_recording_id`; it becomes
`verification_recording_id` below. Voice/phrase comparison starts with:

```http
POST /api/voice-verification/
Content-Type: application/json

{
  "voice_recording_id": "<processed singing/source recording id>",
  "verification_recording_id": "<processed verification recording id>",
  "phrase_id": "<phrase id>"
}
```

The initial response consumes `id`, `status`, and, on rejection,
`rejection_reason`. When `status == "pending"`, the client polls at 1.5-second
intervals up to 40 times:

```http
GET /api/voice-verification/{verification_id}
```

Any status other than `pending` stops polling. `approved` is success; the
approved response `id` is retained as the final `verification_id`. The current
UI recognizes `didnt_say_verification_phrase` as a specific rejection reason.
Still pending after about 60 seconds is presented as a verification timeout.

The generic, fully evidenced finalization call is:

```http
POST /api/persona/create/
```

The body builder includes only fields whose values were provided:

```json
{
  "root_clip_id": "<optional clip id>",
  "name": "<optional; empty is localized Untitled>",
  "description": "<optional; empty becomes an empty string>",
  "image_s3_id": "<optional>",
  "is_public": "<optional boolean>",
  "is_suno_persona": "<optional boolean>",
  "persona_type": "<optional>",
  "vox_audio_id": "<optional>",
  "vocal_start_s": "<optional number>",
  "vocal_end_s": "<optional number>",
  "user_input_styles": "<optional>",
  "source": "<optional>",
  "singer_skill_level": "<optional>",
  "clips": "<optional>",
  "is_voice_recording": "<optional boolean>",
  "voice_recording_id": "<optional>",
  "verification_id": "<optional>"
}
```

The client refuses to submit unless either `root_clip_id` is present or
`is_voice_recording` is true. A successful response is a non-empty Persona/
Voice object and causes the user's Persona list to be reset. HTTP 409 is
interpreted as “already exists for clip,” which is also a useful recovery
signal.

The current Voice details screen invokes that builder with this narrower exact
shape (fields marked optional are omitted when absent):

```json
{
  "is_voice_recording": true,
  "voice_recording_id": "<processed singing/source recording id>",
  "name": "<trimmed non-empty name>",
  "description": "<trimmed description or empty string>",
  "is_public": false,
  "persona_type": "vox",
  "source": "<library_song or random_song>",
  "user_input_styles": "<optional trimmed styles>",
  "singer_skill_level": "<optional>",
  "verification_id": "<optional approved verification id>",
  "image_s3_id": "<optional image data URL>",
  "vox_audio_id": "<optional completed processed audio id>",
  "vocal_start_s": 0,
  "vocal_end_s": 120.0
}
```

`source` is `library_song` only for the lazy flow's `library` source value and
`random_song` otherwise. The optional `vox_audio_id`, `vocal_start_s`, and
`vocal_end_s` are added together after processed audio completes. The avatar
editor can use `/api/gen/prompt_image/` as documented in the cover-media
subsection; it downloads the returned image and converts it to a data URL
before passing it as `image_s3_id`.

The exact non-empty `singer_skill_level` choices emitted by the current UI are
`Beginner`, `Intermediate`, `Advanced`, and `Professional`; Skip omits the
field. As above, `120.0` is illustrative and the actual `vocal_end_s` is the
selected duration rounded to two decimal places.

The current input `maxLength` values are 80 UTF-16 code units for name, 256 for
styles, and 2000 for description. When the `voices-biometric-consent` gate is
enabled, all source choices remain disabled until the user explicitly consents
to collection and processing of voice data that may be biometric under Suno's
Terms and Privacy Policy. `voices-training-consent` changes the disclosed use
text (including possible service/model training according to the user's
choice), but does not add another checkbox or submit gate in this bundle.

**CURRENT CLI BOUNDARY.** `voice create` accepts only structurally valid WAVs.
Because the CLI does not silently re-encode or retain a second locally trimmed
asset, the singing-sample WAV must already be trimmed: its actual duration,
rounded to two decimal places, must equal `--sample-duration`. Before the first
upload the CLI requires the active `persona` plan feature, refetches the
selected language's current phrase and matches its exact ID, and separately
requires rights, 18+/region/audio-upload eligibility, and biometric-processing
confirmations. The latter flags are explicit user attestations because Statsig
geo/age/consent state has no authenticated read API captured here; server-side
checks remain authoritative. The processor poll is bounded to 1 second x 120,
verification to 1.5 seconds x 40, and final Persona detail uses a short bounded
read-only convergence poll. Every returned server identity is atomically
written to a private managed checkpoint. The checkpoint is inspection
evidence, not a verified resume command, so status/detail recovery is marked
non-resumable unless an actual safe mutation is available.

**Recovery boundary.** Every upload, processing, verification, and final-create
step is a mutation boundary. Persist the returned upload, processed-audio,
voice-recording, phrase, verification, and Persona IDs as they become known.
After a lost processor response, inspect the upload/processed resource before
re-uploading. After a lost verification POST response, an ID is unavailable,
so do not record/re-submit automatically. After a lost final Persona response,
recover through Persona list/detail and treat HTTP 409 as possible prior
success before replaying. List/detail/manage operations continue to use the
current Persona routes documented elsewhere.

### 3. Custom Models and My Taste are separate features

**OFFICIAL.** [Custom Models](https://help.suno.com/en/articles/11362497) is a
Pro/Premier feature. The user can have up to three models, training needs at
least six songs (library selection and bulk upload are supported), the user
must own the rights, typical readiness is two to five minutes, and models are
private. [The v5.5 overview](https://help.suno.com/en/articles/11362305)
describes Custom Models as a way to personalize v5.5.

**LIVE-READ / CURRENT-BUNDLE.** The account has `custom_models` enabled. The UI
also requires the `custom-model-ui` gate. Current bundle validation requires at
least six resolved clip IDs, a name of 1 to 16 characters, and no active uploads. It allows
up to 100 selected songs in the normal flow (200 for an Artist-plan branch) and
shows a 100-credit cost. The local UI contains experimental max-model branches
(`unlimited`, otherwise 10 for one VIP branch, otherwise 3), but the official
Pro/Premier limit is three and server/account state is authoritative.

**EXPLICIT-ATTESTATION CLI BOUNDARY.** The bundle proves the `custom-model-ui`
gate name, but this audit did not capture an authenticated API request/response
field that exposes its value. `accessible_features: ["custom_models"]` therefore
does not prove the second gate. Training requires both that live entitlement and
`--confirm-ui-available`, which the user may pass only after visibly confirming
that the current Suno Web account exposes Custom Model training. Without the flag
the CLI sends no training POST. It does not guess a Statsig body; the Suno server
remains authoritative for final eligibility and charging.

Create training:

```http
POST /api/custom-model/create/
Content-Type: application/json

{
  "clip_ids": ["<clip id>", "..."],
  "name": "<trimmed name, fallback Custom Model>"
}
```

The response field consumed by the client is the required top-level `id`. A
successful submit invalidates both billing/subscription model data and
pending-model state.

Pending training state:

```http
GET /api/custom-model/pending/
```

```json
{
  "has_pending": true,
  "pending_models": [
    {"id": "<model id>", "name": "<model name>"}
  ]
}
```

Both fields are treated as optional (`false` and `[]` defaults). The query
retries three times and polls every 15 seconds while pending rows exist or the
query is in error. A `true -> false` transition invalidates billing data and
shows the ready notification.

After an accepted create/archive response, CLI business verification follows
the same eventual-consistency boundary with a bounded 45-second, 15-second-
interval read-only loop over pending and billing state. The write is issued
exactly once; exhaustion preserves the operation/model IDs for later GET-only
inspection.

The Web action labeled Delete is an archive mutation for both active and
pending rows:

```http
POST /api/custom-model/archive/
Content-Type: application/json

{"id": "<model id>"}
```

The current client does not inspect a response body before invalidating billing
and pending state. Ready Custom Models are not returned by a separate list
route in the current first-load bundle; they are model rows in
`GET /api/billing/info/`. A `custom` badge alone is not an archive identity seam
because a base model can also carry it. CLI archive accepts either an exact
pending-model ID or a billing row whose exact `extra.id`, `chirp-custom...`
external key, and `custom`/`training` badge agree; otherwise it fails closed.
Selection uses the billing model's current `external_key`. No current separate
detail, rename, restore, or hard-delete endpoint was found. Therefore “delete”
should be named `archive` at the protocol layer and no claim of permanent
deletion or recoverability should be made.

**OFFICIAL / CURRENT-BUNDLE.** [My Taste](https://help.suno.com/en/articles/11362561)
is available to all users and augments styles through the Magic Wand. It is not
a Custom Model, does not train from selected clips, and has no Custom Model ID.
Its independent settings contract is:

```http
GET /api/personalization/settings
POST /api/personalization/settings
```

The GET response field used by the client is `styles_augmentation` (defaulting
to true). The POST body is:

```json
{"styles_augmentation": true}
```

The UI updates optimistically, rolls back on error, and invalidates the setting
after success. Personalized tag/style enhancement records personalization
state in its own response/submission metadata; it does not attach a Custom
Model to the training protocol.

**Recovery boundary.** A missing model ID, unusable 2xx response, or transport
failure after `custom-model/create` is ambiguous. Read pending state and then
billing models before any replay. The same rule applies to archive: re-read
pending and billing state first. Uploading training sources uses the existing
upload mutation and its own recovery rules; completion of uploads does not
prove training submit success.

### 4. Lyrics Projects, selection rewrite, mashup, and cover media

#### Lyrics Projects and the improved editor

**CURRENT-BUNDLE.** Lyrics Projects are now the persisted backing store for
saved Lyrics 2.0 drafts. The exact CRUD/flush contract is:

```http
GET    /api/lyrics-projects?limit=<default 50>&sort=<default updated_at>&cursor=<optional>
POST   /api/lyrics-projects
GET    /api/lyrics-projects/{project_id}
PATCH  /api/lyrics-projects/{project_id}
DELETE /api/lyrics-projects/{project_id}
POST   /api/lyrics-projects/{project_id}/flush
```

List pagination appends `projects` until `next_cursor` is null. Create and
rename truncate by Unicode code points to 200 characters:

```json
{"title": "<at most 200 characters>"}
```

Create, get, and patch return the project object. Fields consumed by the client
are `id`, `title`, `lyrics`, `created_at`, and `updated_at`. Delete requires
only an HTTP-success response. Flush accepts:

```json
{"lyrics": "<full current lyrics>"}
```

and returns at least `updated_at`; the browser may send it with Fetch
`keepalive: true`. Manual-lyrics song generation may include
`lyrics_project_id`, linking the generated song back to the saved project.
Project CRUD/flush itself is persistence, not music generation.

Selection rewrite/enhance is synchronous with a 30-second client timeout:

```http
POST /api/generate/lyrics-infill/
```

```json
{
  "prompt": "<instruction>",
  "context_lyrics_prefix": "<text before selection>",
  "context_lyrics_edit": "<selected text>",
  "context_lyrics_suffix": "<text after selection>",
  "create_session_token": "<session token>",
  "title": "<title>"
}
```

The response fields consumed are `generated_lyrics`, `lyrics_request_id`, and
`lyrics_id`. HTTP 400 with detail `Lyrics too long to enhance.` has a dedicated
error branch.

Two-source lyrics mashup starts with:

```http
POST /api/generate/lyrics-mashup
```

```json
{
  "lyrics_a": "<first lyrics>",
  "lyrics_b": "<second lyrics>",
  "create_session_token": "<session token>",
  "source": "create_ui"
}
```

The reusable hook allows `source: null`; the two-clip helper uses the literal
`create_ui`. The response fields consumed are `lyrics_request_id` and
`mashup_id`. Poll the latter as `{lyrics_id}` every 2.5 seconds:

```http
GET /api/generate/lyrics/{lyrics_id}
```

Terminal `status` values are `complete` and `error`. Completion consumes
`text`, `title`, and `id`; error consumes `error_message`. One helper bounds at
60 polls (about 150 seconds), while the general hook cancels after 90 seconds,
so a CLI should expose an explicit bounded wait rather than assume one global
server timeout.

**CURRENT CLI BOUNDARY.** `create --lyrics-project-id <id>` is accepted only
with explicit custom lyrics. Before any generation write, Sunox performs an
exact project GET and rejects an identity mismatch; the unchanged ID is then
sent on the generation body. Lyrics mashup submission and observation are
separate: the submit command waits on a configurable deadline (150 seconds by
default, 2.5-second interval) unless `--no-wait` is given, while
`mashup-status` only observes an already-known ID and never claims it submitted
the job. On `mashup-status`, `--timeout` is valid only together with `--wait`.
Transport ambiguity is not automatically replayed.

**Recovery boundary.** Create/flush/rename/delete are account writes. If a
flush response is lost, GET the project and compare the exact intended lyrics
before retrying. If create loses its returned ID, list newest projects and
compare stable fields rather than blindly creating duplicates. Mashup submit is
ambiguous after send; poll a returned ID, and if no ID arrived do not replay
without user confirmation because no client transaction UUID is present in
this body.

#### Song cover image and video generation

**LIVE-READ / CURRENT-BUNDLE.** The Pro account has both
`generate_song_image` and `generate_song_video`. The song action is named
`generate_cover_art` and is limited to an owned, non-trashed clip plus its
server-supplied action gate.

For the direct image composition, that server-supplied action is the confirmed
eligibility seam. A top-level `download_disabled_reason` belongs to download
policy and must not override an explicitly enabled `generate_cover_art` action.

**CURRENT-BUNDLE, current-page lazy chunk.** The direct prompt-image contract
is fully confirmed in `155krfj46sodd.js`:

```http
POST /api/gen/prompt_image/
Content-Type: application/json

{"prompt": "<image description>"}
```

The response field consumed by the client is `image_url`:

```json
{"image_url": "<generated image URL>"}
```

The current Voice avatar UI limits the prompt to 200 characters and disables
submit for an empty string. It uses the returned URL directly for preview,
then fetches the image and converts it to a data URL for Voice finalization.
For a song, this direct route composes with the independently live-verified
metadata mutation:

```http
POST /api/gen/{clip_id}/set_metadata/
Content-Type: application/json

{"image_url": "<image_url returned by prompt_image>"}
```

This is the direct `prompt_image + set_metadata` path and is distinct from the
new multi-result `SONG_COVER_ART` batch modal. The uploaded-image + S3 +
`set_metadata` path documented above is a third variant that starts from local
bytes rather than an AI prompt.

**CURRENT-BUNDLE, new batch submit confirmed 2026-08-24.** Suno's official
July 31 release note describes the current Web feature as iterative image
editing from text or a dropped image, producing either an image or a video:
<https://suno.com/release-notes/cover-art-improvements>. The exact transport is
confirmed by the current `/create` deployment chain rather than inferred from
that product description: `2y5thy224ncxq.js` loads the app-modal registry
`09tvl580d4za9.js`; its `SONG_COVER_ART` entry resolves loader `150515` from
`02jx6qed7xt_n.js`, which loads `3fb41b6lf417k.js`, `0td69sloxk6wz.js`,
`25suq96jo2z-y.js`, and `3ywrez2jy646l.js`. The API hooks and exact request
builders are in `0td69sloxk6wz.js`; the modal composition, model/cost selection,
history, and apply workflow are in `3fb41b6lf417k.js` and
`25suq96jo2z-y.js`.

The song modal opens with `supportsVideo: true`, an owned clip ID, and fixed
square output. Image generation submits:

```http
POST /api/video_gen/image/generate
Content-Type: application/json
```

```json
{
  "generated_text_id": "<optional prior text-generation id>",
  "prompt": "<trimmed prompt, hard-sliced to 800 UTF-16 code units>",
  "clip_id": "<song clip id>",
  "quantity": 2,
  "image_gen_category": "<category returned by model-configs>",
  "prompt_images": [
    {"id": "<image id>", "type": "uploaded|generated|s3_filename"}
  ],
  "aspect_ratio": "1:1"
}
```

`generated_text_id`, `image_gen_category`, `prompt_images`, and
`aspect_ratio` are optional at the reusable hook layer and are omitted rather
than sent as JSON `null`; the song modal supplies the category and `1:1`.
`prompt_images` is omitted when empty. Local image attachments first use the
already-confirmed `/api/uploads/image/` presigned upload workflow and then use
`{"id":"<upload id>","type":"uploaded"}`. Iterating an existing generated
result uses `type: "generated"`; an initial S3 filename derived from existing
song art uses `type: "s3_filename"`. The current UI allows submission with
either nonblank prompt text, at least one attached image, or a selected prompt
suggestion. Image attachment count is one by default and four only when the
server-delivered `MULTI_IMAGE_I2I_ENABLED` Web parameter is true.

Video generation is a separate route and a different body:

```http
POST /api/video_gen/video/generate
Content-Type: application/json
```

```json
{
  "generated_text_id": "<optional prior text-generation id>",
  "prompt_start_image": {"id": "<image id>", "type": "uploaded|generated|s3_filename"},
  "clip_id": "<song clip id>",
  "prompt": "<prompt>",
  "quantity": 2,
  "video_gen_category": "<category returned by model-configs>",
  "duration": 5,
  "clip_start_time": 0.0,
  "clip_end_time": 30.0,
  "aspect_ratio": "1:1"
}
```

The reusable hook omits absent optional fields. It defaults `quantity` to two
and `duration` to five seconds. The song modal supplies its clip ID, the
selected server category, `1:1`, and at most the first attached image as
`prompt_start_image`. Allowed video durations are not a stable client enum:
they come from the selected `/api/video_gen/model-configs` entry's
`allowed_durations`, or `allowed_durations_with_image` when an image is
attached. A model whose `image == "not_supported"` is excluded from the
image-to-video choice.

Both submit responses are batch handles. The client requires `batch_id` for
polling and consumes `image_ids` from an image response or `video_ids` from a
video response for event/result identity:

```json
{"batch_id":"<batch id>","image_ids":["<image id>"]}
```

```json
{"batch_id":"<batch id>","video_ids":["<video id>"]}
```

Applying a selected batch result also differs by media type. For an image, the
modal uses the generated image ID rather than copying its URL:

```http
POST /api/gen/{clip_id}/set_metadata/

{
  "cover_image": {"id": "<generated image id>", "type": "generated"},
  "cover_art_session_id": "<client session UUID>"
}
```

For a video, the polled/history item must contain `video_upload_id`; applying
it sends:

```http
POST /api/gen/{clip_id}/set_metadata/

{
  "video_cover_upload_id": "<video_upload_id>",
  "cover_art_session_id": "<client session UUID>"
}
```

The video apply response is consumed for `image_url`, `video_cover_url`, and
`preview_url`. The `cover_art_session_id` is generated client-side for event
and workflow correlation; it is not a submit idempotency key.

The optional prompt-enhancement call is not itself a media submit:

```http
POST /api/video_gen/text/generate
```

```json
{
  "clip_id": "<optional clip id>",
  "target": "image|video",
  "user_prompt": "<prompt>",
  "image_url": "<optional image URL>",
  "clip_start_time": 0.0,
  "clip_end_time": 30.0,
  "duration": 5
}
```

The numeric timing fields in both examples are optional illustrative values,
not fixed defaults; absent values are omitted.

**CURRENT-BUNDLE, dynamic model and credit gates.** Categories must be fetched
at runtime; they must not be hard-coded:

```http
GET  /api/video_gen/model-configs
POST /api/video_gen/cost/image
Body: {"image_gen_category":"<category>","prompt":""}
POST /api/video_gen/cost/video
Body: {"video_gen_category":"<category>","duration":<selected seconds>}
```

`model-configs` returns `image_model_categories` and
`video_model_categories`; the UI consumes each entry's `category`,
`display_name`, `description`, image-input support, and the video duration
arrays described above. Both cost responses consume `cost` and optional
`remaining_gens`. The submit hooks classify HTTP 402 as either
`creation_limit_reached` or insufficient credits, HTTP 429 as rate limiting,
and `error_type == "moderation_error"` as moderation rejection. These server
responses remain authoritative even after a successful cost read.

For a song, the Web entry point additionally requires that the session user
owns the clip, the clip is not trashed, and the server-supplied
`generate_cover_art` action is visible and not disabled. A 2026-08-24 read-only
account check confirms that this Pro plan advertises both
`generate_song_image` and `generate_song_video`; those account features do not
replace the per-clip action check.

**CURRENT-BUNDLE, new batch read/recovery side.** The first-load bundle
confirms these new cover-art batch calls:

```http
POST /api/video_gen/pending_batches
Body: {}
```

The response field is `batch_ids`. Each entry is a batch descriptor with at
least `id` and `type`, not a bare string. The global notification path filters
entries whose `type == "video"`; the cover-art library independently derives
the same descriptor shape from history as
`{"id":"<batch id>","type":"image|video"}` (normalizing the historical
`image-to-video` type to `video`). The filtered descriptors are sent unchanged
to:

```http
POST /api/video_gen/poll_batches
Body: {"batch_ids": [{"id": "<batch id>", "type": "image|video"}]}
```

The response is:

```json
{
  "batches": {
    "<batch id>": [
      {
        "id": "<generated image or video id>",
        "clip_id": "<source clip id>",
        "type": "image|video|image-to-video",
        "status": "processing|complete|error",
        "url": "<optional completed media URL>",
        "thumbnail_url": "<optional generated preview>",
        "video_upload_id": "<optional ID used when applying a video cover>",
        "start_frame_url": "<optional video start frame>",
        "prompt": "<optional prompt>",
        "gen_category": "<optional model category>",
        "duration": 5
      }
    ]
  }
}
```

The global notifier polls every seven seconds and groups results by source
`clip_id` and batch ID. The modal library uses a three-second poll. A batch is
still processing while any item has `status == "processing"`; item terminal
states are `complete` and `error`. Only completed items with a media URL enter
the apply/download carousel. In addition to pending recovery, the current modal
loads its batch history with:

```http
POST /api/video_gen/history
Content-Type: application/json

{
  "clip_id": null,
  "created_at_offset": null,
  "favorites_only": false,
  "media_type": null,
  "limit": 20
}
```

`created_at_offset` becomes the preceding page's last `created_at`; filters may
set `favorites_only` and `media_type` to `image` or `video`. The response field
is `history`. Each batch contains `batch_id`, `type`, `prompt`, `gen_category`,
`created_at`, and `items`; the item fields consumed by the current client are
the polling fields above plus `is_liked`, `clip_start_time`, and
`clip_end_time`.

It also exposes current generated-media library reads:

```http
GET /api/project/library/images?limit=30&cursor=<optional>
GET /api/project/library/videos?limit=30&cursor=<optional>
```

Their response fields are respectively `images` / `videos` and
`next_cursor`.

The older non-AI song-video regeneration/download manager remains independently
current:

```http
POST /api/video/generate/{clip_id}/
GET  /api/video/generate/{clip_id}/status/
```

The POST has no request body used by the client. Status is polled every four
seconds. `status == "complete"` is terminal success; the response fields used
are `video_url` and `video_is_stale`. Exhausting the caller's bounded retry
counter is reported as timeout.

The current bundle/captured responses do not, however, expose an exact
per-clip action name or ownership/download predicate that authorizes this old
video POST. Knowing the route and the account-level `generate_song_video`
feature is insufficient to prove a particular clip is eligible. The CLI thus
keeps `clip video-status` as a bounded read-only surface and fail-closes
`clip generate-video` before POST until that eligibility seam is captured; it
does not guess an action name or interpret opaque ownership metadata.

**CURRENT CLI IMPLEMENTATION.** `clip cover-art` now exposes dynamic model
discovery, pending/history/status reads, distinct two-result image/video batch
submits, and explicit image/video apply commands. Writes fail closed unless the
JWT account subject exactly matches the clip `user_id`, the clip is explicitly
non-trashed, `generate_cover_art` is visible and enabled, and the matching live
plan feature is present. Image/video submits fetch live categories, allowed
durations, and cost first; reject prompts at or beyond the Web's 800 UTF-16
unit hard-slice boundary rather than silently changing caller text; validate the attachment shape;
preserve the returned `batch_id` plus media IDs; and use bounded
`poll_batches` recovery. Generation never auto-applies the first candidate.
Apply requires the caller to provide that batch ID and first proves that the
exact completed image ID or video upload ID belongs to the selected clip.
No current submit body contains a client idempotency key, so a lost submit
response is reported as ambiguous and is not replayed automatically.

**Recovery boundary.** A lost `prompt_image` response is ambiguous because the
image may already have been generated and the contract exposes no client
transaction UUID; do not spend again automatically. Once `image_url` is known,
a failed/lost metadata mutation must first be investigated by reading the clip
and comparing its exact `image_url`. The current CLI marks that mutation
non-resumable and does not promise that replaying even the metadata-only step is
safe. A lost response
after the older video POST is also ambiguous; GET status before replay. For the
new batch system, pending-batch discovery and batch polling are the intended
read-only recovery surfaces, but without a confirmed submit response it is not
yet known whether they can always recover a batch whose submit response was
lost.

## Key Insights for Rust CLI

1. **Captcha/challenge is conditional** — `POST /api/c/check` with `{"ctype":"generation"}` decides whether generation needs a solved token. The CLI mirrors this preflight before `/api/generate/v2-web/` submits. If the preflight reports a challenge and stored Clerk refresh material exists, the CLI refreshes the JWT once and repeats the preflight. A remaining challenge is solved silently using hCaptcha/provider 1 or Cloudflare Turnstile/provider 2 according to `captcha_version`; normal authenticated submits omit `token` and `token_provider`.
2. **Standalone lyrics uses Cowrite** — both model discovery and the synchronous JWT-authenticated `POST /api/generate/cowrite-lyrics/` body/response are current-confirmed by the August 24 interaction chunk and a minimal submission. The CLI does not promise that it is free or permanently exempt from server-side anti-abuse checks.
3. **JWT refresh** — need Clerk cookie exchange or session keepalive
4. **Browser-token header** — dynamically generated from current timestamp, base64-encoded
5. **Browser environment** — browser-cookie extraction records a stable browser source id (`chrome`, `arc`, `brave`, `firefox`, or `edge`) and best-effort public profile settings such as `accept-language`; it does not fabricate a `user-agent` from that label. Interactive login captures stable runtime headers such as `user-agent` and `accept-language`. API calls reuse captured fields independently, derive Chromium client hints from the selected `user-agent`, send the stable browser fetch metadata headers observed in HARs, and fall back field-by-field when unavailable.
6. **Cookie-based approach** — store Clerk session cookies, exchange for JWT via `auth.suno.com/v1/client/sessions/<session_id>/tokens`
7. **`feed/v3` is cursor-based** — the current web request uses `cursor`, `limit`, and scenario-specific filters, not numeric pages
8. **Two auth strategies**:
   a. Cookie-based: store the Clerk client cookie and auto-refresh JWTs
   b. Direct JWT: User pastes JWT, works for ~1 hour (simpler but expires)

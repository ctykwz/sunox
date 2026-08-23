# Sunox / Suno Web Deep Protocol Audit — 2026-08-23

## Executive summary

The current Suno Web create client still uses `POST /api/generate/v2-web/`,
`POST /api/c/check`, the v3 feed, the v2 playlist routes, and the existing
audio-upload/edit routes used by Sunox. The August 23 changes for
`metadata.web_client_pathname: "/create"`, `task: "upload_extend"`, Cowrite
model discovery, and `audio_weight` match the current bundle.

The deeper pass found five confirmed actionable gaps. All five were corrected
in the working tree covered by this report. It also found one high-risk
Persona-generation mismatch that requires a credit-bearing capture:

1. **Model selection for Extend and Inspire is not Web-compatible.** Both
   commands construct a request with `chirp-fenix`, while the Web builder uses
   the active/account-selected model and its task capabilities. Accounts that
   cannot use v5.5 fail even if another model can perform the task.
2. **Uploaded playlist cover payload has drifted.** Current Web sends only
   `metadata.cover_image_s3_id` for an uploaded cover; Sunox also fabricates a
   `cdn2.suno.ai` URL and sends `cover_is_user_set: true`.
3. **Audio upload constraints are missing locally.** Current Web accepts only
   `mp3`, `m4a`, `wav`, `flac`, `ogg`, and `aac`, with a 524,288,000-byte
   client-side limit. Sunox accepts any non-empty extension and does not reject
   an oversized file before creating an upload.
4. **Playlist list normalization can emit duplicate JSON keys.** The current
   flat list response includes `cover_is_user_set`; Sunox leaves it in the
   flattened `extra` map while also serializing its normalized field. The live
   CLI output contained the key twice.
5. **Task/condition compatibility was not enforced.** Billing exposes both
   `capabilities` and exact `allowed_condition_combinations`; Sunox previously
   retained the latter only as opaque extra data.

Separately, Sunox's ordinary `create/describe --persona` request only supplies
`persona_id`, whereas the current advanced Create builder resolves a Persona
reference to `task: "vox"` or `task: "artist_consistency"` and may attach the
source clip and time range. An agentic bare-Persona path also exists, so this is
not yet proof that Sunox's request is rejected; confirming the intended contract
requires an authorized credit-consuming generation capture.

A second implementation review and authenticated readback also tightened four
edges: Persona mutations were restored to normal transport negotiation because
only reads had evidence; nullable `allowed_condition_combinations` now follows
the Web `?? []` behavior; Cowrite POST evidence is labelled as the older
2026-07-26 compatibility capture rather than current confirmation; and the
machine-readable playlist-cover guidance now matches the S3-ID-only payload.
Intermittent resets were observed on both negotiated HTTP/2 and forced HTTP/1.1,
so the transport mitigation is a bounded fallback for explicit idempotent GETs,
not a protocol downgrade for any mutation.

No generation, upload, metadata change, deletion, reaction, playlist mutation,
or credit-consuming endpoint was called during this pass.

## Scope and evidence levels

The repository baseline is commit
`c4383fabf9c0fde0bb902e2f1a8dad7f5b94a326` (`Sync current Suno web protocols`).

Evidence is labelled as follows:

- **BUNDLE** — first-party code downloaded from `https://suno.com/create` on
  2026-08-23. Byte offsets below refer to the raw immutable chunk.
- **LIVE-READ** — authenticated request that does not create, update, delete,
  upload, react, or consume credits.
- **REPO** — current Rust request/response implementation or tests.
- **INFERENCE** — a conclusion that cannot be safely live-tested without a
  mutation or credit risk.

Relevant immutable bundle fingerprints:

| Chunk | SHA-256 | Main evidence |
|---|---|---|
| `2vyct5q4gq553.js` | `7f54ece7f0888ff20c038a97931b78ba91e8aa8f3ae16e229159f678b2ac26ab` | generation builder, task resolver, Cowrite models, edit and aligned-lyrics flows |
| `3u1ycshr_o1-2.js` | `5ed94f918b62041bc63785bf83847f8decdfb478760223e710afddb54c623265` | v2 playlist mutations |
| `0f_f8o2o_ba96.js` | `3a64a1de0dc5c543f4a26cafd5adba8f25c367596b8517430837d3e2e8c0fe3f` | audio upload workflow |
| `1dj8w_ebqiles.js` | `0e399b9b5ff5045324c7c4a68ca4508f04c857aa5449e17d494d7eb193750488` | challenge preflight and provider selection |
| `1vw1tt48qlmdn.js` | `2c7dc982024a406bec64a5e4ff167b13e32b53bfccae3782fdefe28d10acd0d0` | clip mutation, concat/remaster and polling store |
| `2o55p_0ruo1em.js` | `abe3ea475ec66dfd8735d49d1a08c4c3fa9fcbab1299655323d4d9e9d5fd909e` | `GEN_ENDPOINT` constant |

These files were captured between 20:08 and 20:13 Asia/Shanghai. The official
source surface is [Suno Create](https://suno.com/create); chunk names and hashes
are included because immutable asset URLs can later disappear.

## Repository endpoint baseline

The following is the complete application API surface under `src/api`, grouped
by protocol family. Presigned S3 form uploads are shown separately because they
do not use `studio-api-prod.suno.com` after the presign step.

| Family | Method and route | Audit state |
|---|---|---|
| Account | `GET /api/billing/info/` | **LIVE-READ stable**; current model, feature, limits and remaster arrays decoded |
| Challenge | `POST /api/c/check` | **BUNDLE stable**; body remains `{ctype:"generation"}` |
| Feed | `POST /api/feed/v3` | **LIVE-READ stable**; cursor response decoded |
| Clip | `GET /api/clip/{id}` | **LIVE-READ stable** |
| Clip detail | `GET /api/clips/{id}/attribution` | **LIVE-READ stable** through `clip info` |
| Clip detail | `GET /api/gen/{id}/comments?order=most_liked` | **LIVE-READ stable** through `clip info` |
| Clip detail | `GET /api/clips/remixes/count?clip_id={id}` | **LIVE-READ stable** through `clip info` |
| Clip detail | `GET /api/clips/get_similar/?id={id}` | **LIVE-READ stable** through `clip info` |
| Generation | `POST /api/generate/v2-web/` | **BUNDLE stable**, mutation not live-tested |
| Tag enhance | `POST /api/prompts/upsample` | Present in bundle; mutation-like POST not live-tested |
| Cowrite | `GET /api/generate/cowrite-lyrics/models/` | **LIVE-READ stable**; deliberately invalid local model stopped before submit |
| Cowrite | `POST /api/generate/cowrite-lyrics/` | **Unverified**; not present in the downloaded create chunk graph except as the models-prefix string |
| Concat | `POST /api/generate/concat/v2/` | **BUNDLE stable**, mutation not live-tested |
| Remaster | `POST /api/generate/upsample` | **BUNDLE stable**, credit-risk mutation not live-tested |
| Speed | `POST /api/clips/adjust-speed/` | **BUNDLE payload stable**, mutation not live-tested |
| Reverse | `POST /api/clips/reverse-clip/` | **BUNDLE payload stable**, mutation not live-tested |
| Crop/cut | `POST /api/edit/crop/{id}/` | **BUNDLE payload stable**, mutation not live-tested |
| Fade | `POST /api/edit/fade/{id}/` | **BUNDLE payload stable**, mutation not live-tested |
| Edit poll | `GET /api/edit/action/{action_id}/` | **BUNDLE stable** |
| Timed lyrics | `POST`, then `GET /api/gen/{id}/aligned_lyrics/v3` | **BUNDLE stable**; v2 fallback also remains in bundle |
| Timed lyrics compat | `GET /api/gen/{id}/aligned_lyrics/v2` | **BUNDLE stable** |
| Clip trash | `POST /api/gen/trash` | **BUNDLE stable**, mutation not live-tested |
| Clip purge | `POST /api/clips/delete/` | **BUNDLE stable**, destructive and not live-tested |
| Clip metadata | `POST /api/gen/{id}/set_metadata/` | **BUNDLE stable**, mutation not live-tested |
| Clip visibility | `POST /api/gen/{id}/set_visibility/` | **BUNDLE stable**, mutation not live-tested |
| Clip reaction | `POST /api/gen/{id}/update_reaction_type/` | **BUNDLE stable**, mutation not live-tested |
| Download | `GET /api/download/clip/{id}?format=mp3|m4a` | Same-day read-only API evidence exists; current Web also has gated/presigned helpers and response `media_urls` |
| Download WAV | `POST convert_wav`, then `GET wav_file` | Current Web still has the WAV init/poll helper; conversion not live-tested here |
| Download OPUS | `GET opus_file`, optional `POST convert_opus` | Backend compatibility route; not found in this create chunk graph and not live-tested here |
| Playlist list | `GET /api/playlist/me?page=N` | **LIVE-READ observed-compatible**; flat cover fields decode to one normalized key after the fix |
| Playlist detail | `GET /api/playlist/v2/{id}` | **LIVE-READ stable**, v2 metadata/relationship/stats envelope decoded |
| Playlist create | `POST /api/playlist/create/` | Present in bundle, mutation not live-tested |
| Playlist metadata | `PATCH /api/playlist/v2/{id}` | **BUNDLE method/body matched after fix**; mutation not live-tested |
| Playlist legacy metadata | `POST /api/playlist/set_metadata` | Present as Web compatibility branch |
| Playlist reaction | `POST /api/playlist_reaction/{id}/update_reaction_type/` | Present as legacy branch; v2 like/save uses `POST .../save` |
| Playlist save | `POST /api/playlist/v2/{id}/save` | **BUNDLE stable** |
| Playlist unsave | `DELETE /api/playlist/v2/{id}/save` | **BUNDLE stable** |
| Playlist tracks | `POST .../tracks/add|remove` | **BUNDLE stable**, body remains `{clip_ids:[...]}` |
| Playlist reorder | `POST .../tracks/reorder-by-index` | **BUNDLE stable**, body remains `{positions:[...]}` |
| Playlist trash | `POST /api/playlist/v2/{id}/trash` | **BUNDLE stable**, body remains `{undo:boolean}` |
| Persona list | `GET get-personas|get-loved-personas|get-followed-personas` | **LIVE-READ observed-compatible** for mine/loved/followed; page/token responses decoded through bounded idempotent-GET fallback |
| Persona detail | `GET /api/persona/get-persona/{id}/` | **LIVE-READ observed-compatible** for one owned Persona; identity, visibility, and source-range fields decoded |
| Persona clips | `GET /api/persona/get-persona-paginated/{id}/?page=N` | **LIVE-READ envelope compatible** for an empty page; non-empty clip-item schema not live-verified |
| Persona create/edit/visibility/trash | `POST`, `PUT` persona routes | Mutation routes not live-tested; see suspected drift below |
| Persona love | `POST /api/persona/{id}/toggle_love/` | **BUNDLE stable**, mutation not live-tested |
| Audio upload | `POST /api/uploads/audio/`, S3 form, `POST upload-finish`, `GET status`, `POST initialize-clip` | **BUNDLE endpoint/body and local format/size validation matched after fix**; upload not live-tested |
| Image upload | `POST /api/uploads/image/`, S3 form, `POST upload-finish` | Routes remain in bundle; mutation not live-tested |

## Confirmed stable contracts

### Generation endpoint and base request

**BUNDLE:** `2o55p_0ruo1em.js` byte 17,766 defines
`GEN_ENDPOINT` as `/api/generate/v2-web/`. The request builder in
`2vyct5q4gq553.js` around bytes 1,862,500–1,879,000 retains:

- `generation_type: "TEXT"` in the base object;
- null placeholders for clip/reference fields;
- `override_fields: []`;
- `transaction_uuid`, `token`, `token_provider`, and the selected `mv`;
- `metadata.web_client_pathname = window.location.pathname`;
- `metadata.create_session_token`, `create_mode`, `user_tier`, and
  `disable_volume_normalization`.

Sunox now uses `/create`, and the main request structure remains compatible.

### Task resolution

**BUNDLE:** the task resolver in `2vyct5q4gq553.js` around bytes
1,874,000–1,879,500 selects `upload_extend` when an Extend reference has
`isUpload`, otherwise `extend`. It still resolves `cover`, `gen_stem`, and
`playlist_condition`, along with newer task combinations.

The August 23 `upload_extend` correction is therefore confirmed.

### Control sliders

**BUNDLE:** `2vyct5q4gq553.js` around bytes 1,867,300–1,868,200:

- no references: `weirdness_constraint` and `style_weight` are eligible;
- one or more references: `audio_weight` is additionally eligible;
- all percentage sliders are divided by 100;
- `aug_creativity` is only included when account flag `aug-creativity` is on.

Sunox's inspiration `audio_weight` normalization and optional
`aug_creativity` schema match this contract.

### Challenge protocol

**BUNDLE:** `1dj8w_ebqiles.js` byte 44,732 uses
`POST /api/c/check` with `{ctype: <type>}`. For generation, captcha version 2
selects Turnstile and other values normalize to hCaptcha. Sunox matches the
endpoint, body, and provider numbering.

### Edit operations

**BUNDLE:** `2vyct5q4gq553.js` around bytes 1,081,200–1,086,000 matches Sunox:

- speed: `{clip_id,speed_multiplier,keep_pitch,title}`;
- reverse: `{clip_id,title}`;
- crop/cut: `{crop_start_s,crop_end_s,is_crop_remove,title,ui_surface:"song_actions"}`;
- fade: `{fade_in_time?,fade_out_time?,title}`;
- edit completion: poll `GET /api/edit/action/{action_clip_id}/`.

### Timed lyrics

**BUNDLE:** `2vyct5q4gq553.js` around byte 1,530,500 uses the same v3 workflow:

1. `POST /api/gen/{clip_id}/aligned_lyrics/v3` with
   `{lyrics,enable_augmentation}`;
2. poll the same route with `GET` while state is `running`;
3. retain `GET .../aligned_lyrics/v2` compatibility behavior.

### Audio upload request sequence

**BUNDLE:** `0f_f8o2o_ba96.js` around bytes 48,000–55,000 matches the Sunox
sequence and bodies:

- presign body: `{extension,is_stem_mix,upload_type}`;
- finish body:
  `{upload_type,upload_filename,agreed_to_vip_upload_terms}`;
- status: `GET /api/uploads/audio/{upload_id}/`;
- initialize body is either `{}` or `{user_reviewed_tags:true}` depending on a
  Web gate.

## Confirmed issues fixed in this pass

### P1 — Extend and Inspire hard-code v5.5 instead of resolving an eligible model

**REPO:** `src/api/extend.rs` and `src/api/inspiration.rs` construct
`GenerateRequest::new("chirp-fenix", "custom")`. Neither CLI command exposes a
model option. Billing preparation then requires that exact external key to be
usable.

**BUNDLE:** the current builder derives `mv` from the active model tier or
`modelOverride`; mappings are v3/v3.5/v4/v4.5/v4.5+/v5/v5.5 to
`chirp-v3-0`, `chirp-v3-5`, `chirp-v4`, `chirp-auk`, `chirp-bluejay`,
`chirp-crow`, and `chirp-fenix`. `gen_stem` is the intentional exception and
uses `chirp-v3-0`.

**LIVE-READ:** current billing data remains account-specific and reports
`can_use`, defaults, capabilities, features, max lengths, and
`allowed_condition_combinations`.

**Impact:** a non-v5.5 account can be rejected locally even when another
account model supports Extend or playlist conditioning. The fix should resolve
`auto`/configured model and validate the requested task or condition against
the account response, not merely `can_use`.

**RESOLVED:** Extend and Inspire now use the configured model (defaulting to
account `auto`). Auto selection skips models incompatible with the request's
task and exact active-condition set; an explicitly selected incompatible model
fails before generation submission.

### P1 — Uploaded playlist cover body does not match current Web

**BUNDLE:** `3u1ycshr_o1-2.js` around byte 17,549 performs
`PATCH /api/playlist/v2/{playlist_id}` with:

```json
{
  "metadata": {
    "name": "<optional>",
    "cover_image_s3_id": "<optional>",
    "is_public": "<optional>"
  },
  "bio": {"description": "<optional>"}
}
```

The separate Web-generated-art flow at byte 16,810 is
`POST /api/playlist/v2/{playlist_id}/cover-image`; it is not the uploaded image
flow.

**REPO:** `SetPlaylistCoverRequest::from_upload_id` creates and sends
`cover_url`, `cover_image_s3_id`, and `cover_is_user_set`. The URL is fabricated
as `https://cdn2.suno.ai/image_<upload_id>.jpeg`.

**Impact:** the extra fields rely on backend tolerance and hard-code a CDN host
the server is now responsible for selecting. Send only the uploaded S3 ID in
the current v2 metadata body.

**RESOLVED:** the v2 uploaded-cover patch serializes only
`metadata.cover_image_s3_id`, with exact request-shape coverage.

### P2 — Upload extension and size constraints are not enforced before presign

**BUNDLE:** current Web upload accepts these extensions:

```text
mp3, m4a, wav, flac, ogg, aac
```

and configures a maximum file size of `524288000` bytes.

**REPO:** `audio_extension` accepts every non-empty extension, and the workflow
reads `metadata.len()` only to stream the part; it does not reject an oversized
file before `POST /api/uploads/audio/`.

**Impact:** avoidable presign/API failures occur late and may leave incomplete
upload records. Validate before the first remote write.

**RESOLVED:** the workflow rejects unsupported extensions and files larger
than 524,288,000 bytes after local stat but before upload creation. The size
regression uses a sparse local fixture and performs no remote write.

### P2 — Playlist list serialization can contain duplicate keys

**LIVE-READ:** `GET /api/playlist/me?page=1` succeeded. Current flat playlist
items include top-level `cover_is_user_set` and other presentation fields.

**REPO:** `RawPlaylistInfo` does not declare top-level `cover_is_user_set`, so
Serde captures it in `extra`. `PlaylistInfo` separately serializes normalized
`cover_is_user_set`. The live `sunox playlist list --json` output contained two
`cover_is_user_set` keys for the same item (`null` and `false`).

**Impact:** duplicate JSON keys are parser-dependent and violate the CLI's
machine-readable contract. Consume known flat cover fields before flattening,
or explicitly remove normalized keys from `extra`.

**RESOLVED:** the flat response parser consumes all known top-level cover
fields before flattening. A live readback produced one normalized
`cover_is_user_set` key.

### P2 — Account task compatibility is parsed only as opaque extra data

**LIVE-READ:** generation models expose both `capabilities` and
`allowed_condition_combinations`. Values differ by model and account. Newer
models may expose broad `capabilities:["all"]`, while older models enumerate
`upload_extend`, `playlist_condition`, `cover`, and other task names.

**REPO:** `Model` declares `capabilities`, but
`allowed_condition_combinations` is only retained in the flattened `extra` map.
Generation preparation validates `can_use`, selected feature flags, and length
limits; it does not validate the task/condition combination.

**Impact:** the CLI can pass its local checks and submit a combination the
account model does not offer in Web. This matters increasingly as the task
universe expands.

**RESOLVED:** `allowed_condition_combinations` is typed and generation
preparation validates Cover, Extend, uploaded-audio Extend, and Inspiration
against both task capabilities and exact condition combinations.

## Suspected issues requiring a safe capture or explicit mutation approval

### P1 risk — `create/describe --persona` does not match advanced Create references

**REPO:** `create/describe --persona` sets `persona_id` and adds `prompt` plus
`tags` to `override_fields`, but does not resolve a task, source clip, or source
time range (`src/commands/create/submit.rs:166-174,223-230`).

**BUNDLE:** the current advanced Create builder writes `artist_clip_id` (when a
source clip exists), `persona_id`, `artist_start_s`, and `artist_end_s` for a
Persona reference (chunk `2vyct5q4gq553.js`, byte 1,872,023). Its task resolver
chooses `vox` or `artist_consistency` according to the reference/version path
(bytes 1,878,580 and 1,878,612).

The bundle also contains an `agenticPersonaId` bare-Persona path. Therefore the
evidence does **not** establish that Sunox's shorter request is rejected, but it
does establish that the CLI's ordinary Persona contract is not equivalent to
the current advanced Create contract and cannot locally validate Persona type,
source, or model/task compatibility.

**Required confirmation:** capture one user-authorized Persona generation and
connect the selected Persona type, resolved task, submit payload, result, and
credit delta. Until then, either resolve Persona detail/source before building
`vox` or `artist_consistency`, or explicitly document the current option as an
agentic bare-Persona contract rather than advanced Create parity.

### Cowrite submit may have moved or become async

The current create chunk graph contains and live-validates
`GET /api/generate/cowrite-lyrics/models/`, including
`id`, `display_name`, `family`, and `supports_thinking` (bundle byte 610,997).
The UI state still defaults `lyricsModel` to literal `default`.

However, the downloaded chunks did not contain an actual
`POST /api/generate/cowrite-lyrics/` call or its older request fields
(`selected`, `context_before`, `context_after`, `num_variants`). The current
bundle prominently uses async lyric jobs for adjacent features:
`POST /api/generate/lyrics-mashup`, then
`GET /api/generate/lyrics/{lyrics_id}`.

This is **not proof that the Cowrite POST was removed**: an authenticated lazy
chunk may not have been downloaded, and the model endpoint still exists. A
full DevTools network capture of a user-authorized Cowrite submission is needed
before changing the command.

### Persona mutation route family lacks current live evidence

The current loaded create graph retains list/detail/create/toggle-love strings,
but not Sunox's edit, visibility, paginated-clips, or per-persona trash strings.
Older live evidence in `API_INTELLIGENCE.md` observed
`PUT /api/persona/bulk-trash-personas/`, while Sunox uses per-persona
`PUT /api/persona/trash-persona/{id}/`.

List pagination is response-compatible but intermittently transport-flaky;
mutation route parity still needs a DevTools capture. No persona was changed
for this audit.

### Read transport resets are intermittent and recoverable

Authenticated Persona and playlist GETs intermittently reset. In separate
probes, both directions occurred: a request could reset over negotiated HTTP/2
and succeed over HTTP/1.1, then later reset over HTTP/1.1 and succeed over
HTTP/2. Because the probes occurred at different times, this proves only that
both transports can intermittently reset and a retry can recover; it does not
attribute recovery causally to the protocol switch or show that a route family
permanently requires one protocol.

The final-binary account readback made the remaining availability limit
visible: two Persona commands exhausted all three attempts, while a later
followed-list request and then the same non-empty mine-list request succeeded.
An independent authenticated HTTP/1.1 GET with the same browser-facing headers
also returned 200 during the failing interval. This supports an intermittent
upstream/transport diagnosis, not a persistent route or schema break. The
bounded retry improves recovery but does not claim to make upstream
availability deterministic.

Sunox now retries only explicitly idempotent GETs with a bounded sequence:
normal negotiation, HTTP/1.1 fallback after a transport error, then one final
normal attempt if the fallback also fails. HTTP status errors are not retried
by this mechanism. Persona, playlist, billing/model, and Cowrite-model reads use
it. Mutation calls use the normal negotiated client; their endpoints, methods,
and payloads are unchanged. A local server regression test drops the first
completed GET without a response and verifies that the same GET succeeds on the
fallback request. Additional fault injection truncates a success JSON body and
drops the first two requests, verifying respectively that body reads remain
inside the retry boundary and that the final normal attempt is exercised. A
separate runtime guard test proves that a POST is rejected before network I/O if
a future caller accidentally passes it to the read helper.

### Download behavior now has multiple Web paths

Current clip responses expose `media_urls` (including a direct M4A entry), and
Web has feature-gated presigned download helpers. Sunox's prepared
`GET /api/download/clip/{id}?format=mp3|m4a` path still had same-day read-only
success, so it is not broken. It is nonetheless no longer the only current Web
path, and direct `media_urls` could be a lower-latency fallback when allowed by
the clip response.

OPUS conversion is a backend compatibility feature but is not visible in the
current create graph. Do not remove it without a direct read-only endpoint
probe on an owned completed clip.

### Inspiration's mandatory tag-upsample step needs a submit capture

The generation builder itself does not mandate `/api/prompts/upsample`; it
accepts the tags already present in create state. Sunox always performs tag
upsampling for `clip inspire` and requires the model's `tag_upsample` feature.
Prior HAR evidence supported that sequence, but a fresh Web submit capture is
needed to determine whether it is still unconditional or only a UI option.

## New Web capability surface, not regressions in existing commands

The current task resolver includes more than Sunox exposes:

- `fixed_infill`;
- `stem_condition`, `stem_condition_infill`, `cover_stem_condition`;
- `vox`, `vox_cover`, `vox_extend`, `vox_playlist_condition`;
- `underpainting`, `overpainting`;
- `sample_condition`, `chop_sample_condition`, `mashup_condition`;
- `stacked`;
- additional `audio_refs`, uploaded image/video IDs, and agentic persona fields.

These are capability gaps, not evidence that the implemented
create/cover/extend/stems/inspiration routes stopped working. They should remain
reported as unsupported until their validation, credit model, and response
workflow are captured end to end.

Other first-party routes present in the current graph but not implemented by
Sunox include lyrics infill/mashup jobs, bulk ZIP download, playlist generated
cover art, clip permissions, project collaboration, and video upload/generation.

## Read-only response findings

### Billing and models

`GET /api/billing/info/` decoded successfully. Relevant current fields include:

- `can_use`, `is_default_model`, and `is_default_free_model`;
- `capabilities`, `features`, and `allowed_condition_combinations`;
- `max_lengths` for title, prompt, tags, negative tags, and one-box prompt;
- richer model presentation metadata retained safely by `extra`;
- remaster models with account-specific `can_use`.

No credit balance, account ID, model UUID, or other account-specific value is
recorded in this document.

### Feed and clips

`POST /api/feed/v3` with a two-item limit returned `clips`, a string
`next_cursor`, and `has_more`. Rich current clip fields such as `action_config`,
`media_urls`, ownership, model badges, and secondary metadata remain safely
preserved through flattened extras.

`clip info` successfully composed the direct clip, attribution, comments,
remix count, and similar-clips reads. The auxiliary routes therefore remain
backend-compatible even where their strings were absent from the create-only
chunk set.

### Playlist and persona pagination

- `GET /api/playlist/me?page=1` returned a page-number envelope with
  `num_total_results`, `current_page`, and flat playlist items.
- `GET /api/playlist/v2/{id}` returned the v2
  `metadata`/`relationship`/`stats` envelope and decoded successfully.
- Persona mine/loved/followed list GETs returned their page envelopes. Mine
  included `personas`, `total_results`, `current_page`, nullable
  `continuation_token`, and current quota counters.
- One owned Persona detail decoded identity, visibility, and source-range
  fields; its paginated clips route decoded a valid empty page. This confirms
  the empty envelope, not the schema of a non-empty clip item.

The observed playlist responses and Persona list/detail/empty-page envelopes
are compatible under the bounded GET fallback. Playlist flat-item
normalization was also corrected.

## Not safely verified

The following require a write, a background job, a credit-bearing action, or a
destructive action, so this audit did not probe them live:

- any `/api/generate/v2-web/` variant, including create, cover, extend,
  inspiration, and stems;
- Cowrite POST, tag upsample, remaster, concat, speed, reverse, crop, fade, and
  timed-lyrics initiation;
- audio/image upload and finalization;
- clip/persona/playlist metadata, visibility, reactions, membership, trash,
  restore, or purge;
- WAV/OPUS conversion initiation;
- challenge solving (the request schema was read from first-party code only).

A successful HTTP status from any of these would not by itself prove the full
workflow; a future authorized capture should connect the submit body, returned
IDs, polling route, final persisted state, credit delta, and rollback or cleanup.

## Follow-up order

1. Capture a Persona generation before choosing between advanced-reference
   resolution (`vox`/`artist_consistency`) and an explicitly agentic bare path.
2. Capture a Cowrite submit and persona mutation in DevTools before changing
   those protocols.
3. Re-capture Inspiration submit to decide whether tag upsampling remains
   mandatory.

## Reproduction notes

Repository inventory used `rg` over Rust sources and no JavaScript script.
Bundle analysis used literal searches, byte offsets, `dd`, `perl`, and SHA-256
hashing. Live checks used the compiled Sunox CLI with read-only commands. The
intentional Cowrite probe supplied an invalid model name: the client completed
model discovery, rejected the name locally, and never sent the generation
request.

# Sunox / Suno Web Deep Protocol Audit — 2026-08-23～24

## Executive summary

The current Suno Web create client still uses `POST /api/generate/v2-web/`,
`POST /api/c/check`, the v3 feed, the v2 playlist routes, and the existing
audio-upload/edit routes used by Sunox. The August 23 changes for
`metadata.web_client_pathname: "/create"`, `task: "upload_extend"`, Cowrite
model discovery, and `audio_weight` match the current bundle.

The first deep pass found five actionable gaps. All five were corrected in the
working tree covered by this report:

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

The August 24 follow-up then closed the protocol questions that were still
marked suspected:

- **Persona Advanced is implemented against the current picker contract.** The
  normal Persona-detail picker uses only a valid `root_clip_id` as
  `artist_clip_id`; rootless Vox does not substitute nested `clip.id`,
  `vocal_clip_id`, or `persona_clips`. Root-backed references use the full
  `0..rootClip.metadata.duration` range, rootless Vox uses `0/null`, and a
  sourced Vox reference falls back to `artist_consistency` on an older model
  while rootless Vox fails closed unless the model supports Vox. An authorized
  rootless-Vox generation using the repaired contract subsequently completed
  end to end on v5.5 (`chirp-fenix`).
- **Cowrite submit is current, not merely a July compatibility assumption.** A
  current first-party interaction chunk contains the POST body and response
  fields. One authorized minimal submit also succeeded on the configured
  account; the observed before/after credits delta was `0`.
- **Persona mutation routes are present in current route-specific source.**
  Create, edit, visibility, love, bulk and per-item trash/restore/purge are all
  accounted for. Edit retains the existing `image_s3_id`. This is source
  confirmation, not a claim that a Persona was mutated live.
- **Inspiration enhancement is optional.** Current Web invokes tag upsampling
  only from an explicit Enhance action and reads personalization settings;
  Sunox's opt-in `enhance_tags` flow mirrors that boundary.
- **WAV and OPUS are both GET-first.** Web first checks the file route, starts
  conversion only when the URL is absent, then polls. The OPUS helper is still
  referenced by Studio and is not dead compatibility code.

Authenticated readback on August 24 decoded both a root-backed legacy Persona
and a rootless Vox Persona with non-empty nested clip structures. After the
authorized Advanced generation, the paginated Persona-clips route also
succeeded: page 1 reported `total_results=12` and 12 nested Persona clips; both
new results were complete, private, `task:"vox"`, and linked to the selected
Persona. Earlier transport resets therefore remain availability evidence, but
the later bounded retry/read closes the route and non-empty schema live.

The configured account is an active Pro plan whose top-level
`accessible_features` and plan features both contain `remaster`. Its available
Remaster models nevertheless report `can_use:false`. Current Web source gates
the menu on the plan feature, passes the full `remaster_model_types` array to
the selector, and never reads or filters the models by `can_use`. A live read of
one owned, private, complete source clip also returned Remaster
`action_config={visible:true,disabled:false}`. Sunox's former `can_use:false`
pre-block was therefore a false negative and has been removed. No Remaster POST
was sent and no Remaster result or credit cost is claimed.

The authorized writes in this follow-up were the minimal Cowrite submit and one
private rootless-Vox Advanced generation. Generation returned two complete
private clips with audio URLs; immediate credits moved from `2107` to `2097`,
an exact delta of `10`. No upload, Persona/clip/playlist metadata mutation,
visibility/publication change, reaction, deletion, or WAV/OPUS conversion was
called. No account, Persona, or clip identifier is recorded in this report.
Intermittent resets remain a bounded availability concern for idempotent reads,
not evidence of a route or schema change.

## Scope and evidence levels

The repository baseline is commit
`c4383fabf9c0fde0bb902e2f1a8dad7f5b94a326` (`Sync current Suno web protocols`).

Evidence is labelled as follows:

- **BUNDLE** — first-party code downloaded from `https://suno.com/create` and
  route-specific Suno pages on 2026-08-23～24. Byte offsets below refer to the
  raw immutable chunk.
- **LIVE-READ** — authenticated request that does not create, update, delete,
  upload, react, or consume credits.
- **LIVE-SUBMIT** — the one authorized minimal Cowrite POST, with response
  decoding and credits read before and after. Its observed delta is evidence
  for that submission only, not a universal billing guarantee.
- **LIVE-GENERATION** — one authorized private rootless-Vox Advanced generation,
  including immediate credit readback, completed result fields, and a
  subsequent persisted Persona-clips read. It proves this selected contract and
  account/model combination, not every Persona or model path.
- **REPO** — current Rust request/response implementation or tests.
- **INFERENCE** — a conclusion that cannot be safely live-tested without a
  mutation or credit risk.

Relevant immutable bundle fingerprints:

| Chunk | SHA-256 | Main evidence |
|---|---|---|
| `2vyct5q4gq553.js` | `7f54ece7f0888ff20c038a97931b78ba91e8aa8f3ae16e229159f678b2ac26ab` | generation builder, Persona picker/reference, tag upsample, edit/aligned-lyrics flows, and Remaster menu/source gates |
| `3glkvxw_k-y2j.js` | `581825334acf60efb854dde6f94e01375c93afabb7a49843f60515d82e50ceba` | current Cowrite submit handler |
| `0psavgrakzykm.js` | `e2e001380b49d4620078290007dac6468e27f94b496fc5820455f9ea5e0b6b41` | MP3/M4A and GET-first WAV/OPUS helpers |
| `3u1ycshr_o1-2.js` | `5ed94f918b62041bc63785bf83847f8decdfb478760223e710afddb54c623265` | v2 playlist mutations |
| `0f_f8o2o_ba96.js` | `3a64a1de0dc5c543f4a26cafd5adba8f25c367596b8517430837d3e2e8c0fe3f` | audio upload workflow |
| `1dj8w_ebqiles.js` | `0e399b9b5ff5045324c7c4a68ca4508f04c857aa5449e17d494d7eb193750488` | challenge preflight plus billing feature/remaster-model extraction |
| `3teie_t7wfp1a.js` | `54681826d888baef51cf581e686bf69641140a5f3f405e83943b66552d5b8f38` | `PlanFeature.Remaster` and feature-list helper |
| `2meib9yq1qhch.js` | `44d6998bee09ac9350a5f1ce19545db2c2f2557ef07cccba40217f662d2edec7` | Remaster modal model selector; no `can_use` read/filter |
| `1vw1tt48qlmdn.js` | `2c7dc982024a406bec64a5e4ff167b13e32b53bfccae3782fdefe28d10acd0d0` | clip mutation, Remaster payload/POST, concat and polling store |
| `2o55p_0ruo1em.js` | `abe3ea475ec66dfd8735d49d1a08c4c3fa9fcbab1299655323d4d9e9d5fd909e` | `GEN_ENDPOINT` and `EMPTY_UUID` constants |
| `24l-fzxvb5otv.js` | `3d3a5a3a647b8cd60eb2f0ac0d4e536deec18f0fc9c3f2c5c892c19e3a4e21cf` | Persona bulk trash/restore/purge |
| `3spz_3hruuf8x.js` | `fc8c28e1ea045b3380710a714bf6e1b8835479b273fd41c3683c49e89904e4dd` | Persona per-item trash/restore/purge |
| `2x-65bipljzoj.js` | `476de78ecc690115872d049af7e1030b1f6e52f38e380982dfc5dad0dee3fcd9` | Persona edit route |
| `3iglu18q2jo7v.js` | `012286bf7496160dc099ac988d1ffe8d5fce44881714b03e18481d870c78e063` | full Persona edit body |
| `24xq6d54vzip9.js` | `e5c54cdc72714aa9e5eeed5fade6c47929671df1b23c4939b76c3f5c7472c6fb` | `/voice` visibility and lightweight edit |

The initial create-page files were captured between 20:08 and 20:13
Asia/Shanghai on August 23; the interaction and route-specific chunks were
resolved in the August 24 follow-up. The official source surface is
[Suno Create](https://suno.com/create); chunk names and hashes are included
because immutable asset URLs can later disappear.

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
| Generation | `POST /api/generate/v2-web/` | **BUNDLE stable + LIVE-GENERATION** for one private rootless-Vox Advanced request; other variants not live-tested |
| Personalization | `GET /api/personalization/settings` | **BUNDLE current**; read by the optional Enhance flow; settings mutation was not exercised |
| Tag enhance | `POST /api/prompts/upsample` | **BUNDLE current and optional**; explicit Enhance only, POST not live-tested |
| Cowrite | `GET /api/generate/cowrite-lyrics/models/` | **LIVE-READ stable**; current model family and thinking fields decoded |
| Cowrite | `POST /api/generate/cowrite-lyrics/` | **BUNDLE + LIVE-SUBMIT current**; minimal submit decoded, observed credits delta `0` |
| Concat | `POST /api/generate/concat/v2/` | **BUNDLE stable**, mutation not live-tested |
| Remaster | `POST /api/generate/upsample` | **BUNDLE + LIVE-READ eligibility current**; Pro feature and source `action_config` are enabled, and CLI now matches Web by ignoring legacy model `can_use`; mutation not live-tested |
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
| Download WAV | `GET wav_file`, optional `POST convert_wav`, then poll GET | **BUNDLE current GET-first**; conversion not live-tested here |
| Download OPUS | `GET opus_file`, optional `POST convert_opus`, then poll GET | **BUNDLE current GET-first**; helper is referenced by Studio, conversion not live-tested here |
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
| Persona detail | `GET /api/persona/get-persona/{id}/` | **LIVE-READ current** for root-backed legacy and rootless Vox Personas; non-empty nested clips decoded |
| Persona clips | `GET /api/persona/get-persona-paginated/{id}/?page=N` | **LIVE-READ current** after bounded retry; non-empty page and two newly generated private Vox clips decoded |
| Persona create/edit/visibility/trash | `POST`, `PUT` persona routes | **BUNDLE current**, including route-specific chunks and `image_s3_id` preservation; no live Persona mutation |
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

## Current protocol confirmations and bounded live gaps

### Persona Advanced references are implemented against the current picker

**BUNDLE:** the current Advanced builder still resolves a Persona reference to
`task:"vox"` or `task:"artist_consistency"`, then writes `persona_id`, optional
`artist_clip_id`, and the artist range. The important boundary is the picker
projection, not an arbitrary fallback across every clip-shaped field:

- the normal Persona-detail picker assigns both create-state `rootClipId` and
  `clipId` from `detail.root_clip_id` only (chunk `2vyct5q4gq553.js`, bytes
  1,168,010 and 1,168,044);
- the Advanced reference then reads `clipId || rootClipId` (bytes 504,282,
  504,784 and 1,234,312), and only a truthy reference clip becomes
  `artist_clip_id`;
- `persona_clips[0].clip.id || vocal_clip_id` is retained only as an agentic
  fallback; the Advanced builder does not consume it;
- a separate `applyPersonaFromData` helper uses `root_clip_id || clip.id`, and a
  clip-origin entry can use the current clip ID. These are entry-specific paths,
  not the normal Persona-detail picker rule.

`EMPTY_UUID` is currently
`00000000-0000-0000-0000-000000000000` (`2o55p_0ruo1em.js`, byte 17,219).
The Web validity helper rejects empty and zero IDs, the Persona detail hook
normalizes an invalid root to `null`, and the final generation validator rejects
a zero `artist_clip_id`. Therefore rootless Vox sends no artist source; it does
not promote a nested clip or `vocal_clip_id` into `artist_clip_id`.

The normal root-backed picker range is `artist_start_s=0` and
`artist_end_s=rootClip.metadata.duration`. Rootless Vox uses `0/null`; the
normal picker does not derive this range from `vocal_start_s/vocal_end_s`.

Model selection is also part of the protocol. The current
`modelValidForVoxPersona` accepts model keys containing
`crow/custom/dodo/eagle/fenix/goose/hawk/ibis`. A sourced Vox reference uses
`task:"vox"` on such a model but clears the Vox version and falls back to
`artist_consistency` on an older model. A rootless Vox cannot make that fallback
because it has no source, and Web blocks it with `VOICE_REQUIRES_V5`.
Legacy/root-backed Personas use `artist_consistency`.

**LIVE-READ:** August 24 detail reads distinguished the cases in real data. A
legacy Persona returned a valid root equal to its nested clip, a non-zero vocal
range, and a longer root duration. A rootless Vox returned a zero root plus a
non-empty nested `clip.id`, `vocal_clip_id`, and `persona_clips`. Those fields
decoded successfully but, per the source mapping above, are not normal-picker
artist-source fallbacks.

**REPO:** `create/describe --persona` now resolves detail, normalizes zero root,
uses only a valid root source, obtains the full root duration, implements the
sourced-Vox model fallback, and fails closed for rootless Vox on a model without
Vox support. Request-shape tests cover rootless handling, legacy range, fallback,
and failure.

**LIVE-GENERATION:** one owned private rootless Vox whose detail root was the
zero UUID was submitted through that repaired Advanced path. The response used
`task:"vox"` and v5.5 (`chirp-fenix`); both returned clips reached `complete`,
had audio URLs, remained `is_public:false`, and carried the selected
`persona_id`. Credits were read immediately before submission (`2107`) and
after completion (`2097`), an exact delta of `10`. A subsequent paginated
Persona-clips read reported `total_results=12`/12 nested clips and contained
both new complete/private Vox results with the same Persona association. This
connects request construction, backend interpretation, completed media,
privacy, cost, and persistence without publishing or mutating the Persona.

The separate Simple agentic Web path remains distinct: it can send bare
`persona_id` plus optional `persona_voice_ref`/audio references without the
Advanced task resolver. A paid generation would still be required only if the
project wants to prove that its former bare *custom* short request remains a
backend-compatible third shape. It is no longer required to discover or adapt
the current Advanced contract.

### Cowrite submit is current and live-compatible

**BUNDLE:** interaction chunk `3glkvxw_k-y2j.js` byte 8,735 calls
`POST /api/generate/cowrite-lyrics/`. The current body contains `selected`,
`context_before`, `context_after`, `instruction`, title (truncated to 100 code
points), `style`, `mode`, `references`, nullable `num_variants`, `lyricist_id`,
`metadata.{lyrics_model,enable_thinking}`, nullable `create_session_token`, and
nullable `lyrics_project_id`. Its response reads `edited_lyrics`,
`lyrics_request_id`, `lyrics_id`, `variants`, `artist_to_tag_mapping`, and
`next_prompts`.

**LIVE-SUBMIT:** one authorized minimal Cowrite submission succeeded on August
24 and decoded through the current response type. Credits were read before and
after; the observed delta was `0`. This demonstrates current-account backend
compatibility and the observed cost of that one submission, not a promise that
every Cowrite mode or account is always free.

The earlier conclusion arose because the submit handler lives in an interaction
lazy chunk rather than the initial create graph. Cowrite is not currently
evidenced as having moved to the adjacent lyrics-mashup async job protocol.

### Persona mutation route family is current in first-party source

Current route-specific and interaction chunks confirm both mutation families:

| Operation | Current route | BUNDLE evidence |
|---|---|---|
| Create | `POST /api/persona/create/` | `2vyct5q4gq553.js`, byte 861,679 |
| Edit | `PUT /api/persona/edit-persona/{id}/` | `2x-65bipljzoj.js`, byte 25,733 |
| Visibility | `PUT /api/persona/set_visibility/{id}/?is_public=<bool>` | `24xq6d54vzip9.js`, byte 2,906 |
| Love | `POST /api/persona/{id}/toggle_love/` | `2vyct5q4gq553.js`, byte 1,888,350 |
| Bulk trash/restore/purge | `PUT /api/persona/bulk-trash-personas/` | `24l-fzxvb5otv.js`, byte 2,563 |
| Per-item trash/restore/purge | `PUT /api/persona/trash-persona/{id}/?undo=<bool>&hide=<bool>` | `3spz_3hruuf8x.js`, byte 3,093 |

Both trash families encode ordinary trash as `undo=false,hide=false`, restore
as `undo=true,hide=false`, and permanent hide/purge as
`undo=false,hide=true`. The full and lightweight edit surfaces retain the
existing `image_s3_id`; `/voice` initializes edit state from the current image
and sends it again. Sunox's edit request preservation is therefore intentional.

This section is **BUNDLE**, not LIVE-SUBMIT evidence. No Persona was created,
edited, published, loved, trashed, restored, or purged during this audit. A
real mutation is unnecessary for route discovery and would only add account
permission, response, persistence, and rollback evidence.

### Read transport resets are intermittent and recoverable

Authenticated Persona and playlist GETs intermittently reset. In separate
probes, both directions occurred: a request could reset over negotiated HTTP/2
and succeed over HTTP/1.1, then later reset over HTTP/1.1 and succeed over
HTTP/2. Because the probes occurred at different times, this proves only that
both transports can intermittently reset and a retry can recover; it does not
attribute recovery causally to the protocol switch or show that a route family
permanently requires one protocol.

On August 24, two early attempts to read
`GET /api/persona/get-persona-paginated/{id}/?page=1` failed with “error sending
request” after automatic JWT refresh. An ordinary Persona-detail control then
failed at the same transport layer. After the authorized generation, the
bounded retry/read succeeded on that same paginated route and decoded a
non-empty page, including both new results. The early failures remain evidence
of intermittent transport availability, while the later success rules out a
current route or response-schema break for the observed account data.

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

### Download protocol is current and WAV/OPUS are GET-first

**BUNDLE:** `0psavgrakzykm.js` confirms the prepared MP3/M4A routes and the full
WAV/OPUS state machines:

```text
GET  /api/download/clip/{id}?format=mp3|m4a

GET  /api/gen/{id}/wav_file/
POST /api/gen/{id}/convert_wav/

GET  /api/gen/{id}/opus_file/
POST /api/gen/{id}/convert_opus
```

MP3/M4A occur at bytes 18,194/18,778, WAV read/convert at 24,065/23,007,
and OPUS read/convert at 23,119/23,274. Both lossless/codec helpers first GET
the existing file. They return immediately when a URL is present; only a
missing file starts conversion, followed by at most 24 GET polls five seconds
apart. The OPUS helper is referenced from current Studio initialization, so it
is not an unreferenced historical route.

**REPO:** Sunox follows the same GET-first ordering for WAV and OPUS. No
conversion POST was exercised in this audit. Current `media_urls`, direct audio
fallbacks and presigned helpers are parallel optimization/compatibility paths;
they do not invalidate the prepared-download or file-status contracts.

### Inspiration tag enhancement is explicit and optional

**BUNDLE:** the generation builder accepts the tags already in create state; an
Inspiration reference does not itself call `/api/prompts/upsample`. Current Web
offers a separate Enhance action, gated by the selected model's `tag_upsample`
feature. That flow reads `GET /api/personalization/settings`, treats missing
`styles_augmentation` as enabled, and only then submits the upsample request.

**REPO:** Inspiration exposes `enhance_tags` as opt-in. With it disabled, Sunox
preserves the supplied tags and does not fetch personalization settings or call
upsample. With it enabled, Sunox validates model support, reads personalization
settings, sends original tags plus optional lyrics/user guidance, adopts the
upsampled value (`is_instrumental:false` for the current vocal Inspiration
flow), records `metadata.last_tags_generation` including
`personalization_enabled`, and validates the final tag length against the
resolved model. Request-shape tests cover both branches.

This closes the earlier “mandatory upsample” concern without a generation
capture. Neither tag upsampling nor personalization-settings mutation was
called live in this audit.

## New Web capability surface, not regressions in existing commands

The current task resolver includes more than Sunox exposes:

- `fixed_infill`;
- `stem_condition`, `stem_condition_infill`, `cover_stem_condition`;
- `vox_cover`, `vox_extend`, `vox_playlist_condition` beyond the implemented
  ordinary Advanced Persona `vox` reference;
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
- top-level `accessible_features` and plan features, both including `remaster`
  for the active Pro account;
- remaster models whose legacy `can_use` values were all `false`; current Web
  does not use that field as a gate, so Sunox now treats presence in
  `remaster_model_types` as model availability.

Only the immediate `2107`/`2097` credit readings needed to establish the exact
generation delta are recorded. No account identifier, model UUID, Persona ID,
clip ID, or unrelated account-specific value is retained in this document.

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
- August 24 owned-detail reads decoded one root-backed legacy Persona and one
  rootless Vox Persona. The latter included non-empty nested `clip`,
  `vocal_clip_id`, and `persona_clips` data (including Vox task metadata), so
  the nested detail schema is live-proven.
- A historical August 23 read decoded a valid empty paginated-clips envelope.
  On August 24 the first reads reset at the transport layer, but a later bounded
  retry/read succeeded with `total_results=12` and 12 nested clips. It decoded
  both newly generated results as complete, private, Vox-task clips associated
  with the selected Persona.

The observed playlist responses and Persona list/detail/paginated nested-clip
envelopes are compatible under the bounded GET fallback. Playlist flat-item
normalization was also corrected.

## Not safely verified

Other than the authorized minimal Cowrite submit and private rootless-Vox
Advanced generation described above, the following require a write, a
background job, account eligibility, a credit-bearing action, or a
destructive/public state change, so this audit did not probe them live:

- other `/api/generate/v2-web/` variants, including non-Persona create, cover,
  extend, inspiration, stems, root-backed Persona and older-model fallback;
- tag upsample, concat, speed, reverse, crop, fade, and timed-lyrics initiation;
- Remaster submission: account and source eligibility were confirmed read-only,
  but a generation-backed POST was not authorized merely to prove the fix;
- audio/image upload and finalization;
- clip/persona/playlist metadata, visibility, reactions, membership, trash,
  restore, or purge;
- WAV/OPUS conversion initiation;
- challenge solving (the request schema was read from first-party code only).

A successful HTTP status from any of these would not by itself prove the full
workflow; a future authorized capture should connect the submit body, returned
IDs, polling route, final persisted state, credit delta, and rollback or
cleanup. In particular, current first-party Persona mutation source is not
being represented here as a destructive or public live test.

## Follow-up order

1. If an end-to-end Remaster result is required, explicitly authorize one
   credit-bearing submission on the already confirmed eligible private source;
   capture the returned clip IDs, completion, persistence, and credit delta.
2. If the project wants to preserve a claim that the former bare *custom*
   Persona payload remains backend-compatible, capture one authorized paid
   generation with submit body, result, task interpretation, and credit delta.
   The implemented current Advanced rootless-Vox path is already live-closed;
   this optional check concerns only the former shorter payload.

No further account write is required to confirm Cowrite, the implemented
Advanced rootless-Vox path, current Persona mutation routes, paginated Persona
clips, optional Inspiration enhancement, or GET-first WAV/OPUS.

## Reproduction notes

Repository inventory used `rg` over Rust sources and no JavaScript script.
Bundle analysis used literal searches, byte offsets, `dd`, `perl`, and SHA-256
hashing. Live checks used the compiled Sunox CLI and the configured account.
Most calls were read-only. The two authorized exceptions were the minimal
Cowrite submit (successfully decoded, observed credits delta `0`) and one
private rootless-Vox Advanced generation. The latter used v5.5
(`chirp-fenix`), returned two complete private Vox clips with audio URLs and the
selected Persona association, and consumed exactly 10 credits (`2107` to
`2097`). The later paginated read verified both results persisted. No account,
Persona, or clip identifier is recorded here. No upload, metadata/publication
change, reaction, deletion, Persona mutation, or conversion job was performed.

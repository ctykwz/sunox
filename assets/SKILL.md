---
name: sunox
description: Generate AI music from the terminal using the `sunox` CLI. Use when user asks to "generate a song", "make music", "create AI music", "make a track", "generate audio", or wants to programmatically use Suno web workflows for custom lyrics, tags, voice personas, playlists, covers, remasters, speed/reverse/crop/fade edits, or generation-backed stems. Also use when downloading Suno songs (default MP3 auto-embeds lyrics; explicit m4a/wav/opus supported). Run `sunox agent-info` for the full machine-readable capability dump. NOT for writing song prompts/lyrics without generating audio.
---

# sunox CLI

Generate AI music from your terminal using direct Suno web workflows, including custom lyrics, style tags, voice personas, playlists, covers, remasters, speed/reverse/crop/fade edits, stems extraction, and word-level timed lyrics.

## When to use

- User wants to **generate** AI music programmatically through the sunox CLI
- User wants to **download** Suno songs (default MP3 auto-embeds USLT + SYLT lyrics into ID3 tags; explicit m4a/wav/opus supported)
- User wants to **batch generate**, **script**, or **integrate** Suno into a music workflow
- User mentions Suno, AI music generation, or wants to control Suno parameters from the terminal

## When NOT to use

- Writing song lyrics or Suno-formatted prompts without actually generating audio
- General music theory, composition advice, or non-Suno music tasks

## Setup (first time on a new machine)

```bash
# Auto-extract auth from browser (Chrome / Arc / Brave / Firefox / Edge)
sunox login

# Verify it worked
sunox doctor

# Diagnose DNS/direct-TCP/HTTPS connectivity when auth or API requests cannot connect
sunox doctor --network
sunox doctor --network --strict  # non-zero when a requested network path is degraded
```

If `sunox login` fails on a headless box, fall back to:

```bash
printf '%s' "$SUNOX_COOKIE_INPUT" | sunox auth --cookie-stdin
printf '%s' "$SUNOX_JWT_INPUT" | sunox auth --jwt-stdin
sunox auth --refresh                                      # force-refresh stored Clerk session
```

## Discovery

Always start by reading machine-readable capabilities:

```bash
sunox agent-info        # JSON: commands, models, exit codes, features, env prefix
sunox --help            # full subcommand list
sunox <cmd> --help      # flags for a specific subcommand
```

## Agent integration

This skill is intended for Codex-style coding agents that need a deterministic
tool contract. Install it with:

```bash
sunox install-skill --target codex
```

Use `--json` whenever you need to parse command output. The human-facing
surface is intentionally small (`sunox <prompt>`, `sunox create`, `sunox
download`, `sunox add --to`, `sunox login`, `sunox doctor`). For automation,
use the resource commands exposed by `sunox agent-info`.

Suno write commands are account-scoped serial by default to avoid accidental
same-account concurrent mutations. Use `sunox config set serial_mutations false`
to persistently disable this behavior, `-c serial_mutations=false` for one
invocation, or `--parallel` for one command.
Agents should not pass --parallel or disable `serial_mutations` unless the user
explicitly asks to allow same-account concurrent writes.

Risk control defaults for agents:

- do not run multiple same-account write commands in parallel unless the user
  explicitly asks for parallel writes or has disabled serial mutations.
- do not publish clips, playlists, or personas, or make them public, unless the
  user explicitly asks.
- do not run delete, trash, purge, `empty-trash`, or similar destructive commands
  unless the user explicitly asks. `clip purge` and `clip empty-trash` are
  irreversible. When explicitly requested, pass `-y/--yes` because destructive
  commands require it.
- allow the normal challenge preflight to run automatic browser verification
  when Suno requires it. `challenge_browser=auto` prefers a paired existing
  Suno tab and falls back to an isolated browser. Do not force `--captcha`
  unless the user asks, and prefer an externally supplied `--token` when provided.
- never print or commit cookies, Clerk values, JWTs, challenge tokens, or other
  auth material.

Treat commands that return new or processing clips as asynchronous workflows.
After `sunox create`, `sunox clip inspire`, `sunox clip cover`, `sunox clip extend`, `sunox clip
concat`, `sunox clip stems`, `sunox clip remaster`, `sunox clip speed`, or
`sunox clip reverse` returns a new/processing clip ID, call `sunox clip wait
<clip_id> --json` before download, quality filtering, or playlist decisions
unless the user only asked to submit. `sunox clip crop` and `sunox clip fade`
already wait for the resulting clip to complete, so a successful response from
either command does not require another `clip wait`.

For simple audio analysis, prefer the existing media: read `audio_url` from
`sunox clip info <clip_id> --json` or use the default CDN `sunox clip download`.
`clip info`
also returns song-page context such as attribution, comments,
`direct_children_count`, and `similar_clips` in JSON. If a non-auth,
non-rate-limit supplemental read fails, the base clip still returns and JSON
includes `supplemental_errors`; auth and rate-limit errors still abort
normally. Do not trigger new Suno generation/export work just to inspect audio.
Reserve WAV or generation-backed stems for explicit deep-analysis, lossless,
or stem requests. Studio functionality is outside this CLI's scope.
The current CLI download defaults to the existing `clip.audio_url` CDN MP3 and
supports explicit `--format mp3|m4a|wav|opus` through Suno's official download
endpoints; `--video` uses `clip.video_url` when present.
`sunox clip stems` performs generation-backed stems extraction; it is not the
same as Suno Web Pro Get Stems export.

Download output directories are created automatically. Existing local files are
preserved by default; only pass `--force` when the user explicitly asks to
replace the matching downloaded file.

## Human commands

```bash
# Plain prompt shortcut
sunox "a chill lo-fi track about rainy mornings"

# Full generation controls
sunox create --title "Rainy Morning" --tags "lo-fi, chill" "a track about rainy mornings"

# Download completed songs
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# Add songs to a playlist
sunox add <clip_id_1> <clip_id_2> --to <playlist_id>

# Account and health
sunox login
sunox doctor
sunox doctor --network
```

## Agent and advanced commands

```bash
# Generate with full control (custom mode)
sunox create \
  --title "Weekend Code" \
  --tags "indie rock, guitar, upbeat" \
  --exclude "metal, heavy" \
  --lyrics-file lyrics.txt \
  --vocal male \
  --weirdness 40 \
  --style-influence 65

# Generate from a free-text description (Suno writes the lyrics)
sunox create --title "Rainy Morning" "a chill lo-fi track about rainy mornings"

# Use one existing clip as loose inspiration
sunox clip inspire <clip_id> --title "New Song" --tags "garage pop" --lyrics-file lyrics.txt

# Wait for returned clip IDs, then download completed MP3s
sunox clip wait <clip_id_1> <clip_id_2>
sunox clip download <clip_id_1> <clip_id_2> --output ./songs/

# Upload a local audio file into the Suno library
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload-status <upload_id> --json

# Lyrics only — FREE, uses no credits
sunox lyrics --prompt "song about coffee at sunrise"

# Generate using a voice persona (your own voice)
sunox create \
  --persona e483d2f0-50ca-4a09-8a74-b9e074646377 \
  --title "My Song" --tags "pop, warm" \
  --lyrics "[Verse]\nHello from the CLI"

# Inspect a specific clip
sunox clip info <clip_id>
sunox persona list
sunox persona info <persona_id>
sunox persona clips <persona_id> --page 1
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"  # private by default
sunox persona create <clip_id> --name "Public Voice" --public                    # explicit opt-in only
sunox persona set <persona_id> --name "My Voice" --description "Warm lead vocal" --public false
sunox persona processed-clip <processed_clip_id>
sunox persona publish <persona_id>        # only when the user explicitly asks to make it public
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona toggle-love <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id>
sunox persona purge <persona_id> -y       # only when the user explicitly asks for permanent deletion

# List / search your library
sunox clip list
sunox clip list --cursor <next_cursor>
sunox clip list --trashed
sunox clip list --liked --public --sort popular
sunox clip search "rainy" --limit 50
sunox clip search "rainy" --all

# Manage playlists
sunox playlist list
sunox playlist create --name "Release candidates" --description "Tracks to review" --image-url <cover_url>
sunox playlist set <playlist_id> --image-file ./cover.png
sunox add <clip_id_1> <clip_id_2> --to <playlist_id>
sunox playlist add <playlist_id> <clip_id_1> <clip_id_2>
sunox playlist remove <playlist_id> <clip_id_1>
sunox playlist publish <playlist_id> --private
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
sunox playlist restore <playlist_id>
sunox playlist save <playlist_id>
sunox playlist unsave <playlist_id>
sunox playlist like <playlist_id>
sunox playlist dislike <playlist_id>
sunox playlist delete <playlist_id> -y

# Cover or remaster an existing clip
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip remaster <clip_id> --model v5.5 --variation subtle # subtle, normal, or high

# Adjust playback speed while keeping pitch
sunox clip speed <clip_id> --multiplier 0.94

# Non-generation edit actions
sunox clip reverse <clip_id>
sunox clip crop <clip_id> --start 12.5 --end 74.0
sunox clip crop <clip_id> --start 30.0 --end 45.0 --remove-section
sunox clip fade <clip_id> --in 2.0 --out 78.5

# Extract stems (vocals + instruments)
sunox clip stems <clip_id>

# Word-level timed lyrics (LRC format for synced display)
sunox clip timed-lyrics <clip_id> --lrc > song.lrc

# Download with auto-embedded synced lyrics by default, or request a format
sunox download <clip_id_1> <clip_id_2> --output ./songs/
sunox clip download <clip_id> --format wav --output ./songs/

# Manage clips
sunox clip set <clip_id> --title "New Title" --lyrics-file updated.txt
sunox clip publish <clip_id_1> <clip_id_2>          # only when the user explicitly asks to make public
sunox clip publish <clip_id_1> --private            # make private
sunox clip delete <clip_id> -y               # move to trash
sunox clip restore <clip_id>                  # undo trash
sunox clip purge <clip_id> -y                 # irreversible permanent delete
sunox clip empty-trash -y                     # irreversible: delete every trashed clip
sunox clip like <clip_id>
sunox clip like <clip_id> --clear
sunox clip dislike <clip_id>

# Account
sunox credits
sunox models
sunox config show
sunox config set output_dir ./songs
```

## Generation parameters reference

| Flag | What it does | Range / format |
|---|---|---|
| `--title` | Song title | ≤ 100 chars |
| `--tags` | Style direction | Read `max_lengths.tags` from `sunox models --json` |
| `--exclude` | Styles to avoid | Read `max_lengths.negative_tags` from `sunox models --json` |
| `--lyrics` / `--lyrics-file` | Custom lyrics with `[Verse]` `[Chorus]` tags | Read `max_lengths.prompt` |
| `--prompt` (describe mode) | Free-text description | Read `max_lengths.gpt_description_prompt` |
| `--model` | Model version | account default when omitted; v5.5, v5, v4.5+, v4.5-all, v4.5, v4 |
| `--vocal` | Vocal gender | male, female; custom mode uses Web's `metadata.vocal_gender` |
| `--persona` | Voice persona UUID | from Suno voice creation |
| `--weirdness` | How experimental | 0–100 |
| `--style-influence` | How strictly to follow tags | 0–100 |
| `--enhance-tags` | Call Suno's tag upsample flow before submit | explicit opt-in |
| `--instrumental` | No vocals | flag |
| `--token` | Externally supplied challenge token | only when Suno challenges the request |
| `--captcha` | Force browser-backed challenge verification | runs even when preflight says unnecessary |
| `--no-captcha` | Disable automatic browser verification | challenge preflight still runs |

## Models

| Version | Codename | Notes |
|---|---|---|
| auto | account response | CLI default; resolves the current usable account default |
| v5.5 | chirp-fenix | Latest generation; fallback only when billing info is unavailable |
| v5 | chirp-crow | Previous gen |
| v4.5+ | chirp-bluejay | Extended capabilities |
| v4.5-all | chirp-auk-turbo | Free-tier option when available to the account |
| v4.5 | chirp-auk | Stable |
| v4 | chirp-v4 | Legacy |
| v3.5 | chirp-v3-5 | Legacy |
| v3 | chirp-v3-0 | Legacy |
| v2 | chirp-v2-xxl-alpha | Legacy |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

Model availability, the account default, and length limits are account-specific. The default
`default_model=auto` resolves the account's usable default directly from `/api/billing/info/`.
`sunox models --json` exposes separate `generation` and `remaster` arrays from the same account data. Explicit models are validated
against `can_use` and `max_lengths` when billing info is available; v5.5 is used only when that
read is unavailable.

## Agent-friendly output

- Every command supports `--json`. JSON is **auto-detected** when stdout is piped.
- Progress messages and errors go to **stderr** so they don't pollute JSON pipelines.
- Suno write commands are account-scoped serial by default; do not pass --parallel or disable `serial_mutations` unless the user explicitly allows same-account concurrent writes.
- For simple audio analysis, prefer clip `audio_url` CDN media from `sunox clip info <clip_id> --json` or use the default CDN `sunox clip download`; `clip info` also includes `attribution`, `comments`, `direct_children_count`, `similar_clips`, and non-fatal `supplemental_errors`. Reserve explicit `--format`, generation-backed stems, or Pro video for requests that name that format or need deep/lossless analysis. Studio functionality is outside this CLI's scope.
- Download output directories are created automatically. Do not pass `--force` unless the user explicitly requests replacing an existing local download; ordinary downloads refuse to overwrite a matching file.
- MP3 downloads abort on auth/rate-limit failures while fetching timed lyrics. Other timed-lyrics failures preserve the MP3 with available plain lyrics and add a structured `warnings` entry. Downloads have a two-hour total deadline and 2 GiB size limit; Ctrl-C cleans staging files.
- `--quiet` suppresses download progress and ordinary status output. A batch download that has already written any output and then fails returns `partial_download`; inspect `error.details.succeeded`, `error.details.failed`, and `error.details.not_attempted_clip_ids`, then retry only the required IDs.
- `playlist remove` submits one remove request per clip. If a later clip fails, JSON errors use code `partial_mutation`; inspect `error.details.succeeded_clip_ids`, `error.details.failed`, and `error.details.not_attempted_clip_ids` before retrying.
- Multi-clip `publish`, `unpublish`, `like`, and `dislike` also run serially. If a later clip fails, inspect the same `partial_mutation` progress fields and retry only failed or unattempted IDs.
- Multi-step playlist create/set, local image upload, clip cover update, and audio upload failures use `partial_mutation` with resource IDs, `completed_steps`, `failed.step/code/message`, and `recovery`. Follow `recovery.command` only when `recovery.resumable=true`; never replay a mutation marked false. Use `sunox clip upload-status <upload_id> --json` for a read-only audio upload status check.
- Do not publish, make public, or run destructive commands unless the user explicitly asks for that action; destructive commands require `-y/--yes`.
- Errors include actionable suggestions in the JSON envelope.

```bash
# Pipe-friendly: auto-JSON
sunox clip list | jq '.data.clips[0].title'
sunox clip info <clip_id> --json | jq '.data.audio_url'
```

## Exit codes (semantic)

| Code | Meaning | What the agent should do |
|---|---|---|
| 0 | Success | Continue |
| 1 | Runtime, web endpoint, partial mutation or partial download error | Inspect `error.code` and `error.details` before retrying |
| 2 | Config error | Fix config, do not retry blindly |
| 3 | Auth error | Run `sunox login` |
| 4 | Rate limited | Wait 30–60s, then retry |
| 5 | Not found | Verify the resource ID |

## Common workflows

### Generate and download a finished MP3 with synced lyrics

```bash
sunox create --title "Foo" --tags "ambient, piano" --lyrics-file foo.txt
sunox clip wait <clip_id>
sunox clip download <clip_id> --output ./out/
# → ./out/foo-<clipid8>.mp3 with USLT + SYLT lyrics embedded
```

### Resume / continue a clip

```bash
sunox clip extend <clip_id> --at 60.0 --lyrics "[Verse 2]\n..."
sunox clip wait <new_clip_id>
sunox clip concat <new_clip_id>           # stitch into a full song
```

Use `clip concat` with a source that has original Suno generation history. Do
not assume crop, fade, reverse, or other edited results are valid inputs: Suno
can reject them with `Bad history.`

### Batch download yesterday's songs as JSON, then pull MP3s

```bash
ids=$(sunox clip list --json | jq -r '.data.clips[].id')
sunox clip download $ids --output ./archive/
```

## Notes

- Auth refreshes automatically (~7-day session lifetime).
- On Windows, `sunox login` skips live Chromium cookie databases so App-Bound
  decryption cannot force-close the user's browser. Firefox uses its safe
  read-only SQLite path. The dedicated interactive fallback requires an
  installed Chromium-family browser when no reusable session is found.
- Browser-derived request context is real-value first: `sunox login` links cookies
  to the matching local profile and probes the same installed browser binary for
  runtime user-agent, language, and client hints without a visible window.
  Legacy auth is repaired before the next authenticated command. Fresh values
  win per field, stored values survive a failed probe, and built-in constants
  are only the final fallback for browser UA/language fields. A same-account
  browser `Device-Id` is recovered when possible, otherwise omitted rather
  than replaced with a fabricated identity. Conditional `Referring-*` fields
  are not invented without real navigation/referrer context.
- Generation metadata fills `user_tier` from the current account's
  `/api/billing/info/` `plan.id` when available, and falls back to an empty
  value when that read is unavailable.
- Do not fabricate tag-upsample metadata. In captured web flows,
  `metadata.last_tags_generation` is only present after a real
  `/api/prompts/upsample` response; use `--enhance-tags` when the user asks for
  Suno to enhance tags, otherwise omit it. Its tags/request_id come from the
  response; `personalization_enabled` follows the captured submit shape. The
  submit also marks `override_fields=["tags"]`. Vocal requests pass current
  custom lyrics as upsample context; instrumental requests omit lyrics.
- Commands that submit through `/api/generate/v2-web/` preflight `POST /api/c/check` with `ctype=generation`; if Suno reports a challenge and stored Clerk refresh material exists, Sunox refreshes the JWT once and repeats the preflight. When a challenge remains, `challenge_browser=auto` first uses the paired Browser Bridge in an existing Suno tab and falls back to the matching isolated browser. hCaptcha uses provider 1 and Cloudflare Turnstile uses provider 2 according to `captcha_version`. Install or update the optional bridge with `sunox install-browser-extension --force`; never install or reload a browser extension without the user's authorization. When no challenge is required, submit uses `token=null` and `token_provider=null`.
- Prefer `--token <solved>` when an external token is already available. Use `--captcha` only to force verification even when preflight says it is unnecessary, or `--no-captcha` to disable automatic browser verification.
- Generation paths (normal, describe, voice persona, inspiration, cover, extend, generation-backed stems) use `/api/generate/v2-web/`; create, inspire, cover, extend, and stems expose `--token`, `--captcha`, and `--no-captcha`. Source-dependent commands read clips through the current `GET /api/clip/{id}` route; multi-clip polling uses feed/v3 exact-ID filters. Cover uses `task=cover`, `metadata.create_mode=custom`, and the source title. Inspiration uses one source clip and the live-captured playlist-conditioned request; do not invent uncaptured instrumental or multi-source inputs. Extend sets `metadata.lyrics_updated` only when replacement lyrics were supplied and uses feed/v3 exact-id metadata enrichment only when the current single-clip response lacks source style metadata. It defaults `title`, `tags`, `negative_tags`, and `make_instrumental` from the source when available; use `--title`, `--tags`, `--exclude`, `--instrumental`, or `--no-instrumental` to override. Timed lyrics use the current v3 start/poll contract; v2 is compatibility fallback only. Remaster and speed use their current web edit/generation routes. `sunox clip list` supports query-only filters such as `--liked`, `--public`, `--upload`, `--cover`, `--extend`, and `--sort popular`; this is not a library sync workflow. The production-live similar-song and lyrics-only service endpoints remain supplemental because the current Web bundle has no behaviorally equivalent replacement. `sunox clip stems` is not the same as Suno Web Pro Get Stems export. You usually only need the subcommands.
- Persona list/detail/clips/create/set/processed-clip/publish/unpublish/love/unlove/toggle-love/delete/restore/purge are available through `sunox persona ...`.
- Playlist create/list/detail/metadata/add/remove/publish/reorder/save/unsave/like/dislike/restore/delete are available through `sunox playlist ...`; use `playlist set <id> --image-file <path>` for local cover uploads.
- Clip delete/restore/purge and like/dislike are available through `sunox clip delete`, `sunox clip restore`, `sunox clip purge`, `sunox clip like`, and `sunox clip dislike`. `sunox clip empty-trash -y` permanently deletes every trashed clip. Purge and empty-trash are irreversible and require an explicit user request. `--clear` removes the selected reaction.
- `sunox clip upload <file>` uploads local audio through Suno's presigned S3 flow, waits for processing, initializes a clip, and can set title/lyrics metadata. Uploaded local clip covers are applied by `image_s3_id`; arbitrary external URLs use `image_url`. `sunox clip upload-status <upload_id>` only reads an existing upload's processing status.
- `sunox config set <key> <value>` persists local defaults; `SUNOX_*` environment variables override persisted config.
- When the CLI returns `schema_drift` (Suno changed its web schema), run `sunox update` to pull the latest binary from GitHub Releases.
- When unsure about flags, run `sunox <command> --help` or `sunox agent-info`.

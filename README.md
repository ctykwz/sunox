<div align="center">

# sunox

**Generate AI music from your terminal — direct Suno web workflow support**

<br />

[![GitHub](https://img.shields.io/badge/GitHub-ctykwz%2Fsunox-181717?style=for-the-badge&logo=github)](https://github.com/ctykwz/sunox)

<br />

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
&nbsp;
[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
&nbsp;
[![crates.io](https://img.shields.io/crates/v/sunox?style=for-the-badge)](https://crates.io/crates/sunox)
&nbsp;
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=for-the-badge)](https://github.com/ctykwz/sunox/pulls)

---

A single Rust binary that talks directly to Suno's web endpoints. Generate songs with custom lyrics, style tags, your own voice persona, vocal control, weirdness/style sliders, covers, remasters, speed/reverse/crop/fade edits, and stems. Zero-friction auth — one command extracts credentials from your browser automatically.

**Languages:** English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

[Install](#install) | [Quick Start](#quick-start) | [Human Commands](#human-commands) | [Agent & Advanced Commands](#agent--advanced-commands) | [Features](#features) | [Contributing](#contributing)

</div>

## Why

Suno's web UI works, but it is not built for scripting, piping lyrics from a file, batch generation, or integration into a terminal-based music workflow.

This CLI fixes that. Auto-auth from your browser, core generation parameters exposed as flags, dual JSON/table output for both humans and AI agents. Downloads auto-embed synced lyrics into MP3 files.

Sunox is an unofficial project and is not affiliated with or endorsed by Suno. It uses private Web APIs that can change without notice. You are responsible for complying with Suno's terms, account limits, and the rights applicable to generated or uploaded material.

## Install

### Cargo (any platform)

```bash
cargo install sunox
```

Requires Rust 1.88 or newer.

### Pre-built binaries

Download from [GitHub Releases](https://github.com/ctykwz/sunox/releases) — binaries for macOS (Apple Silicon + Intel), glibc 2.28+ Linux (x86_64 + ARM), and 64-bit Windows.
Releases include unsigned platform archives for macOS (Apple Silicon + Intel), Linux, and Windows, plus a static CRT Windows build, CycloneDX SBOM, provenance attestation, and `SHA256SUMS`; `sunox update` verifies the selected archive before installing it. macOS and Windows may show their platform's unsigned-download warnings.

### Self-update

Already have `sunox` installed? Pull the latest binary from GitHub Releases without touching your package manager:

```bash
sunox update --check    # see what's available
sunox update            # install the latest release
```

> Tip: when Suno changes its web schema mid-cycle, run `sunox update` first — it's faster than reinstalling with `cargo install sunox`.

## Quick Start

```bash
# 1. Authenticate (auto-extracts from Chrome/Arc/Brave/Firefox/Edge)
sunox login

# 2. Generate from a plain prompt
sunox "a chill lo-fi track about rainy mornings"

# 3. Generate with full control
sunox create \
  --title "Weekend Code" \
  --tags "indie rock, guitar, upbeat" \
  --exclude "metal, heavy" \
  --lyrics-file lyrics.txt \
  --vocal male \
  --weirdness 40 \
  --style-influence 65

# 4. Wait for the returned clip IDs, then download completed audio
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# 5. Add a result to a playlist
sunox add <clip_id> --to <playlist_id>
```

For agents and scripts, start with `sunox agent-info --json`, then call the
resource commands with `--json`.

## Global options

Available on every subcommand:

| Flag | What it does |
|---|---|
| `--json` | Force structured JSON output (auto-detected when stdout is piped) |
| `--quiet` | Suppress non-essential progress output |
| `--parallel` | Allow concurrent Suno write requests for the same account; default writes are account-scoped serial |
| `-c key=value` / `--config key=value` | Override a config value for this invocation, e.g. `-c default_model=v5.5 -c output_dir=./songs` (repeatable) |
| `-V` / `--version` | Print the CLI version |
| `-h` / `--help` | Subcommand-aware help |

Suno write commands are account-scoped serial by default. Disable that behavior
persistently with `sunox config set serial_mutations false`, for one invocation
with `-c serial_mutations=false`, or for one command with `--parallel`.
Environment overrides use the `SUNOX_*` prefix, for example `SUNOX_DEFAULT_MODEL`,
`SUNOX_OUTPUT_DIR`, and `SUNOX_BROWSER_PATH`.

## Human Commands

These are the commands most people should need day to day:

```
sunox <prompt>                  Generate from a plain description
sunox create [prompt]           Generate with title, tags, lyrics, model, persona
sunox download <clip_ids>       Download completed songs
sunox add <clip_ids> --to <id>  Add songs to a playlist
sunox login                     Set up authentication from browser
sunox logout                    Remove stored auth and interactive login profile
sunox doctor                    Diagnose config and auth
sunox doctor --network          Diagnose DNS, direct TCP, and HTTPS connectivity
sunox doctor --network --strict Return non-zero when a network path is degraded
```

## Agent & Advanced Commands

`sunox` keeps lower-level Suno workflows available for Codex-style agents,
automation, and debugging. Agents should prefer `--json` and discover the exact
contract with `sunox agent-info --json`.

### Create

```
sunox create              Description mode or custom lyrics mode
sunox lyrics              Generate lyrics only (free, no credits)
sunox clip extend         Continue a clip from a timestamp
sunox clip concat         Stitch clips into a full song
sunox clip cover          Create a cover with different style/model
sunox clip inspire        Generate a new song from one clip's loose inspiration
sunox clip remaster       Remaster with a different model version
sunox clip speed          Adjust playback speed
sunox clip reverse        Reverse audio
sunox clip crop           Trim to a section or remove a section
sunox clip fade           Add fade in/out
sunox clip stems          Generate stems from an existing clip
```

### Browse & Inspect

```
sunox clip list            List your songs
sunox clip list --cursor <next_cursor>
sunox clip list --trashed  List songs currently in trash
sunox clip list --liked --public --sort popular
sunox clip search <query>  Search songs by title or tags (`--cursor`, `--limit`, `--all`)
sunox clip info <id>       Detailed view plus song-page context
sunox persona list    List your voice personas
sunox persona info <id> View a voice persona
sunox persona clips <id> List songs attached to a voice persona
sunox persona create <clip_id> Create a voice persona from a clip
sunox persona set <id> --name "My Voice" Update voice persona metadata
sunox persona processed-clip <id> View processed vocal clip status
sunox persona publish <id> Make a voice persona public
sunox persona unpublish <id> Make a voice persona private
sunox persona love <id> Favorite a voice persona
sunox persona unlove <id> Remove a voice persona favorite
sunox persona toggle-love <id> Toggle favorite state for a voice persona
sunox persona delete <id> -y Move a voice persona to trash
sunox persona restore <id> Restore a trashed voice persona
sunox persona purge <id> -y Permanently delete a trashed voice persona
sunox playlist list   List your playlists
sunox playlist info <id> View playlist details
sunox clip status <ids> Check generation progress
sunox clip wait <ids>   Wait for generated clips to complete (use --timeout <secs>)
sunox credits         Show balance and plan info
sunox models          List generation and remaster models with account availability
```

### Manage

```
sunox download <ids>       Download audio/video; default CDN MP3 embeds lyrics (explicit --format mp3|m4a|wav|opus, --video for MP4)
sunox clip download <ids>  Agent/advanced equivalent of `sunox download`
sunox clip upload <file>   Upload local audio into your Suno library (--upload-type, --stem-mix, --timeout)
sunox clip upload-status <upload_id>  Read existing audio upload processing status
sunox clip delete <ids> -y Delete/trash clips
sunox clip restore <ids>   Restore trashed clips
sunox clip purge <ids> -y Permanently delete trashed clips
sunox clip empty-trash -y Permanently delete every trashed clip
sunox clip like <ids>      Like clips (--clear to remove like)
sunox clip dislike <ids>   Dislike clips (--clear to remove dislike)
sunox clip set <id>        Update title, lyrics, caption, or cover
sunox clip publish <ids>   Toggle public/private visibility (--private for private)
sunox playlist create Create a playlist
sunox playlist set <id> Update playlist name or description
sunox add <clip_ids> --to <id> Human shortcut for adding songs to a playlist
sunox playlist add <id> <clip_ids> Agent/advanced playlist add
sunox playlist remove <id> Remove clips from a playlist
sunox playlist publish <id> Toggle public/private visibility
sunox playlist reorder <id> Move a clip to another playlist index
sunox playlist restore <id> Restore a trashed playlist
sunox playlist save <id> Save a playlist to your library
sunox playlist unsave <id> Remove a saved playlist
sunox playlist like <id> Like a playlist (--clear to remove)
sunox playlist dislike <id> Dislike a playlist (--clear to remove)
sunox playlist delete <id> -y Delete/trash a playlist
sunox clip timed-lyrics    Get word-level timestamped lyrics (--lrc for LRC format)
```

### Config & Auth

```
sunox login          Set up authentication from browser
sunox logout         Remove stored auth and interactive login profile
sunox auth           Advanced auth: refresh, cookie, jwt
sunox config         show | set | check
sunox doctor         Diagnose config and auth
sunox doctor --network Diagnose DNS, direct TCP, and HTTPS connectivity (`--strict` for non-zero on degradation)
sunox agent-info     Machine-readable capabilities JSON
sunox install-skill  Install agent skill into Codex / Claude Code / Cursor
sunox update         Self-update from GitHub Releases (--check to peek first)
```

## Features

Studio functionality is outside the scope of this CLI.

### Zero-Friction Auth

```bash
sunox login    # Browser-cookie auth, with interactive Chrome/Edge fallback
```

`sunox login` first tries to read the Clerk auth cookie from Chrome, Arc, Brave, Firefox, or Edge. If that succeeds, Sunox records a stable browser source id for the extractor that produced the session and best-effort public profile settings such as accepted languages; it does not fabricate a user-agent from the browser label. If browser-cookie extraction fails, it opens a dedicated Sunox Chrome/Edge-compatible browser profile and waits for you to log into Suno there. The captured Clerk session is exchanged for a JWT and used to refresh stale JWTs automatically while the underlying session is still valid. When interactive login is used, stable browser runtime headers such as user-agent and accepted languages are saved and reused for later API calls. API requests derive Chromium client hints from the selected user-agent, send browser fetch metadata headers, and fall back field-by-field when real browser values are unavailable.

Credentials are stored as local JSON, not in an OS keychain. Sunox creates the auth file with mode `0600` on Unix; on Windows it relies on the per-user ACL of the configuration directory. Manual `--cookie` and `--jwt` values can be visible in shell history and process listings, so prefer `sunox login` or pipe secrets through `--cookie-stdin` / `--jwt-stdin`; never include credentials in logs, prompts, project files, or commits.

Auth methods (in order of convenience):
1. `sunox login` — automatic browser extraction, with interactive Chrome/Edge fallback (recommended)
2. `printf '%s' "$SUNOX_COOKIE_INPUT" | sunox auth --cookie-stdin` — safe stdin input for headless servers; accepts either raw `__client` or a full browser `Cookie` header
3. `printf '%s' "$SUNOX_JWT_INPUT" | sunox auth --jwt-stdin` — safe stdin JWT input
4. `sunox auth --refresh` — force a fresh JWT from the stored Clerk session

`sunox auth` with no flags checks the existing session, or starts browser login if no auth is configured. Auth, login, and logout emit normal success envelopes with `--json`. `sunox logout` removes stored credentials, the interactive login profile, and any legacy captcha profile.

### Generation Parameters

| Flag | What it does | Values |
|---|---|---|
| `--title` | Song title | up to 100 chars |
| `--tags` | Style direction | Model/account limit; inspect `sunox models --json` |
| `--enhance-tags` | Ask Suno to enhance style tags before submit | explicit opt-in |
| `--exclude` | Styles to avoid | Model/account limit; inspect `sunox models --json` |
| `--lyrics` / `--lyrics-file` | Custom lyrics with `[Verse]` tags | `max_lengths.gpt_description_prompt` |
| `--prompt` (describe) | Free text description | `max_lengths.prompt` |
| `--model` | Model version | v5.5, v5, v4.5+, v4.5-all, v4.5, v4, v3.5, v3, v2 |
| `--vocal` | Vocal gender | male, female |
| `--persona` | Voice persona ID | UUID from Suno voice creation |
| `--weirdness` | How experimental | 0-100 |
| `--style-influence` | How strictly to follow tags | 0-100 |
| `--instrumental` | No vocals | flag |

### Voice Personas

Generate songs using your own voice. Create a voice in Suno's web UI, then use the persona ID:

```bash
# List and view persona details
sunox persona list
sunox persona info <persona_id>
sunox persona clips <persona_id> --page 1
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"  # private by default
sunox persona create <clip_id> --name "Public Voice" --public                    # explicit opt-in
sunox persona set <persona_id> --name "My Voice" --description "Warm lead vocal" --public false
sunox persona processed-clip <processed_clip_id>
sunox persona publish <persona_id>        # only when you explicitly want it public
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona toggle-love <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id>
sunox persona purge <persona_id> -y       # permanent deletion

# Generate with your voice
sunox create --persona <persona_id> --title "My Song" --tags "pop" --lyrics "[Verse]\nHello world"

# Works with describe mode too
sunox create --persona <persona_id> --title "Starlight" "a warm ballad about starlight"
```

Persona publish/unpublish uses Suno Web's `set_visibility` endpoint. Persona deletion moves a voice persona to trash; restore and purge use the same Suno Web bulk trash endpoint with the web bundle's `undo`/`hide` modes.

### Playlists

```bash
# List and inspect playlists
sunox playlist list
sunox playlist info <playlist_id>

# Create and edit metadata
sunox playlist create --name "Release candidates" --description "Tracks to review" --image-url <cover_url>
sunox playlist set <playlist_id> --name "Final shortlist" --image-url <cover_url>
sunox playlist set <playlist_id> --image-file ./cover.png

# Manage songs in a playlist
sunox playlist add <playlist_id> <clip_id_1> <clip_id_2>
sunox playlist remove <playlist_id> <clip_id_1>
sunox playlist publish <playlist_id>            # only when you explicitly want it public
sunox playlist publish <playlist_id> --private  # make private
sunox playlist reorder <playlist_id> --clip-id <clip_id> --index 0
sunox playlist restore <playlist_id>
sunox playlist save <playlist_id>
sunox playlist unsave <playlist_id>
sunox playlist like <playlist_id>
sunox playlist like <playlist_id> --clear
sunox playlist dislike <playlist_id>
sunox playlist delete <playlist_id> -y
```

### Clip Transforms

Create covers, remaster clips, or apply non-generation clip edits:

```bash
# Cover with different style tags
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5
sunox clip inspire <clip_id> --title "New Song" --tags "garage pop" --lyrics-file lyrics.txt

# Remaster an old clip with the latest model. Variation defaults to normal.
sunox clip remaster <clip_id> --model v5.5 --variation subtle
sunox clip remaster <clip_id> --model v5.5 --variation high
sunox clip wait <new_clip_id>
sunox clip download <new_clip_id> --output ./remastered/

# Change playback speed while keeping pitch
sunox clip speed <clip_id> --multiplier 0.94

# Reverse audio
sunox clip reverse <clip_id>

# Trim to a section, or remove a section from the middle
sunox clip crop <clip_id> --start 12.5 --end 74.0
sunox clip crop <clip_id> --start 30.0 --end 45.0 --remove-section

# Add fade in/out
sunox clip fade <clip_id> --in 2.0 --out 78.5
```

Cover uses Suno's unified web generation endpoint (`/api/generate/v2-web/`);
remaster, speed, reverse, crop, and fade use their dedicated current web routes.
Create, cover, remaster, speed, and reverse can return submitted or processing
clips, so wait before downstream work. Crop and fade wait internally and return
only after the result clip is complete.

### Clip Info

```bash
# Full details for any clip
sunox clip info <clip_id>

# JSON for scripting
sunox clip info <clip_id> --json | jq '.data.audio_url'
```

JSON output also includes `attribution`, `comments`, `direct_children_count`,
and `similar_clips` from the current song page APIs. If a non-auth,
non-rate-limit supplemental read fails, the base clip still returns with
`supplemental_errors`; auth and rate-limit errors still abort normally.

### Edit & Manage

```bash
# Update title and lyrics on an existing clip
sunox clip set <clip_id> --title "New Title" --lyrics-file updated.txt

# Replace or remove a clip cover
sunox clip set <clip_id> --image-file ./cover.png
sunox clip set <clip_id> --image-url <cover_url>
sunox clip set <clip_id> --remove-cover
sunox clip set <clip_id> --remove-video-cover

# Make clips public only when explicitly requested
sunox clip publish <clip_id_1> <clip_id_2>

# Trash, restore, or react to clips
sunox clip delete <clip_id> -y
sunox clip restore <clip_id>
sunox clip purge <clip_id> -y       # irreversible
sunox clip empty-trash -y           # irreversible: clear all trashed clips
sunox clip like <clip_id>
sunox clip like <clip_id> --clear
sunox clip dislike <clip_id>

# Get timed lyrics in LRC format
sunox clip timed-lyrics <clip_id> --lrc > song.lrc
```

### Downloads with Embedded Lyrics

Downloads automatically embed lyrics into MP3 files via ID3 tags:
- **USLT** (plain lyrics) — shown in most music players
- **SYLT** (synced word-by-word timestamps) — shown in Apple Music with timing

Authentication and rate-limit failures while fetching timed lyrics abort the download. Other supplemental failures keep the MP3 with available plain lyrics and add a structured `warnings` entry in JSON output. Downloads have a two-hour total deadline and a 2 GiB safety limit; Ctrl-C removes staging files.

```bash
sunox download <id1> <id2> --output ./songs/

# Existing destination files require an explicit overwrite
sunox download <id1> --output ./songs/ --force

# Request a specific audio format
sunox download <id1> --format wav --output ./songs/
sunox download <id1> --format m4a --output ./songs/

# Download the MP4 video render instead of audio
sunox download <id1> --video --output ./videos/
```

Files use slug format: `title-slug-clipid8.<ext>` — no overwrites when Suno generates 2 variations. Output directories are created automatically. Existing files are preserved unless `--force` is supplied. MP3 is the default and embeds lyrics; M4A, WAV, and OPUS are explicit format requests.

### Audio Uploads

Upload a local audio file into your Suno library. The CLI creates the presigned
Suno upload, posts the bytes to S3, waits for processing, initializes a clip,
and can set title or lyrics metadata.

```bash
sunox clip upload ./demo.mp3 --title "Demo Upload"
sunox clip upload ./demo.wav --lyrics-file lyrics.txt --timeout 900

# Mark the uploaded audio as a stem mix
sunox clip upload ./vocal-stem.wav --stem-mix --title "Vocal stem"

# Override the Suno upload_type value (default: file_upload)
sunox clip upload ./demo.mp3 --upload-type file_upload

# Read status without replaying any upload mutation
sunox clip upload-status <upload_id> --json
```

### Models

| Version | Codename | CLI default | Notes |
|---|---|---|---|
| auto | account response | Yes | Current usable account default |
| v5.5 | chirp-fenix | | Latest generation; unavailable-billing fallback |
| v5 | chirp-crow | | Previous generation |
| v4.5+ | chirp-bluejay | | Extended capabilities |
| v4.5-all | chirp-auk-turbo | | Free-tier option when available |
| v4.5 | chirp-auk | | Stable |
| v4 | chirp-v4 | | Legacy |
| v3.5 | chirp-v3-5 | | Legacy |
| v3 | chirp-v3-0 | | Legacy |
| v2 | chirp-v2-xxl-alpha | | Legacy |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

Model availability, the account default, and length limits are account-specific. The default
`default_model=auto` resolves the current usable account default directly from
`/api/billing/info/`; `sunox models --json` returns separate `generation` and `remaster` arrays from the same account data. Explicit
models are validated against `can_use` and `max_lengths` when billing info is available; v5.5 is
used only when that read is unavailable.

### Agent-Friendly

The human surface is intentionally small; the full resource API is for agents
and scripts. Start with `sunox agent-info --json` to discover supported
commands, features, models, exit codes, and recommended workflows.

Every command supports `--json` for structured output. When stdout is piped, JSON is auto-detected. Progress and errors go to stderr. Suno write commands are account-scoped serial by default; do not use `sunox config set serial_mutations false`, `-c serial_mutations=false`, or `--parallel` unless the user explicitly allows same-account concurrent writes.

For routine audio inspection, use the existing clip media: `sunox clip info <id> --json` exposes `audio_url` plus song-page context (`attribution`, `comments`, `direct_children_count`, and `similar_clips`), and default `sunox clip download` downloads that CDN MP3 and embeds lyrics; explicit `--format mp3|m4a|wav|opus` requests an official Suno download format, and `--video` uses `clip.video_url` when present. If a non-auth, non-rate-limit supplemental read fails, `clip info` still returns the base clip with `supplemental_errors`; auth and rate-limit errors abort normally. `sunox clip stems` is generation-backed stems extraction and is not the same as Suno Web Pro Get Stems export. Agents should use an explicit format, stems, or video only when the user explicitly requests it. `--quiet` suppresses download progress and ordinary status output. If a batch download returns `partial_download`, inspect `error.details.succeeded`, `error.details.failed`, and `error.details.not_attempted_clip_ids`, then retry only the necessary IDs. If `playlist remove` or a multi-clip publish/reaction command returns `partial_mutation`, inspect `error.details.succeeded_clip_ids`, `error.details.failed`, and `error.details.not_attempted_clip_ids` before retrying. Do not publish, make public, force `--captcha`, print auth material, or run destructive commands unless the user explicitly asks for that action; destructive commands require `-y/--yes`.

Playlist create/set, local image upload, clip cover update, and audio upload are multi-step workflows. After server-side work exists, later failures return `partial_mutation` with resource IDs, `completed_steps`, `failed.step/code/message`, and `recovery`. Follow the structured recovery command only when `recovery.resumable=true`; never replay a mutation marked false. Audio files are streamed to the presigned transfer endpoint rather than buffered entirely in memory, and a metadata-changing audio upload polls until the requested fields are visible. `clip upload-status` is read-only and does not resume or replay mutations.

Exit codes are semantic:

| Code | Meaning | Agent action |
|---|---|---|
| 0 | Success | Continue |
| 1 | Runtime, web endpoint, partial mutation or partial download error | Inspect `error.code` and `error.details` before retrying |
| 2 | Config error | Fix config, don't retry |
| 3 | Auth error | Run `sunox login` |
| 4 | Rate limited | Wait 30-60s, retry |
| 5 | Not found | Verify resource ID |

Error responses include actionable suggestions:

```json
{
  "version": "1",
  "status": "error",
  "error": {
    "code": "auth_expired",
    "message": "JWT expired or rejected by Suno",
    "suggestion": "Run `sunox auth --refresh`; if that fails, run `sunox login`"
  }
}
```

```bash
# Pipe-friendly: auto-JSON when piped
sunox clip list | jq '.data.clips[0].title'
sunox clip list --liked --public --sort popular --json

# Agent capabilities discovery
sunox agent-info --json
```

### Install as a Coding Agent Skill

Teach Codex, Claude Code, or Cursor how to use `sunox` with one command:

```bash
# Codex / Trae CLI (~/.codex/skills/sunox/SKILL.md)
sunox install-skill

# Claude Code (~/.claude/skills/sunox/SKILL.md)
sunox install-skill --target claude

# Cursor (./.cursor/rules/sunox.mdc in the current workspace)
sunox install-skill --target cursor

# Print the skill content without writing
sunox install-skill --print

# Custom path
sunox install-skill --path ~/my-agents/sunox.md --force
```

After installation, your coding agent automatically picks up the skill on the next session and knows how to invoke `sunox` for music generation, downloads, stems, covers, remasters, and clip edits.

### Web Endpoint Versions (Implementation Notes)

| Endpoint | Version | Status |
|---|---|---|
| Feed | **v3** (`POST /api/feed/v3`) | Latest |
| Generate | **v2-web** (`POST /api/generate/v2-web/`) | HAR-confirmed custom create body |
| Inspiration | **v2-web** (`task: playlist_condition`) | HAR-confirmed; exposed as single-source `clip inspire` |
| Stems | **v2-web** (`POST /api/generate/v2-web/`, `task: "gen_stem"`) | HAR-confirmed current web stem task |
| Remaster | `POST /api/generate/upsample` | HAR-confirmed current web remaster route |
| Speed adjust | `POST /api/clips/adjust-speed/` | HAR-confirmed current web edit route |
| Reverse | `POST /api/clips/reverse-clip/` | Bundle-confirmed body; live CLI result verified |
| Crop / remove section | `POST /api/edit/crop/{id}/`, then `GET /api/edit/action/{action_clip_id}/` | Bundle-confirmed body; live CLI result verified |
| Fade | `POST /api/edit/fade/{id}/`, then poll edit action | Live browser-verified body and completion polling |
| Concat | **v2** (`POST /api/generate/concat/v2/`) | Bundle-confirmed body; live CLI submit/complete verified for an original generation source |
| Download | `GET /api/download/clip/{id}?format=mp3\|m4a`, `POST /api/gen/{id}/convert_wav/`, `GET /api/gen/{id}/wav_file/`, `GET/POST /api/gen/{id}/opus_file/` | Bundle-confirmed current web download routes |
| Aligned lyrics | **v2** (`GET /api/gen/{id}/aligned_lyrics/v2/`) | Latest |
| Persona list | `GET /api/persona/get-personas/` | Bundle-confirmed |
| Persona detail | `GET /api/persona/get-persona/{id}/` | Bundle-confirmed |
| Persona clips | `GET /api/persona/get-persona-paginated/{id}/?page=N` | HAR-confirmed on voice detail page |
| Persona create | `POST /api/persona/create/` | Bundle-confirmed, not live-mutated by tests |
| Persona edit | `PUT /api/persona/edit-persona/{id}/` | HAR-confirmed |
| Processed clip | `GET /api/processed_clip/{id}` | HAR-confirmed on voice detail page |
| Persona visibility | `PUT /api/persona/set_visibility/{id}/?is_public=true\|false` | HAR-confirmed |
| Persona trash/restore/purge | `PUT /api/persona/bulk-trash-personas/` | Bundle-confirmed modes: trash `{undo:false,hide:false}`, restore `{undo:true,hide:false}`, purge `{undo:false,hide:true}` |
| Playlist list | `GET /api/playlist/me` | Bundle-confirmed |
| Playlist detail | `GET /api/playlist/v2/{id}` | Bundle-confirmed |
| Playlist mutation | `POST /api/playlist/create/`, `POST /api/playlist/set_metadata`, `PATCH /api/playlist/v2/{id}`, `POST/DELETE /api/playlist/v2/{id}/save`, `POST /api/playlist/v2/{id}/tracks/{add,remove}`, `POST /api/playlist/v2/{id}/tracks/reorder-by-index`, `POST /api/playlist/v2/{id}/trash`, `POST /api/playlist_reaction/{id}/update_reaction_type/` | Bundle-confirmed; playlist cover upload live-verified through image upload + v2 metadata patch |
| Persona love | `POST /api/persona/{id}/toggle_love/` | HAR-confirmed empty-body mutation |
| Clip trash/restore | `POST /api/gen/trash` | Bundle-confirmed; live CLI-verified July 10, 2026 |
| Clip permanent delete | `POST /api/clips/delete/` | Current frontend-confirmed body `{"ids":[...]}`; live CLI-verified July 10, 2026 |
| Clip reaction | `POST /api/gen/{id}/update_reaction_type/` | HAR-confirmed body with `recommendation_metadata` |
| Audio upload | `POST /api/uploads/audio/`, presigned S3 form upload, `POST /api/uploads/audio/{id}/upload-finish/`, `GET /api/uploads/audio/{id}/`, `POST /api/uploads/audio/{id}/initialize-clip/` | CLI workflow implemented and live-verified for `file_upload` |
| Image upload | `POST /api/uploads/image/`, presigned S3 form upload, `POST /api/uploads/image/{id}/upload-finish/` | CLI workflow implemented for clip and playlist covers; playlist cover patch uses `PATCH /api/playlist/v2/{id}` with `cover_url`, `cover_image_s3_id`, `cover_is_user_set`; clip cover patch uses `POST /api/gen/{id}/set_metadata/` with `image_url` |

Generation tasks use `/api/generate/v2-web/`. The custom create payload was live-recaptured on June 30, 2026: custom lyrics are sent as `gpt_description_prompt` while `prompt` stays empty, and a solved challenge token uses `token_provider: 1`. Sunox resolves `metadata.user_tier`, account model availability, the account default, and field limits from `/api/billing/info/`; `default_model=auto` falls back to v5.5 only when that read is unavailable. With `--enhance-tags`, Sunox first calls `/api/prompts/upsample`, carries the returned tags plus `request_id` into `metadata.last_tags_generation`, and marks `override_fields=["tags"]`; the `personalization_enabled` field follows the captured web submit shape. Without that flag it omits `metadata.last_tags_generation`. Instrumental create also uses custom mode; when `sunox create --instrumental <prompt>` is used, the prompt is folded into style tags and the submitted `prompt` field stays empty, matching the live web request shape recaptured in `15suno-labs-nostudio-20260630.har`. `sunox clip inspire` implements the live-captured `task: "playlist_condition"` flow for one source clip: it upsamples the supplied tags, puts lyrics in `prompt`, carries the real upsample metadata, and does not expose unverified instrumental or multi-source variants. Extend fetches the source clip before submit, uses a feed/v3 exact-id metadata fallback when `GET /api/feed/?ids` omits source style metadata, sends `title` as the source title unless `--title` is provided, defaults `tags` and `negative_tags` from the source when available, and inherits `metadata.make_instrumental` unless `--instrumental` or `--no-instrumental` overrides it; use `--tags` and `--exclude` to override the inherited values. `clip list` uses `POST /api/feed/v3` and exposes query-only filters such as `--liked`, `--public`, `--upload`, `--cover`, `--extend`, and `--sort popular`; this is not a library sync workflow. Remaster uses the live-captured `/api/generate/upsample` route, speed adjust uses `/api/clips/adjust-speed/`, reverse uses `/api/clips/reverse-clip/`, and crop/fade use `/api/edit/*` action routes. Concat completed in a live CLI validation from an original generation source; edited inputs can be rejected by Suno with `Bad history.` Commands that submit through `/api/generate/v2-web/` preflight `/api/c/check` with `ctype=generation`; if Suno reports a challenge and the stored Clerk session can refresh, Sunox refreshes the JWT once and repeats the preflight before asking for a solved token. When no challenge remains they submit without a challenge token, and when a challenge is still required you can use `--token <solved>` to supply one or `--captcha` to force the browser solver on create, inspire, cover, extend, and stems. The audio upload workflow was live-verified for `file_upload`; clip cover upload was live-verified through image upload plus clip metadata update; playlist cover upload was live-verified through image upload plus v2 metadata patch. Cover generation and concat browser mutation bodies still need fresh live captures. Playlist mutations are implemented from bundle/live evidence plus endpoint contract tests; playlist remove intentionally submits one clip per request because larger live batches can return Suno 500s.

## Contributing

1. Create a branch (`git checkout -b feature/your-idea`)
2. Make your changes and test with `cargo test`
3. Open a PR

We especially welcome:
- Integration tests with `assert_cmd`
- OS keychain/Secret Service/CredMan storage for auth secrets

## License

MIT — see [LICENSE](LICENSE).

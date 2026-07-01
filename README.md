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

A single Rust binary that talks directly to Suno's web endpoints. Generate songs with custom lyrics, style tags, your own voice persona, vocal control, weirdness/style sliders, covers, remasters, speed edits, and stems. Zero-friction auth — one command extracts credentials from your browser automatically.

**Languages:** English | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

[Install](#install) | [Quick Start](#quick-start) | [Human Commands](#human-commands) | [Agent & Advanced Commands](#agent--advanced-commands) | [Features](#features) | [Contributing](#contributing)

</div>

## Why

Suno's web UI works, but it is not built for scripting, piping lyrics from a file, batch generation, or integration into a terminal-based music workflow.

This CLI fixes that. Auto-auth from your browser, core generation parameters exposed as flags, dual JSON/table output for both humans and AI agents. Downloads auto-embed synced lyrics into MP3 files.

## Install

### Cargo (any platform)

```bash
cargo install sunox
```

### Pre-built binaries

Download from [GitHub Releases](https://github.com/ctykwz/sunox/releases) — binaries for macOS (Apple Silicon + Intel), Linux (x86_64 + ARM), and Windows.

### Self-update

Already have `sunox` installed? Pull the latest binary from GitHub Releases without touching your package manager:

```bash
sunox update --check    # see what's available
sunox update            # install the latest release
```

> Tip: when Suno changes its web schema mid-cycle, run `sunox update` first — it's faster than `cargo install sunox` or waiting for the Homebrew bottle to refresh.

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
| `-c key=value` / `--config key=value` | Override a config value for this invocation, e.g. `-c default_model=v5.5 -c output_dir=./songs` (repeatable) |
| `-V` / `--version` | Print the CLI version |
| `-h` / `--help` | Subcommand-aware help |

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
sunox clip remaster       Remaster with a different model version
sunox clip speed          Adjust playback speed
sunox clip stems          Extract vocals and instruments
```

### Browse & Inspect

```
sunox clip list            List your songs
sunox clip list --cursor <next_cursor>
sunox clip search <query>  Search songs by title or tags
sunox clip info <id>       Detailed view of a single clip
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
sunox persona delete <id> Move a voice persona to trash
sunox persona restore <id> Restore a trashed voice persona
sunox persona purge <id> Permanently delete a trashed voice persona
sunox playlist list   List your playlists
sunox playlist info <id> View playlist details
sunox clip status <ids> Check generation progress
sunox clip wait <ids>   Wait for generated clips to complete (use --timeout <secs>)
sunox credits         Show balance and plan info
sunox models          List available models with limits
```

### Manage

```
sunox download <ids>       Download audio/video with embedded lyrics (--video for MP4)
sunox clip download <ids>  Agent/advanced equivalent of `sunox download`
sunox clip upload <file>   Upload local audio into your Suno library (--upload-type, --stem-mix, --timeout)
sunox clip delete <ids>    Delete/trash clips
sunox clip restore <ids>   Restore trashed clips
sunox clip like <ids>      Like clips (--clear to remove like)
sunox clip dislike <ids>   Dislike clips (--clear to remove dislike)
sunox clip set <id>        Update title, lyrics, caption, or remove cover
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
sunox playlist delete <id> Delete/trash a playlist
sunox clip timed-lyrics    Get word-level timestamped lyrics (--lrc for LRC format)
```

### Config & Auth

```
sunox login          Set up authentication from browser
sunox logout         Remove stored auth and interactive login profile
sunox auth           Advanced auth: refresh, cookie, jwt
sunox config         show | set | check
sunox doctor         Diagnose config and auth
sunox agent-info     Machine-readable capabilities JSON
sunox install-skill  Install agent skill into Codex / Claude Code / Cursor
sunox update         Self-update from GitHub Releases (--check to peek first)
```

## Features

### Zero-Friction Auth

```bash
sunox login    # Browser-cookie auth, with interactive Chrome/Edge fallback
```

`sunox login` first tries to read the Clerk auth cookie from Chrome, Arc, Brave, Firefox, or Edge. If that fails, it opens a dedicated Sunox Chrome/Edge-compatible browser profile and waits for you to log into Suno there. The captured Clerk session is exchanged for a JWT, stored in a `0600` local auth file, and used to refresh stale JWTs automatically while the underlying session is still valid.

Auth methods (in order of convenience):
1. `sunox login` — automatic browser extraction, with interactive Chrome/Edge fallback (recommended)
2. `sunox auth --cookie <cookie>` — manual paste for headless servers; accepts either raw `__client` or a full browser `Cookie` header
3. `sunox auth --jwt <token>` — direct JWT, expires in ~1 hour
4. `sunox auth --refresh` — force a fresh JWT from the stored Clerk session

`sunox auth` with no flags checks the existing session, or starts browser login if no auth is configured. `sunox logout` removes stored credentials and the dedicated interactive browser profile.

### Generation Parameters

| Flag | What it does | Values |
|---|---|---|
| `--title` | Song title | up to 100 chars |
| `--tags` | Style direction | `"pop, synths, upbeat"` (1000 chars) |
| `--exclude` | Styles to avoid | `"metal, heavy, dark"` (1000 chars) |
| `--lyrics` / `--lyrics-file` | Custom lyrics with `[Verse]` tags | up to 5000 chars |
| `--prompt` (describe) | Free text description | up to 500 chars |
| `--model` | Model version | v5.5, v5, v4.5+, v4.5, v4, v3.5, v3, v2 |
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
sunox persona create <clip_id> --name "My Voice" --description "Warm lead vocal"
sunox persona set <persona_id> --name "My Voice" --description "Warm lead vocal" --public false
sunox persona processed-clip <processed_clip_id>
sunox persona publish <persona_id>
sunox persona unpublish <persona_id>
sunox persona love <persona_id>
sunox persona unlove <persona_id>
sunox persona toggle-love <persona_id>
sunox persona delete <persona_id> -y
sunox persona restore <persona_id> -y
sunox persona purge <persona_id> -y

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

# Manage songs in a playlist
sunox playlist add <playlist_id> <clip_id_1> <clip_id_2>
sunox playlist remove <playlist_id> <clip_id_1>
sunox playlist publish <playlist_id>            # make public
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

Create covers, remaster clips, or adjust playback speed:

```bash
# Cover with different style tags
sunox clip cover <clip_id> --tags "jazz, smooth piano" --model v5.5

# Remaster an old clip with the latest model
sunox clip remaster <clip_id> --model v5.5
sunox clip wait <new_clip_id>
sunox clip download <new_clip_id> --output ./remastered/

# Change playback speed while keeping pitch
sunox clip speed <clip_id> --multiplier 0.94
```

Cover uses Suno's unified web generation endpoint (`/api/generate/v2-web/`);
remaster and speed adjust use their dedicated current web routes.

### Clip Info

```bash
# Full details for any clip
sunox clip info <clip_id>

# JSON for scripting
sunox clip info <clip_id> --json | jq '.data.audio_url'
```

### Edit & Manage

```bash
# Update title and lyrics on an existing clip
sunox clip set <clip_id> --title "New Title" --lyrics-file updated.txt

# Make clips public
sunox clip publish <clip_id_1> <clip_id_2>

# Trash, restore, or react to clips
sunox clip delete <clip_id> -y
sunox clip restore <clip_id>
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

```bash
sunox download <id1> <id2> --output ./songs/

# Download the MP4 video render instead of audio
sunox download <id1> --video --output ./videos/
```

Files use slug format: `title-slug-clipid8.mp3` — no overwrites when Suno generates 2 variations.

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
```

### Models

| Version | Codename | Default | Notes |
|---|---|---|---|
| **v5.5** | chirp-fenix | Yes | Latest, best quality |
| v5 | chirp-crow | | Previous generation |
| v4.5+ | chirp-bluejay | | Extended capabilities |
| v4.5 | chirp-auk | | Stable |
| v4 | chirp-v4 | | Legacy |
| v3.5 | chirp-v3-5 | | Legacy |
| v3 | chirp-v3-0 | | Legacy |
| v2 | chirp-v2-xxl-alpha | | Legacy |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

### Agent-Friendly

The human surface is intentionally small; the full resource API is for agents
and scripts. Start with `sunox agent-info --json` to discover supported
commands, features, models, exit codes, and recommended workflows.

Every command supports `--json` for structured output. When stdout is piped, JSON is auto-detected. Progress and errors go to stderr. Exit codes are semantic:

| Code | Meaning | Agent action |
|---|---|---|
| 0 | Success | Continue |
| 1 | Runtime error (network, web endpoint) | Retry with backoff |
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
sunox clip list | jq '.data[0].title'

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

After installation, your coding agent automatically picks up the skill on the next session and knows how to invoke `sunox` for music generation, downloads, stems, covers, remasters, and speed edits.

### Web Endpoint Versions (Implementation Notes)

| Endpoint | Version | Status |
|---|---|---|
| Feed | **v3** (`POST /api/feed/v3`) | Latest |
| Generate | **v2-web** (`POST /api/generate/v2-web/`) | HAR-confirmed custom create body |
| Stems | **v2-web** (`POST /api/generate/v2-web/`, `task: "gen_stem"`) | HAR-confirmed current web stem task |
| Remaster | `POST /api/generate/upsample` | HAR-confirmed current web remaster route |
| Speed adjust | `POST /api/clips/adjust-speed/` | HAR-confirmed current web edit route |
| Concat | **v2** (`POST /api/generate/concat/v2/`) | Implemented contract; no live body in June 30 HAR |
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
| Playlist mutation | `POST /api/playlist/create/`, `POST /api/playlist/set_metadata`, `PATCH /api/playlist/v2/{id}`, `POST/DELETE /api/playlist/v2/{id}/save`, `POST /api/playlist/v2/{id}/tracks/{add,remove}`, `POST /api/playlist/v2/{id}/tracks/reorder-by-index`, `POST /api/playlist/v2/{id}/trash`, `POST /api/playlist_reaction/{id}/update_reaction_type/` | Bundle-confirmed, not live-mutated by tests |
| Persona love | `POST /api/persona/{id}/toggle_love/` | HAR-confirmed empty-body mutation |
| Clip trash/restore | `POST /api/gen/trash` | Bundle-confirmed, not live-mutated by tests |
| Clip reaction | `POST /api/gen/{id}/update_reaction_type/` | HAR-confirmed body with `recommendation_metadata` |
| Audio upload | `POST /api/uploads/audio/`, presigned S3 form upload, `POST /api/uploads/audio/{id}/upload-finish/`, `GET /api/uploads/audio/{id}/`, `POST /api/uploads/audio/{id}/initialize-clip/` | CLI workflow implemented and live-verified for `file_upload` |

Generation tasks use `/api/generate/v2-web/`. The custom create payload was live-recaptured on June 30, 2026: custom lyrics are sent as `gpt_description_prompt` while `prompt` stays empty, and a solved challenge token uses `token_provider: 1`. Instrumental create also uses custom mode; when `sunox create --instrumental <prompt>` is used, the prompt is folded into style tags and the submitted `prompt` field stays empty, matching the live web request shape recaptured in `15suno-labs-nostudio-20260630.har`. `task: "playlist_condition"` was also captured and intentionally treated as a separate inspiration flow because it puts lyrics in `prompt`. Remaster uses the live-captured `/api/generate/upsample` route, and speed adjust uses `/api/clips/adjust-speed/`. Authenticated generation defaults to submitting without a challenge token; use `--token <solved>` to supply one or `--captcha` to force the browser solver. The audio upload workflow was live-verified for `file_upload`; cover, concat, and playlist mutation bodies still need live mutation captures.

## Contributing

1. Create a branch (`git checkout -b feature/your-idea`)
2. Make your changes and test with `cargo test`
3. Open a PR

We especially welcome:
- Integration tests with `assert_cmd`
- OS keychain/Secret Service/CredMan storage for auth secrets

## License

MIT — see [LICENSE](LICENSE).

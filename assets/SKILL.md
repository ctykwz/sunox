---
name: sunox
description: Generate AI music from the terminal using the `sunox` CLI. Use when user asks to "generate a song", "make music", "create AI music", "make a track", "generate audio", or wants to programmatically use Suno web workflows for custom lyrics, tags, voice personas, playlists, covers, remasters, speed edits, or stems. Also use when downloading Suno songs (auto-embeds lyrics into MP3). Run `sunox agent-info` for the full machine-readable capability dump. NOT for writing song prompts/lyrics without generating audio — use `suno-song-generator` for that.
---

# sunox CLI

Generate AI music from your terminal using direct Suno web workflows, including custom lyrics, style tags, voice personas, playlists, covers, remasters, speed edits, stems extraction, and word-level timed lyrics.

## When to use

- User wants to **generate** AI music programmatically through the sunox CLI
- User wants to **download** Suno songs (auto-embeds USLT + SYLT lyrics into MP3 ID3 tags)
- User wants to **batch generate**, **script**, or **integrate** Suno into a music workflow
- User mentions Suno, AI music generation, or wants to control Suno parameters from the terminal

## When NOT to use

- Writing song lyrics or Suno-formatted prompts without actually generating audio → use the `suno-song-generator` skill instead
- General music theory, composition advice, or non-Suno music tasks

## Setup (first time on a new machine)

```bash
# Auto-extract auth from browser (Chrome / Arc / Brave / Firefox / Edge)
sunox login

# Verify it worked
sunox doctor
```

If `sunox login` fails on a headless box, fall back to:

```bash
sunox auth --cookie '<Cookie header or __client value>'  # paste from browser DevTools
sunox auth --jwt '<jwt>'                                  # ~1 hour lifetime
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

Treat generation as an asynchronous workflow: submit with `sunox create`, wait
with `sunox clip wait`, then fetch media with `sunox clip download` or the
human shortcut `sunox download`.

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

# Wait for returned clip IDs, then download completed MP3s
sunox clip wait <clip_id_1> <clip_id_2>
sunox clip download <clip_id_1> <clip_id_2> --output ./songs/

# Upload a local audio file into the Suno library
sunox clip upload ./demo.mp3 --title "Demo Upload"

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

# List / search your library
sunox clip list
sunox clip list --cursor <next_cursor>
sunox clip search "rainy"

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
sunox clip remaster <clip_id> --model v5.5

# Adjust playback speed while keeping pitch
sunox clip speed <clip_id> --multiplier 0.94

# Extract stems (vocals + instruments)
sunox clip stems <clip_id>

# Word-level timed lyrics (LRC format for synced display)
sunox clip timed-lyrics <clip_id> --lrc > song.lrc

# Download with auto-embedded synced lyrics
sunox download <clip_id_1> <clip_id_2> --output ./songs/

# Manage clips
sunox clip set <clip_id> --title "New Title" --lyrics-file updated.txt
sunox clip publish <clip_id_1> <clip_id_2>          # make public
sunox clip publish <clip_id_1> --private            # make private
sunox clip delete <clip_id> -y
sunox clip restore <clip_id>
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
| `--tags` | Style direction | "pop, synths, upbeat" (≤ 1000 chars) |
| `--exclude` | Styles to avoid | "metal, heavy, dark" (≤ 1000 chars) |
| `--lyrics` / `--lyrics-file` | Custom lyrics with `[Verse]` `[Chorus]` tags | ≤ 5000 chars |
| `--prompt` (describe mode) | Free-text description | ≤ 500 chars |
| `--model` | Model version | v5.5 (default), v5, v4.5+, v4.5, v4 |
| `--vocal` | Vocal gender | male, female |
| `--persona` | Voice persona UUID | from Suno voice creation |
| `--weirdness` | How experimental | 0–100 |
| `--style-influence` | How strictly to follow tags | 0–100 |
| `--instrumental` | No vocals | flag |
| `--token` | Externally supplied challenge token | only when Suno challenges the request |
| `--captcha` | Force browser-backed challenge solver | optional; not the default |
| `--no-captcha` | Do not force the browser-backed solver | challenge preflight still runs |

## Models

| Version | Codename | Notes |
|---|---|---|
| **v5.5** | chirp-fenix | Default, latest, best quality |
| v5 | chirp-crow | Previous gen |
| v4.5+ | chirp-bluejay | Extended capabilities |
| v4.5 | chirp-auk | Stable |
| v4 | chirp-v4 | Legacy |
| v3.5 | chirp-v3-5 | Legacy |
| v3 | chirp-v3-0 | Legacy |
| v2 | chirp-v2-xxl-alpha | Legacy |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

## Agent-friendly output

- Every command supports `--json`. JSON is **auto-detected** when stdout is piped.
- Progress messages and errors go to **stderr** so they don't pollute JSON pipelines.
- Errors include actionable suggestions in the JSON envelope.

```bash
# Pipe-friendly: auto-JSON
sunox clip list | jq '.data[0].title'
sunox clip info <clip_id> --json | jq '.data.audio_url'
```

## Exit codes (semantic)

| Code | Meaning | What the agent should do |
|---|---|---|
| 0 | Success | Continue |
| 1 | Transient (network, web endpoint) | Retry with backoff |
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

### Batch download yesterday's songs as JSON, then pull MP3s

```bash
ids=$(sunox clip list --json | jq -r '.data[].id')
sunox clip download $ids --output ./archive/
```

## Notes

- Auth refreshes automatically (~7-day session lifetime).
- Commands that submit through `/api/generate/v2-web/` preflight `POST /api/c/check` with `ctype=generation`; when no challenge is required, submit uses `token=null` and `token_provider=null`.
- If Suno requires a challenge, prefer `--token <solved>` when available; use `--captcha` only to force the built-in browser-backed solver.
- Generation paths (normal, describe, voice persona, cover, extend, stems) use `/api/generate/v2-web/`; create, cover, extend, and stems expose `--token`, `--captcha`, and `--no-captcha`. Remaster and speed use their current web edit/generation routes. You usually only need the subcommands.
- Persona list/detail/clips/create/set/processed-clip/publish/unpublish/love/unlove/toggle-love/delete/restore/purge are available through `sunox persona ...`.
- Playlist create/list/detail/metadata/add/remove/publish/reorder/save/unsave/like/dislike/restore/delete are available through `sunox playlist ...`; use `playlist set <id> --image-file <path>` for local cover uploads.
- Clip delete/restore and like/dislike are available through `sunox clip delete`, `sunox clip restore`, `sunox clip like`, and `sunox clip dislike`. `--clear` removes the selected reaction.
- `sunox clip upload <file>` uploads local audio through Suno's presigned S3 flow, waits for processing, initializes a clip, and can set title/lyrics metadata.
- `sunox config set <key> <value>` persists local defaults; `SUNO_*` environment variables override persisted config.
- When the CLI returns `schema_drift` (Suno changed its web schema), run `sunox update` to pull the latest binary from GitHub Releases.
- When unsure about flags, run `sunox <command> --help` or `sunox agent-info`.

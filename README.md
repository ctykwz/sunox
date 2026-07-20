# sunox

`sunox` is an unofficial Rust CLI for using Suno from a terminal. It supports song creation,
lyrics, downloads, playlists, personas, covers, remasters, clip edits, stems, and audio uploads.

[![crates.io](https://img.shields.io/crates/v/sunox)](https://crates.io/crates/sunox)
[![CI](https://github.com/ctykwz/sunox/actions/workflows/ci.yml/badge.svg)](https://github.com/ctykwz/sunox/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

English · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) ·
[Français](README.fr.md) · [Español](README.es.md)

> [!WARNING]
> Sunox is not affiliated with or endorsed by Suno. It uses private Suno Web APIs, which may
> change without notice. You are responsible for following Suno's terms, account limits, and the
> rights that apply to any material you generate or upload.

## What it covers

- Create songs from a description, custom lyrics, style tags, a voice persona, or an instrumental
  brief.
- Wait for asynchronous generations and download the resulting MP3, M4A, WAV, Opus, or video.
- Browse, search, edit, publish, trash, restore, and download clips.
- Cover, extend, concatenate, remaster, reverse, crop, fade, change speed, or generate stems.
- Manage playlists and voice personas, and upload local audio or cover images.
- Use table output in a terminal or stable JSON envelopes in scripts and coding agents.

## Install

With Cargo:

```bash
cargo install sunox
```

Rust 1.88 or newer is required.

Prebuilt binaries for macOS, Linux, and Windows are available from
[GitHub Releases](https://github.com/ctykwz/sunox/releases). These binaries are unsigned, so macOS
and Windows may show the usual warning for software downloaded from the internet. Each release
includes `SHA256SUMS`; `sunox update` verifies the archive before installing it.

## Login

Log in to suno.com in a supported browser, then run:

```bash
sunox login
```

Sunox first looks for a reusable session in Chrome, Edge, Brave, Arc, Chromium, or Firefox. If it
cannot reuse one, it opens a separate browser profile for an interactive login.

Credentials are stored in the local Sunox configuration directory. Avoid passing cookies or JWTs
directly on the command line because shell history and process tools may expose them. For a
headless machine, use `--cookie-stdin` or `--jwt-stdin`.

Check the current session with:

```bash
sunox doctor
sunox credits
```

## Create and download a song

A plain description is enough for a first run:

```bash
sunox "warm ambient electronic music with a slow pulse"
```

For custom lyrics and generation controls:

```bash
sunox create \
  --title "Night Drive" \
  --tags "dream pop, synth, female vocal" \
  --exclude "metal, aggressive" \
  --lyrics-file lyrics.txt \
  --weirdness 35 \
  --style-influence 70
```

### Instrumental input modes

Choose one mode; `--instrumental` cannot be combined with `--lyrics` or `--lyrics-file`:

- Use `--instrumental` by itself when you want a no-lyrics instrumental and do not need to control
  its internal sections.
- To control sections, rhythm, edit points, or arrangement, omit `--instrumental` and use a
  structured lyrics file. Put `[Instrumental]` on the first line and keep every remaining
  non-empty line inside square brackets, with no singable body text.

```text
[Instrumental]
[Intro — sparse felt piano, free time]
[Build — strings enter and the pulse accelerates]
[Final cut — hard unresolved ending]
```

After the clips complete, use `sunox clip timed-lyrics <clip_id> --json` as a vocal quality gate.
Reject a generated version if it contains any successful non-empty aligned word.

One generation request normally returns two clip IDs. Wait for them to finish, then download the
ones you want:

```bash
sunox clip wait <clip_id_1> <clip_id_2>
sunox download <clip_id_1> <clip_id_2> --output ./songs
```

The default download is the existing CDN MP3. Sunox writes available plain and timed lyrics into
the file's ID3 tags. Use `--format mp3|m4a|wav|opus` only when you want Suno's format-conversion
workflow, or `--video` for an available MP4.

## Common commands

```text
sunox <prompt>                    Create from a short description
sunox create [prompt]             Create with full generation options
sunox lyrics                      Generate lyrics only

sunox clip list                   List your songs
sunox clip search <query>         Search your songs
sunox clip info <id>              Show clip details
sunox clip wait <ids>             Wait for generation to finish
sunox download <ids>              Download completed clips

sunox clip cover <id>             Create a cover
sunox clip extend <id>            Extend a clip
sunox clip concat <ids>           Join clips into a full song
sunox clip remaster <id>          Remaster a clip
sunox clip speed <id>             Change playback speed
sunox clip reverse <id>           Reverse audio
sunox clip crop <id>              Keep or remove a time range
sunox clip fade <id>              Add a fade
sunox clip stems <id>             Generate stems

sunox playlist list               List playlists
sunox playlist create             Create a playlist
sunox add <clip_ids> --to <id>    Add clips to a playlist

sunox persona list                List voice personas
sunox persona create <clip_id>    Create a persona from a clip

sunox clip upload <file>          Upload local audio
sunox credits                     Show credits and plan information
sunox models                      Show models available to the account
sunox doctor --network            Check DNS, TCP, and HTTPS access
sunox update                      Install the latest GitHub release
```

Run `sunox --help` or `sunox <command> --help` for the complete set of options.

## Generation challenges

Before a generation-backed request, Sunox calls Suno's generation challenge check. When no
challenge is required, it submits directly and does not launch a browser. When Suno requires a
challenge, Sunox first asks the optional Browser Bridge extension to execute the invisible widget
inside an existing `suno.com` tab. If no paired tab responds, the default `auto` mode falls back to
the matching installed Chromium-family browser and closes its temporary profile afterward.

### Install the Browser Bridge on macOS or Windows

The Browser Bridge is bundled with the Sunox binary, so there is no separate ZIP or Chrome Web
Store install. macOS and Windows use the same setup:

1. Extract the bundled extension and note the directory printed by the command:

```bash
sunox install-browser-extension
```

2. In the same Chrome profile where you use Suno, open `chrome://extensions`.
3. Enable **Developer mode**, choose **Load unpacked**, and select the exact directory printed by
   Sunox. On macOS, press `Shift+Command+G` in the folder picker and paste the printed path because
   `~/Library` is hidden by default. On Windows, paste the printed path into the folder picker's
   address bar.
4. Keep the extension enabled, then open or reload an authenticated `https://suno.com` tab.

The extension stays installed across browser restarts. After a Sunox update that changes the
bridge, refresh its files and reload it in Chrome:

```bash
sunox install-browser-extension --force
```

Then click **Reload** on the Sunox Browser Bridge card and reload the Suno tab. The command chooses
the correct per-user application directory on both macOS and Windows; do not move or delete that
directory while Chrome is using the unpacked extension.

The relevant overrides are:

```text
--captcha          Run browser verification even when the preflight says it is unnecessary
--no-captcha       Do not run the automatic browser solver
--token <token>    Submit an externally solved challenge token
```

Set `challenge_browser` to `auto` (default), `existing` (never open a new browser), or `isolated`
(always use the temporary browser). A one-command override looks like
`-c challenge_browser=existing`. In `existing` mode, a missing or stale bridge is reported as an
error instead of opening another browser. In `auto` mode, Sunox may open the isolated fallback when
no connected Suno tab responds.

For unattended runs that must never open a new window, use confirmed Browser Bridge installation
as the command-selection boundary. When it is installed, omit `--no-captcha` and use
`-c challenge_browser=existing`; that mode verifies that a refreshed, authenticated Suno tab is
connected and fails without opening another browser when it is not. If the bridge is not installed
or installation is unknown, keep `--no-captcha`; a required challenge will then stop before
submission. Merely omitting `--no-captcha` in the default `auto` mode still allows the
isolated-browser fallback.

Installing the Bridge is standing permission for Sunox to execute invisible challenges in that
existing tab; it does not require separate permission for every generation. Requests such as “no
popup”, “no new browser”, or “no visible captcha” mean `challenge_browser=existing`, not
`--no-captcha`. Keep `--no-captcha` despite an installed Bridge only when every challenge mechanism,
including the invisible Bridge, is explicitly forbidden or that exact flag is requested.

## JSON output and automation

Every command supports `--json`. Sunox also selects JSON automatically when stdout is piped:

```bash
sunox clip list --json
sunox clip list | jq '.data.clips[0].title'
```

Errors use stable codes and nonzero exit statuses. Partial multi-step operations include completed,
failed, and unattempted items so callers can retry only what is necessary.

For machine-readable command and workflow discovery:

```bash
sunox agent-info --json
```

To install the bundled usage skill for a coding agent:

```bash
sunox install-skill                 # Codex
sunox install-skill --target claude
sunox install-skill --target cursor
```

## Configuration

Show or change persistent settings:

```bash
sunox config show
sunox config set output_dir ./songs
sunox config set default_model auto
sunox config set challenge_browser auto
```

Use `-c key=value` for a one-command override. Environment variables use the `SUNOX_*` prefix,
such as `SUNOX_OUTPUT_DIR`, `SUNOX_DEFAULT_MODEL`, `SUNOX_CHALLENGE_BROWSER`, and
`SUNOX_BROWSER_PATH`.

Write operations are serialized per account by default. `--parallel` disables that protection for
one command; use it only when same-account concurrent writes are intentional.

## Limits and safety

Sunox covers non-Studio workflows that can be verified against the current Suno Web application.
Suno Studio features are intentionally out of scope.

Some commands create paid resources or change remote state. Sunox keeps created clips, playlists,
and personas private unless a command explicitly requests public visibility. Destructive commands
require `-y` or `--yes`.

## Development

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Create changes on a feature branch and open a pull request against `main`.

## License

[MIT](LICENSE)

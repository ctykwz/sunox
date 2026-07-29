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
sunox doctor --browser-bridge     Check Bridge transport without running a challenge
sunox update                      Install the latest GitHub release
```

Run `sunox --help` or `sunox <command> --help` for the complete set of options.

## Generation challenges

Before a generation-backed request, Sunox calls Suno's generation challenge check. When no
challenge is required, it submits directly and does not launch a browser. When Suno requires a
challenge, Sunox first asks the optional Browser Bridge extension to execute the invisible widget
inside the user's regular Chrome profile. The extension keeps only its local listener alive while
idle. For a required challenge, it creates one nonce-bound `suno.com` iframe inside Chrome's
invisible offscreen document. The frame keeps a normal layout viewport for visibility-sensitive
provider code, but Chrome creates no tab, popup, minimized window, or separate browser process.
The frame is credentialless, so it cannot read the user's Suno cookies or persistent browser
storage. At `document_start`, the Bridge stops the host response and replaces it with a minimal
provider-only challenge document. Only the first-level extension-owned frame whose network-visible
query nonce, document nonce, final request headers, and controlled response headers all agree may
connect. Unexpected navigation, caching, reload, disconnect, protocol drift, or identity mismatch
removes the frame and fails closed. The frame is also removed immediately after a token or terminal
error, and there is no visible or credentialed fallback. Raw provider errors never cross the Bridge
boundary or enter storage or logs.
This flow is supported on macOS and Windows. If the Bridge does not
respond, the default `auto` mode falls back to the matching installed Chromium-family browser only
when no Bridge installation has been recorded. Once the Bridge has been installed, `auto` fails closed
instead of launching that separate browser. Use `challenge_browser=isolated` explicitly when the
separate fallback is acceptable.

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
4. Keep the extension enabled. The Bridge creates no Suno tab or browser window.

Verify the Bridge transport without creating a song, running a challenge, or consuming credits:

```bash
sunox doctor --browser-bridge
```

The extension stays installed across browser restarts. Its manifest uses the independent Bridge
runtime build, so a CLI-only release does not change the extracted bundle or require a Chrome
reload. After a Sunox update that actually changes the Bridge, refresh its files:

```bash
sunox install-browser-extension --force
```

The command records activation until Chrome authenticates the exact runtime and pairing. A first
extraction returns `status=installed`, `reload_required=null`, `runtime_ack_pending=true`,
`pending_origin=load_unpacked`, and `activation_required=load_unpacked`: complete **Load unpacked**
and then run `sunox doctor --browser-bridge`. It does not claim the Bridge is ready merely because
the files exist.

When an acknowledged installation changes, the result is `reload_required=true`,
`runtime_ack_pending=true`, and `activation_required=reload`: click **Reload** exactly once and run
doctor. If a never-acknowledged or restored installation has uncertain Chrome state,
`activation_required=ensure_loaded`; `activation_options` contains condition-labelled alternatives
such as `load_unpacked_if_missing` or `enable_and_reload_if_present`. These are mutually exclusive
branches, not steps to run in sequence. An already-current bundle is actively probed; exact
authentication clears the marker and returns `reload_required=false`,
`runtime_ack_pending=false`, and no activation requirement. If it remains unknown, the result is
`reload_required=null` and `runtime_ack_pending=true`; follow its single activation decision rather
than repeatedly clicking Reload.

Doctor distinguishes a missing or repairably corrupt pairing secret and directs one managed
`install-browser-extension --force` repair. Unsafe or inaccessible entries, including symlinks,
non-UTF-8 data, and unreadable paths, fail closed and are not promised to be repairable by force or
Reload. A CLI-only update, computer restart, or Chrome restart never by itself requires
reinstalling or reloading the Bridge. No Suno page reload is needed. The command chooses the
correct per-user application directory on both macOS and Windows; do not move or delete that
directory while Chrome is using the unpacked extension.

The relevant overrides are:

```text
--captcha          Run browser verification even when the preflight says it is unnecessary
--no-captcha       Do not run the automatic browser solver
--token <token>    Submit an externally solved challenge token
```

Set `challenge_browser` to `auto` (default), `existing` (require the Bridge and never launch a
separate browser process), or `isolated` (always use the temporary browser). A one-command override
looks like `-c challenge_browser=existing`. The `existing` name is retained for configuration
compatibility; it now means “use the installed Bridge in the existing Chrome profile.” The Bridge
automatically creates and removes its nonce-bound offscreen iframe, so no user-opened or retained
Suno tab or browser window is created. It accepts only the currently supported Suno/Clerk redirect
shape and does not execute until the return URL has been cleaned and stabilized. A configured
Bridge that is missing, stale, or protocol-drifted is reported as an error instead of opening another browser. In `auto` mode, Sunox may open
the isolated fallback only when no Bridge installation has been recorded. An installed Bridge that is
disabled, stale, or unreachable fails closed; use `isolated` explicitly to allow a separate browser
process.

For unattended runs that must not add a Suno tab to the active browser window or launch a separate
browser process, install the Browser Bridge and omit `--no-captcha`. Both `auto` and
`challenge_browser=existing` then fail closed when the Bridge is unavailable; `existing` additionally
requires the Bridge even when no pairing has been configured. If the Bridge is not installed or
installation is unknown, keep `--no-captcha`; a required challenge will then stop before submission.
Without a configured Bridge, merely omitting `--no-captcha` in the default `auto` mode still allows
the isolated-browser fallback.

Installing the Bridge is standing permission for Sunox to execute challenges in its automatically
managed, short-lived context; it does not require separate permission for every generation.
Requests such as “do not leave a Suno tab open”, “no new browser process”, or “no visible captcha”
allow the installed Bridge and do not mean `--no-captcha`; `challenge_browser=existing` remains the
explicit Bridge-only override. Keep `--no-captcha` despite an installed Bridge only when every
challenge mechanism, including the Bridge, is explicitly forbidden or that exact flag is requested.

## JSON output and automation

Every command supports `--json`. Sunox also selects JSON automatically when stdout is piped:

```bash
sunox clip list --json
sunox clip list | jq '.data.clips[0].title'
```

Commands submitted through `/api/generate/v2-web/`, plus `clip remaster`, preserve Suno's upstream
clips envelope exactly under `.data`; read their submitted clip IDs from `.data.clips[].id`.
`clip concat` preserves its upstream bare Clip object instead, so its ID is `.data.id`. Table output
still uses the parsed clip fields. This avoids materializing omitted upstream fields as synthetic
`null`, `0`, or empty metadata values.

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

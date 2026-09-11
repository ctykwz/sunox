use crate::app::AppContext;
use crate::core::CliError;

pub async fn agent_info(_ctx: &AppContext) -> Result<(), CliError> {
    let auth_path = crate::core::project_config_dir()
        .map(|dir| dir.join("auth.json").display().to_string())
        .unwrap_or_else(|| "~/.config/sunox/auth.json".into());

    let mut info = serde_json::json!({
        "name": "sunox",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Suno AI music generation CLI — direct Suno web workflow",
        "commands": [
            "create", "download", "add", "lyrics", "clip", "persona", "voice", "playlist",
            "credits", "models", "capabilities", "login", "logout", "auth", "config", "doctor", "agent-info",
            "install-skill", "install-browser-extension", "update"
        ],
        "models": {
            "v6": "chirp-hawk",
            "v6-wild": "chirp-hawk-wild",
            "v6-mini": "chirp-goose",
            "v5.5": "chirp-fenix",
            "v5": "chirp-crow",
            "v4.5+": "chirp-bluejay",
            "v4.5-all": "chirp-auk-turbo",
            "v4.5": "chirp-auk",
            "v4": "chirp-v4",
            "v3.5": "chirp-v3-5",
            "v3": "chirp-v3-0",
            "v2": "chirp-v2-xxl-alpha",
        },
        "model_selection": "The default generation selector is v6 Pro chirp-hawk. Model availability, IDs, task capabilities, and max_lengths remain account-specific and every generation or Cover selector requires a successful billing read. default_model=auto selects a usable account default, then usable free default, then first usable model. Explicit selectors resolve by exact external key, exact account model ID, or unambiguous case-insensitive display name; unavailable, unusable, and ambiguous matches fail before submission. --duration accepts whole seconds from 10..360 for v6 Custom generation and retains the exact v5.5 chirp-fenix compatibility path; advertised max_lengths.duration is also enforced. Remaster uses the separate accessible_features and remaster_model_types contract, then preflights source state and action_config; legacy remaster can_use is diagnostic only.",
        "remaster_models": {
            "v6": "chirp-halibut",
            "v5.5": "chirp-flounder",
            "v5": "chirp-carp",
            "v4.5+": "chirp-bass",
        },
        "workflow": {
            "create": "submit generation or description and return clip payload",
            "clip wait": "poll clip ids until complete or error",
            "clip download": "download completed media after the source clip is explicitly unlocked. is_download_unlocked must be exactly true; otherwise normal mode sends POST /api/download/authorize once and read-only mode fails closed. MP3/M4A/WAV and video prefer prepared mp3/m4a/wav/mp4 routes; OPUS and legacy WAV/direct-video paths run only after source unlock. Default prepared MP3 embeds lyrics. Output directories are created automatically; existing files require explicit --force to replace. Downloads have a two-hour total deadline and 2 GiB limit, may be plan-metered, and batch failures return partial_download details.",
            "post_submit_workflow": "When create or a generation-backed edit, including clip inspire, returns new or processing clip IDs, call `sunox clip wait <clip_id> --json` before download, quality filtering, or playlist decisions unless the caller explicitly wants submit-only behavior.",
            "audio_analysis": {
                "simple": "For simple audio analysis, read playback_url and song-page context from `sunox clip info <clip_id> --json`; playback_url preserves a usable top-level audio_url or resolves the current media_urls progressive M4A when Suno returns /api/forbidden. Download only when a local file is needed. Non-auth supplemental read failures appear in supplemental_errors. Do not create new Suno resources just to inspect audio.",
                "deep": "Use heavier WAV or generation-backed stems only when the user explicitly asks for WAV, stems, lossless audio, or deep spectral analysis; do not silently downgrade a WAV/lossless request to MP3."
            },
            "download_formats": {
                "current_cli": "current CLI download gates every file on the source clip's exact is_download_unlocked=true state. When needed, normal mode calls POST /api/download/authorize once per unique source; --read-only never authorizes. --format selects mp3|m4a|wav|opus and --video selects prepared mp4. MP3/M4A/WAV/mp4 are prepared-first; OPUS and legacy WAV/direct-video fallback require an already authorized source. --no-convert refuses the legacy conversion POST.",
                "web_pro_choices": "Suno Web exposes Pro choices for prepared MP3, M4A, WAV, and Video behind one source unlock. `clip stems` starts the current paid Auto Split or Split from Mix generation; `clip get-stems --download` authorizes the parent source at most once and shares that unlock across all existing stems. OPUS is legacy CLI compatibility, not a current Web chooser option.",
                "agent_default": "Use clip info/playback_url when no local file is needed. Keep audio_url as the raw upstream field; /api/forbidden is not media. When a file is requested, use the prepared default MP3; use another format, conversion, stems, or video only when explicitly requested and supported. Download authorization can consume allowance and an ambiguous authorization must never be replayed blindly."
            }
        },
        "execution_policy": {
            "default_mutations": "account-scoped serial execution for Suno create, upload, edit, playlist, persona, and other write commands",
            "config_disable": "set serial_mutations=false with `sunox config set serial_mutations false`, `-c serial_mutations=false`, or SUNOX_SERIAL_MUTATIONS=false to disable the account-scoped mutation lock",
            "native_batch": "commands may still use a Suno endpoint's native batch body when the endpoint is reliable; playlist remove is intentionally one request per clip because large remove batches can return Suno 500s",
            "partial_failures": "serial multi-clip operations preserve the first semantic error and return partial_mutation after earlier successes. Multi-step workflows also include recovery.resumable and, when safe, a structured recovery command and arguments. Never replay a mutation marked resumable=false.",
            "parallel_override": "pass --parallel for a single invocation override; it takes precedence over serial_mutations",
            "agent_parallel_guidance": "Agents should not pass --parallel or disable serial_mutations unless the user explicitly asks to allow same-account concurrent writes."
        },
        "human_commands": [
            "sunox <prompt>",
            "sunox create <prompt>",
            "sunox download <clip_id>",
            "sunox add <clip_id> --to <playlist_id>",
            "sunox login",
            "sunox doctor [--network] [--strict]"
        ],
        "machine_commands": [
            "sunox agent-info --json",
            "sunox capabilities --json",
            "sunox clip list --json",
            "sunox clip list --liked --public --sort popular --json",
            "sunox clip search <query> --all --json",
            "sunox clip info <clip_id> --json",
            "sunox clip actions <clip_id> --json",
            "sunox clip wait <clip_id> --json",
            "sunox clip upload-status <upload_id> --json",
            "sunox clip inspire <clip_id> --title <title> --tags <tags> --lyrics-file <path> --json",
            "sunox clip reuse <clip_id> --json",
            "sunox clip underpaint <clip_id> --json",
            "sunox clip overpaint <clip_id> --json",
            "sunox clip download <clip_id> --json",
            "sunox clip download <clip_id> --format wav --json",
            "sunox clip get-stems <clip_id> --json",
            "sunox clip video-status <clip_id> --json",
            "sunox voice phrase --language en --json",
            "sunox models custom pending --json",
            "sunox lyrics projects list --json",
            "sunox playlist add <playlist_id> <clip_id> --json",
            "sunox persona list --json",
            "sunox doctor --network --json"
        ],
        "agent_integration": {
            "recommended_target": "codex",
            "install_command": "sunox install-skill --target codex",
            "agent_targets": {
                "codex": "~/.codex/skills/sunox/SKILL.md",
                "claude": "~/.claude/skills/sunox/SKILL.md",
                "cursor": "./.cursor/rules/sunox.mdc"
            },
            "contract": [
                "run sunox agent-info for the static CLI contract and authenticated sunox capabilities for current account models, limits, and entitlements",
                "prefer --json for machine-readable command output",
                "when create or a command in async_clip_edits.returns_new_or_processing returns clip IDs, call clip wait before downstream work unless submit-only behavior was requested; crop and fade already wait for their result clip to complete",
                "do not pass --parallel or disable serial_mutations unless the user explicitly opts into same-account concurrent writes",
                "for simple audio analysis, use clip info playback_url; download only when a local file is needed and reserve conversion or generation-backed stems for explicit deep-analysis or lossless requests",
                "do not publish, make public, or run destructive commands unless the user explicitly asked for that action; destructive commands require -y/--yes",
                "use semantic exit codes to decide retry, auth, and config actions"
            ]
        },
        "agent_safety": {
            "parallel_writes": "do not pass --parallel or disable serial_mutations unless the user explicitly asks to allow same-account concurrent writes",
            "read_only": "pass global --read-only for audits and inspections that must not write. It blocks account writes before submission, disables aligned-lyrics augmentation, and permits a download only for an already unlocked source whose is_download_unlocked field is exactly true; it never calls /api/download/authorize",
            "ambiguous_mutation": "generation, download authorization, Remaster, conversion, Voice, Custom Model, lyrics-project, visual, or another submitted-write ambiguity includes an operation ID and recovery details. Download authorization may consume allowance and must never be replayed blindly; inspect exact read-only state and retry only when recovery.resumable=true",
            "single_write_transport": "Suno business writes refresh authentication before submission, send at most once, never follow redirects, and map transport loss, 3xx, 5xx, or an unreadable accepted response to ambiguous_mutation; only read/validation requests use auth retry",
            "paid_or_credit_work": "create, reuse, inspire, cover, extend, underpaint, overpaint, stems, remaster, speed, reverse, crop, fade, upload, Voice creation, Custom Model training, AI image/video generation, conversion, and prepared download/export workflows can be stateful, credit-sensitive, or plan-metered; only run the amount, operation, and format the user requested",
            "download_quality": "current CLI uses prepared MP3 by default, supports prepared M4A/WAV and video MP4, and retains OPUS only as unlocked legacy compatibility; agents should request a file/format only when needed",
            "public_visibility": "do not publish clips, playlists, or personas or make them public unless the user explicitly asks",
            "persona_create_visibility": "persona create is private by default and requires explicit --public to create a public persona",
            "destructive_actions": "do not run delete, trash, purge, empty-trash, or other destructive commands unless the user explicitly asks. clip purge and clip empty-trash are irreversible and require -y/--yes.",
            "captcha": "for unattended generation that must not show a Suno tab or browser window, use confirmed Browser Bridge installation as the command-selection boundary: when installed, omit --no-captcha; both challenge_browser=auto and challenge_browser=existing use a nonce-bound Suno iframe inside Chrome's invisible offscreen document. The frame is removed on every terminal path. Turnstile rebuilds its widget exactly once only when the first 15-second silent attempt produces no callback; classified provider callbacks never start another fresh widget, while Turnstile may still perform its bounded same-widget recovery. Requests for visible interaction fail immediately. It never creates a tab, popup, minimized window, or separate browser process, and it fails closed instead of falling back to any visible context. Challenge-page and provider failures cross extension boundaries only as allowlisted error codes mapped to fixed sanitized messages; raw Clerk return values and provider errors are never forwarded. Confirmed installation is standing permission for challenges in that managed context and does not require per-run captcha permission. Interpret no retained Suno tab, no new browser process, or no visible captcha as allowing the Bridge, not as --no-captcha. Use --no-captcha with an installed bridge only when the user explicitly forbids every challenge mechanism including the Browser Bridge, or explicitly requests that flag. When the bridge is absent or installation is unknown, keep --no-captcha so a required challenge stops before submission. The default challenge_browser=auto may open an isolated browser only when no Bridge installation has been recorded; a recorded installation with a missing pairing secret still fails closed. Use challenge_browser=isolated explicitly when a separate challenge browser is acceptable. Do not force --captcha unless the user explicitly asks, and prefer an externally supplied --token when provided",
            "structured_instrumental": "choose exactly one input mode: use --instrumental without --lyrics/--lyrics-file for an unconstrained no-lyrics instrumental; for controlled sections, rhythm, or arrangement, omit --instrumental and send a custom lyrics file whose first line is [Instrumental] and whose remaining non-empty lines are bracketed directions. After clip wait, inspect clip timed-lyrics --json and reject a generated version from downstream selection if any successful non-empty aligned word is present; never delete it without explicit authorization",
            "secrets": "never print, persist in project files, or include auth cookies, Clerk values, JWTs, or challenge tokens in prompts, logs, README examples, or commits; prefer auth --cookie-stdin or --jwt-stdin over argv"
        },
        "command_notes": {
            "create": {
                "default_challenge": "preflights POST /api/c/check with ctype=generation; if Suno reports a challenge and stored Clerk refresh material exists, refreshes the JWT once and repeats the preflight; when still required, challenge_browser=auto first asks the installed Sunox Browser Bridge to create one nonce-bound Suno iframe inside Chrome's invisible offscreen document. The frame executes the silent challenge without creating a tab, popup, minimized window, or separate browser process and is removed on every terminal path. Only allowlisted error codes cross extension boundaries; raw provider values are never forwarded. A recorded but unavailable Bridge installation fails closed, including when its pairing secret is missing; auto falls back to an isolated matching browser only when no Bridge installation has been recorded. It then submits provider 1 for hCaptcha or provider 2 for Cloudflare Turnstile; when no challenge is required, token and token_provider are serialized as null to match the current Web client",
                "challenge_flags": {
                    "--token": "use an externally supplied solved challenge token; preflight infers token_provider from the reported captcha_version and falls back to provider 1 only when a non-auth preflight error prevents detection",
                    "--captcha": "force browser-backed challenge verification even when preflight says it is unnecessary",
                    "--no-captcha": "disable automatic browser verification; generation challenge preflight still runs and a required challenge is surfaced without submitting"
                },
                "modes": "description mode when a non-instrumental prompt is provided; custom mode when --lyrics, --lyrics-file, --mumble, or --instrumental is provided. Bracket-only [Instrumental] structure is custom lyrics; unconstrained --instrumental folds the prompt into style tags. --instrumental conflicts with --lyrics and --lyrics-file, and description mode rejects Custom-only --variety/--max-mode instead of silently discarding them",
                "structured_instrumental_quality_gate": "for controlled sections, rhythm, or arrangement, omit --instrumental and pass a file beginning with [Instrumental] whose remaining non-empty lines are all bracketed directions. After clip wait, call clip timed-lyrics <clip_id> --json; any successful non-empty aligned word rejects that generated version from downstream use",
                "request_contract": "custom lyrics use prompt with metadata.create_mode=custom, omit gpt_description_prompt, and encode --vocal as metadata.vocal_gender=m|f; description mode uses gpt_description_prompt with metadata.create_mode=simple, metadata.lyrics_model=default, and leaves prompt empty. v6 Custom duration defaults to 180 seconds and accepts whole seconds from 10..360; description mode rejects --duration because a live request accepted 10 seconds but produced clips around 73 and 80 seconds. --variety maps the whole-number range 0..4 directly to metadata.control_sliders.aug_creativity and requires v6 plus session flag aug-creativity; gated defaults are 0 for chirp-hawk-wild and 1 for other v6 models, and are omitted without the gate. --mumble sends metadata.is_mumble=true and requires both the live model's mumble_mode feature and session flag mumble-mode. --max-mode sends metadata.is_max_mode=true and requires the account max_mode entitlement, session flag max-mode, and a supported model; with Max Mode, requested duration is not a guarantee of final clip length",
                "persona_contract": "--persona follows the current Advanced Persona picker contract: Sunox reads GET /api/persona/get-persona/{id}/, rejects hidden or trashed Personas, normalizes an empty/zero root_clip_id to no source, uses only a valid root clip as artist_clip_id, and sends task=vox for Vox or task=artist_consistency for legacy. Rootless Vox sends persona_id with artist_start_s=0 and no artist_clip_id/artist_end_s; legacy without a usable root fails closed. A sourced Vox selected with an older Persona-capable model falls back to artist_consistency; rootless Vox requires a Vox-capable model. Root-backed references use 0..root clip duration and never substitute detail.clip.id or vocal_clip_id from normal Persona selection",
                "web_context": "generation metadata.user_tier and the configured model selector are resolved against current account /api/billing/info/. The built-in selector is chirp-hawk (v6 Pro); default_model=auto instead prefers a usable is_default_model, then usable is_default_free_model, then the first model whose can_use field is true. Every selector requires that successful billing read and fails closed before challenge or generation when unavailable; no unvalidated compiled-in fallback is submitted",
                "enhance_tags": "pass --enhance-tags only when the user wants Suno to enhance style tags; Sunox first verifies that the resolved model has the custom badge or tag_upsample feature, reads /api/personalization/settings so metadata.last_tags_generation.personalization_enabled matches styles_augmentation (missing defaults true), then calls /api/prompts/upsample with current custom lyrics as context for vocal requests, validates the returned tags against the model length limit, carries the returned tags plus request_id into metadata.last_tags_generation, and marks override_fields=[\"tags\"]",
                "response_derived_metadata": "do not fabricate tag-upsample metadata; metadata.last_tags_generation is only valid after a real /api/prompts/upsample response and should otherwise be omitted",
                "title": "optional; omitted title is sent as an empty string for description mode because Suno currently requires params.title to be a string"
            },
            "clip upload": {
                "status": "user-facing CLI workflow is available",
                "workflow": "open and validate the local file, create a presigned upload, stream the file to S3 with transfer-specific timeouts, finish upload, poll processing, initialize clip, then set title/lyrics/cover metadata when available; metadata-changing uploads poll until the requested fields are visible",
                "status_command": "sunox clip upload-status <upload_id> --json performs a read-only status check and never replays an upload mutation",
                "partial_failure": "after an upload_id exists, failures return partial_mutation with upload_id, optional clip_id, completed_steps, failed.step/code/message, and recovery. Follow recovery only when resumable=true"
            },
            "image upload": {
                "workflow": "create an image upload, submit the presigned S3 form, finish moderation, then apply the approved image to a clip or playlist",
                "partial_failure": "after an upload_id exists, image transfer, finish, moderation, and later clip/playlist cover failures return partial_mutation with a complete cover reference, completed steps, and recovery. Unverified mutation replays are marked resumable=false"
            },
            "clip list": {
                "route": "POST /api/feed/v3",
                "filters": "--public, --liked, --upload, --trashed, --cover, and --extend map to the current web feed filters; --sort popular maps to sortBy=upvote_count, sortDirection=desc",
                "scope": "query-only listing; this is not a library sync or local mirror workflow"
            },
            "clip purge": {
                "route": "POST /api/clips/delete/",
                "constraints": "permanently deletes specific clips that are already in trash in serial batches of 20; requires explicit -y/--yes and cannot be undone. A first-batch failure preserves its original semantic error; a later failure returns partial_mutation with purged_clip_ids, failed.clip_ids/code/message, and not_attempted_clip_ids."
            },
            "clip empty-trash": {
                "route": "POST /api/feed/v3 with only filters.trashed=True, then POST /api/clips/delete/",
                "constraints": "paginates every clip currently in trash and permanently deletes them in serial batches; requires explicit -y/--yes and cannot be undone. A first-batch failure preserves its original semantic error. A later failure returns partial_mutation with purged_clip_ids, failed.clip_ids/code/message, and not_attempted_clip_ids."
            },
            "clip info": {
                "routes": [
                    "GET /api/clip/<clip_id>",
                    "GET /api/clips/{clip_id}/attribution",
                    "GET /api/gen/{clip_id}/comments?order=most_liked",
                    "GET /api/clips/remixes/count?clip_id=<clip_id>",
                    "GET /api/clips/get_similar/?id=<clip_id>"
                ],
                "json_shape": "main clip fields remain top-level and raw audio_url is preserved; playback_url selects a usable audio_url or the current media_urls progressive M4A when audio_url is /api/forbidden; attribution, comments, remix_count={count,is_capped,...}, and similar_clips are added as semantic song-page context; if a non-auth, non-rate-limit supplemental read fails, the base clip is still returned with supplemental_errors; auth and rate-limit errors still abort normally"
            },
            "clip remaster": {
                "route": "POST /api/generate/upsample",
                "body": {
                    "clip_id": "<source clip id>",
                    "model_name": "chirp-halibut|chirp-flounder|chirp-carp|chirp-bass",
                    "variation_category": "subtle|normal|high for chirp-halibut/chirp-flounder/chirp-carp; defaults to normal; omitted for chirp-bass",
                    "v6_optional": "chirp-halibut only: style_profile=natural|boost|clarity; defaults to boost. Lower-level tags and slider fields are intentionally not exposed because the current v2 modal omits them and live checks did not prove ordinary-account support"
                },
                "defaults": "chirp-halibut sends variation=normal and style_profile=boost; chirp-flounder/chirp-carp send variation=normal; chirp-bass rejects explicit --variation and omits the field",
                "eligibility": "requires accessible_features.remaster, a model present in remaster_model_types, and an exact complete, explicitly non-trashed, non-infill source no longer than 960 seconds whose action_config remaster action has visible=true and disabled=false. Legacy model can_use remains diagnostic only.",
                "response": "generation response with submitted clips"
            },
            "clip download": {
                "authorization": "only exact is_download_unlocked=true skips authorization; otherwise POST /api/download/authorize with item_id=<source clip id> and item_type=clip once before any file GET. The response preserves ok, reason, message, and credit_deducted. ok!=true stops; transport ambiguity is read back and never blindly replayed; --read-only fails closed instead of authorizing",
                "route": "GET /api/download/clip/{clip_id}?format=mp3|m4a|wav|mp4 is prepared-first. Legacy GET/convert WAV, direct clip.video_url, and OPUS GET/convert are compatibility paths permitted only after the source unlock gate",
                "defaults": "without --format, uses the official prepared MP3 endpoint and embeds lyrics into ID3 tags; --format selects mp3|m4a|wav|opus",
                "constraints": "--video prefers prepared mp4 and cannot be combined with --format. OPUS is legacy compatibility; --no-convert refuses a missing conversion. Output is preserved unless --force is explicit. Authorization and downloads may be plan-metered.",
                "billing": "credits and capabilities expose optional download_usage.current_period_downloads_limit, current_period_downloads_used, additional_download_remaining, and download_credit_packs from live billing. Runtime never hard-codes quota by plan name",
                "timed_lyrics": "normal mode may POST then poll aligned_lyrics/v3 before v2 compatibility fallback; --read-only only reads an existing alignment and never starts augmentation"
            },
            "clip stems": {
                "route": "POST /api/generate/v2-web/",
                "status": "current generation-backed Pro stem separation; Auto Split is the default and --mode split requires one canonical --stem target",
                "body_constraints": "task=gen_stem, mv=chirp-v3-0, make_instrumental=true, stem_type_id=91; Auto uses group=Twelve/task=twelve, while Pro Split from Mix uses the selected current group/task=extract/canonical stem_name",
                "response": "generation response with multiple chirp-stem clips"
            },
            "clip extend": {
                "route": "GET /api/clip/<clip_id>, optional POST /api/feed/v3 metadata enrichment, then POST /api/generate/v2-web/",
                "defaults": "fetches the source clip through the current single-clip route before submit; only when it lacks source style metadata, searches feed/v3 by source.title and merges the exact source id; title defaults to source.title, tags defaults to source.metadata.tags, negative_tags defaults to source.metadata.negative_tags when available, and make_instrumental defaults to source.metadata.make_instrumental",
                "overrides": "--title overrides the submitted title; --tags overrides inherited style tags; --exclude overrides inherited negative_tags; --instrumental forces make_instrumental=true; --no-instrumental forces make_instrumental=false",
                "body_constraints": "task=upload_extend when the source metadata.type is upload, otherwise task=extend; metadata.create_mode=custom, metadata.is_remix=true, metadata.lyrics_updated reflects whether new lyrics were supplied, mv resolves from configured/auto account models that support the task and extend condition, continue_clip_id=<source clip id>, continue_at=<seconds>, continued_aligned_prompt=<source context or empty string>, title must be a string",
                "response": "generation response with submitted continuation clips"
            },
            "clip cover": {
                "route": "GET /api/clip/<clip_id>, then POST /api/generate/v2-web/",
                "defaults": "fetches the source clip before submit and always sends title as source.title because Suno requires a string title for the cover generation variant",
                "body_constraints": "task=cover, generation_type=SIMPLE_REMIX, metadata.create_mode=simple, metadata.is_remix=true, cover_clip_id=<source clip id>, title=<source title string>; account base-model availability is validated before v3/v3.5 map to chirp-v3-5-tau and v4 maps to chirp-v4-tau; v4.5-all keeps the explicit chirp-auk-turbo Web override",
                "response": "generation response with submitted cover clips"
            },
            "clip reuse": {
                "route": "GET /api/clip/<clip_id>, then POST /api/generate/v2-web/",
                "semantics": "reuse_styles_lyrics is a Web feature and UI condition, never a submitted generation task; Sunox expands source metadata.prompt/tags/negative_tags/title into a normal Custom request",
                "overrides": "explicit --lyrics/--lyrics-file, --tags, --exclude, and --title win over source values; source must be complete and expose lyrics or styles metadata",
                "model_gate": "the selected live model must explicitly list reuse_styles_lyrics in models.features; a generic custom badge is not sufficient"
            },
            "clip underpaint/overpaint": {
                "route": "GET /api/billing/info/, GET /api/clip/<clip_id>, live model preparation, then POST /api/generate/v2-web/",
                "body_constraints": "underpaint sends task=underpainting plus underpainting_clip_id; overpaint sends task=overpainting plus overpainting_clip_id; both send metadata.is_remix=true and no time range",
                "safety": "requires live edit_mode entitlement, exact Suno user-ID claim ownership, complete and explicitly non-trashed source, current Web source eligibility, and matching live underpaint/overpaint model condition",
                "eligibility": "underpaint accepts upload or Vocals/Backing_Vocals stems; overpaint accepts upload, Instrumental stems, empty lyrics, or a single bracket-only prompt"
            },
            "clip inspire": {
                "route": "POST /api/generate/v2-web/; with explicit --enhance-tags, first POST /api/prompts/upsample",
                "status": "implemented from the live-captured playlist-conditioned Use as Inspiration request",
                "constraints": "accepts exactly one source clip; requires --title, --tags, and --lyrics or --lyrics-file; optional --audio-influence is 0..100; --enhance-tags invokes the current Web editor's optional style-enhance action; does not expose instrumental or multi-source variants because those were not captured",
                "body_constraints": "task=playlist_condition, mv resolves from configured/auto account models that support playlist_condition plus the playlist condition, playlist_id=inspiration, playlist_clip_ids=[<source clip id>], metadata.create_mode=custom, optional --audio-influence is normalized into metadata.control_sliders.audio_weight, lyrics are sent in prompt, no gpt_description_prompt, and override_fields=[]; only --enhance-tags sends lyrics as upsample context and carries the returned tags/request_id in metadata.last_tags_generation",
                "response": "generation response with submitted clips"
            },
            "clip concat": {
                "route": "POST /api/generate/concat/v2/",
                "input_constraint": "use a source with original Suno generation history. A live July 10 validation accepted metadata.type=gen and completed; an edit_fade result was rejected by Suno with Bad history.",
                "response": "queued or processing clip; wait for the returned ID before downstream work"
            },
            "challenge_capable_generation_commands": {
                "commands": ["create", "clip cover", "clip reuse", "clip inspire", "clip extend", "clip underpaint", "clip overpaint", "clip stems"],
                "create_description_mode": "sunox create <description> is a mode of the create command; there is no standalone describe subcommand",
                "challenge_flags": "only these commands expose --token, --captcha, and --no-captcha because they submit through /api/generate/v2-web/ and can hit the generation challenge gate"
            },
            "async_clip_edits": {
                "returns_new_or_processing": ["clip cover", "clip reuse", "clip inspire", "clip extend", "clip underpaint", "clip overpaint", "clip concat", "clip stems", "clip remaster", "clip speed", "clip reverse"],
                "waits_for_complete": ["clip crop", "clip fade"],
                "post_submit_workflow": "commands in returns_new_or_processing require clip wait before downstream work; clip crop and clip fade already wait for the resulting clip to complete and do not require another wait after success",
                "challenge_note": "clip cover, clip reuse, clip inspire, clip extend, clip underpaint, clip overpaint, and clip stems expose challenge flags; clip concat, clip remaster, clip speed, clip reverse, clip crop, and clip fade use their own edit routes and do not expose --token, --captcha, or --no-captcha"
            },
            "clip speed": {
                "route": "POST /api/clips/adjust-speed/",
                "body": {
                    "clip_id": "<source clip id>",
                    "speed_multiplier": "positive finite number",
                    "keep_pitch": true,
                    "title": "<new clip title>"
                },
                "response": "processing clip"
            },
            "clip reverse": {
                "route": "POST /api/clips/reverse-clip/",
                "body": {
                    "clip_id": "<source clip id>",
                    "title": "<new clip title>"
                },
                "response": "new clip"
            },
            "clip crop": {
                "route": "POST /api/edit/crop/{clip_id}/ then GET /api/edit/action/{action_clip_id}/",
                "body": {
                    "crop_start_s": "finite seconds, >= 0",
                    "crop_end_s": "finite seconds, greater than crop_start_s",
                    "is_crop_remove": "false for trim-to-section, true for remove-section",
                    "title": "<new clip title>",
                    "ui_surface": "song_actions"
                },
                "response": "poll action_clip_id, then fetch completed clip; polling uses config poll_timeout_secs and poll_interval_secs"
            },
            "clip fade": {
                "route": "POST /api/edit/fade/{clip_id}/ then poll GET /api/edit/action/{action_clip_id}/",
                "body": {
                    "fade_in_time": "optional finite nonnegative seconds",
                    "fade_out_time": "optional finite nonnegative seconds",
                    "title": "<new clip title>"
                },
                "response": "poll the edit action to complete, then fetch the completed clip; polling uses config poll_timeout_secs and poll_interval_secs"
            }
        },
        "features": [
            "account_capabilities", "read_only", "clip_actions", "generation_duration",
            "tags", "enhance_tags", "negative_tags", "vocal_gender",
            "weirdness", "style_influence", "variety", "mumble_mode", "max_mode", "audio_influence",
            "instrumental", "extend", "concat", "cover", "clip_inspiration", "remaster",
            "stems", "existing_stem_banks", "clip_speed", "clip_reverse", "clip_crop", "clip_fade",
            "download_formats", "download_no_convert", "lyrics", "timed_lyrics", "set_metadata",
            "set_visibility", "search", "delete", "clip_restore", "clip_purge", "clip_trash_query",
            "clip_like", "clip_dislike", "optional_captcha_solver", "audio_upload", "audio_upload_status",
            "id3_lyrics_embedding", "clip_list_filters", "voice_persona", "persona_list",
            "persona_info", "persona_clips", "persona_create",
            "persona_set_metadata",
            "persona_set_visibility", "persona_love",
            "persona_unlove", "persona_toggle_love", "persona_delete",
            "persona_restore", "persona_purge",
            "playlist_list", "playlist_info", "playlist_create", "playlist_set_metadata",
            "playlist_set_visibility", "playlist_reorder_tracks",
            "playlist_add_tracks", "playlist_remove_tracks",
            "playlist_save", "playlist_unsave",
            "playlist_like", "playlist_dislike",
            "playlist_restore", "playlist_delete", "playlist_cover_upload",
            "image_upload", "clip_generated_image", "clip_video_status", "clip_info",
            "cover_art_models", "cover_art_cost", "cover_art_image_batch",
            "cover_art_video_batch", "cover_art_pending", "cover_art_history",
            "cover_art_poll", "cover_art_apply_image", "cover_art_apply_video",
            "voice_phrase", "voice_verification", "voice_create_private",
            "custom_model_pending", "custom_model_train", "custom_model_archive",
            "lyrics_projects", "lyrics_rewrite", "lyrics_mashup", "lyrics_project_link"
        ],
        "unsupported_surfaces": {
            "video_upload": {
                "status": "bundle_discovered_unverified",
                "reason": "video upload paths are visible in the frontend bundle but are not exposed as CLI workflows"
            },
            "update_feedback_state": {
                "status": "bundle_discovered_unverified",
                "reason": "clip feedback-state mutation is visible in the bundle and intentionally not exposed"
            },
            "legacy_video_generation_submit": {
                "status": "blocked_unconfirmed_clip_eligibility",
                "reason": "the legacy POST/status routes are confirmed, but no exact current ownership/download eligibility seam is available; video-status remains read-only"
            },
            "studio_multitrack_export": {
                "status": "out_of_scope",
                "reason": "Studio functionality is outside this CLI's scope"
            }
        },
        "config": {
            "set": "sunox config set <key> <value> persists to config.toml",
            "env_override": "SUNOX_* environment variables override persisted config values",
            "challenge_browser": "auto prefers the installed Browser Bridge, which creates one nonce-bound Suno iframe inside Chrome's invisible offscreen document, executes a silent challenge, and removes the frame on every terminal path. It creates no tab, popup, minimized window, or separate browser process. Only allowlisted error codes cross extension boundaries and are mapped to fixed sanitized messages; raw provider values are never forwarded. Auto fails closed when a recorded Bridge installation is unavailable or its pairing secret is missing and falls back to an isolated browser only when no Bridge installation has been recorded; existing is the compatibility name for always requiring the Bridge-managed offscreen frame; isolated always uses the temporary browser",
            "browser_bridge_update": "the extension manifest is versioned by the independent Browser Bridge runtime, so CLI-only releases do not change its bundle. install-browser-extension keeps runtime_ack_pending=true until the exact runtime and pairing authenticate. A first install returns reload_required=null, pending_origin=load_unpacked, activation_required=load_unpacked; complete Load unpacked and run doctor. A normal acknowledged update returns reload_required=true and activation_required=reload. Uncertain or restored state returns the single decision activation_required=ensure_loaded; activation_options are mutually exclusive condition-labelled branches, never a sequence. Exact acknowledgement returns reload_required=false,runtime_ack_pending=false. Doctor sends missing or repairably corrupt pairing values through one managed --force repair, while unsafe or inaccessible secret entries fail closed and are not claimed repairable by force or Reload.",
            "keys": ["default_model", "poll_interval_secs", "poll_timeout_secs", "output_dir", "serial_mutations", "challenge_browser"]
        },
        "resource_management": {
            "clip": {
                "commands": [
                    "clip list", "clip search", "clip info", "clip actions", "clip status", "clip wait",
                    "clip download", "clip upload", "clip upload-status", "clip delete", "clip restore", "clip purge", "clip empty-trash",
                    "clip like", "clip dislike", "clip set", "clip publish",
                    "clip timed-lyrics", "clip extend", "clip concat", "clip reuse",
                    "clip underpaint", "clip overpaint",
                    "clip cover", "clip inspire", "clip remaster", "clip speed", "clip reverse",
                    "clip crop", "clip fade", "clip stems", "clip get-stems",
                    "clip generate-image", "clip generate-video", "clip video-status",
                    "clip cover-art models", "clip cover-art pending", "clip cover-art history",
                    "clip cover-art image", "clip cover-art video", "clip cover-art status",
                    "clip cover-art apply-image", "clip cover-art apply-video"
                ],
                "cover_status": "clip set supports --image-url, --image-file, --remove-cover, and --remove-video-cover; local image files use POST /api/uploads/image/, presigned S3 form upload, POST /api/uploads/image/{id}/upload-finish/, then POST /api/gen/{clip_id}/set_metadata/ with image_s3_id; arbitrary external cover URLs use image_url"
            },
            "persona": {
                "commands": [
                    "persona list", "persona info", "persona clips", "persona create",
                    "persona set",
                    "persona publish", "persona unpublish",
                    "persona love", "persona unlove", "persona toggle-love",
                    "persona delete", "persona restore", "persona purge", "voice phrase", "voice create"
                ],
                "clips_status": "implemented via GET /api/persona/get-persona-paginated/{id}/?page=N",
                "edit_status": "implemented via PUT /api/persona/edit-persona/{id}/",
                "visibility_status": "implemented via PUT /api/persona/set_visibility/{id}/?is_public=true|false",
                "trash_status": "implemented via PUT /api/persona/trash-persona/{id}/?undo=false&hide=false",
                "restore_status": "implemented via PUT /api/persona/trash-persona/{id}/?undo=true&hide=false",
                "purge_status": "implemented via PUT /api/persona/trash-persona/{id}/?undo=false&hide=true"
            },
            "playlist": {
                "commands": [
                    "playlist list", "playlist info", "playlist create",
                    "playlist set", "playlist add", "playlist remove",
                    "playlist publish", "playlist reorder", "playlist restore",
                    "playlist save", "playlist unsave",
                    "playlist like", "playlist dislike",
                    "playlist delete"
                ],
                "metadata_status": "playlist set uses PATCH /api/playlist/v2/{id} with metadata.name and bio.description as the primary contract; arbitrary external image URLs alone retain the legacy set_metadata compatibility route because v2 requires an uploaded S3 cover id",
                "cover_status": "playlist set/create support --image-file for local image upload; uploaded covers use POST /api/uploads/image/, presigned S3 form upload, POST /api/uploads/image/{id}/upload-finish/, then PATCH /api/playlist/v2/{id} with only metadata.cover_image_s3_id",
                "cover_url_status": "playlist set --image-url accepts existing Suno uploaded image URLs such as https://cdn2.suno.ai/image_<upload_id>.jpeg and extracts the upload identity for the same cover_image_s3_id-only v2 patch; arbitrary external URLs still use the legacy set_metadata route",
                "info_json_shape": "playlist info keeps normalized top-level fields for compatibility and also preserves the complete metadata, relationship, and stats objects from the v2 response; unknown top-level response fields remain under extra",
                "multi_step_failure": "playlist create/set expose completed_steps, playlist_id, and failed.step/code/message through partial_mutation when an earlier server mutation succeeded",
                "remove_status": "playlist remove accepts multiple clip IDs but submits one POST /api/playlist/v2/{playlist_id}/tracks/remove request per clip ID because larger batch remove requests can return Suno 500s. If a later item fails, the command returns partial_mutation with error.details containing requested_clip_ids, succeeded_clip_ids, failed, and not_attempted_clip_ids."
            },
            "persona_mutation": {
                "route": "PUT /api/persona/trash-persona/{persona_id}/ with undo/hide query parameters",
                "partial_failure": "multi-persona trash, restore, and purge run through the current per-ID endpoint; a first failure preserves its semantic error, while a later failure returns partial_mutation with succeeded_persona_ids, failed, and not_attempted_persona_ids"
            }
        },
        "exit_codes": {
            "0": "success",
            "1": "runtime, web endpoint, partial or ambiguous mutation, or partial download error; inspect error.code, error.details, and recovery.resumable before retrying",
            "2": "configuration error — check config",
            "3": "auth error — run `sunox login`",
            "4": "rate limited — wait and retry",
            "5": "not found — verify resource ID",
            "130": "interrupted — operation cancelled and staging files cleaned up"
        },
        "env_prefix": "SUNOX_",
        "auth_path": auth_path,
        "auth": {
            "recommended": "sunox login",
            "methods": [
                "browser_cookie_extract",
                "interactive_browser_login",
                "full_cookie_header",
                "raw_clerk_client_cookie",
                "direct_jwt",
                "cookie_stdin",
                "jwt_stdin",
                "stored_clerk_refresh",
            ],
            "login_fallback": "`sunox login` first probes existing browser cookies; if that fails, it opens a dedicated Sunox Chromium-family profile and captures the Clerk session after the user logs in. Windows skips live Chromium cookie databases so App-Bound decryption cannot force-close a running browser, while Firefox uses a non-destructive read-only SQLite path. The interactive fallback requires an installed Chromium-family browser.",
            "logout": "`sunox logout` removes stored auth, the dedicated interactive browser profile, and any legacy captcha profile",
            "generation_challenge": "Commands that submit through /api/generate/v2-web/ preflight POST /api/c/check with ctype=generation. If Suno reports a challenge and stored Clerk refresh material exists, Sunox refreshes the JWT once and repeats the preflight. If a challenge remains, challenge_browser=auto first asks the installed Sunox Browser Bridge to create one nonce-bound Suno iframe inside Chrome's invisible offscreen document using hCaptcha/provider 1 or Cloudflare Turnstile/provider 2 according to captcha_version. The frame executes silently and is removed on every terminal path; it creates no tab, popup, minimized window, or separate browser process. Challenge failures cross extension boundaries only as allowlisted error codes mapped to fixed sanitized messages. A recorded but unavailable Bridge installation fails closed, including when its pairing secret is missing; auto falls back to an invocation-owned isolated browser only when no Bridge installation has been recorded. Use challenge_browser=isolated explicitly to allow that separate browser. Use --token <solved> for an external token, --captcha to force verification, or --no-captcha to disable automatic browser verification.",
            "browser_environment": "Browser-cookie login links auth to the matching local profile and probes the same installed browser binary for runtime user-agent, accept-language, client hints, and matching device identity without a visible window or Suno navigation. Legacy auth is repaired before authenticated commands. Fresh values win per field, stored values survive failed probes, and built-in constants are only the final UA/language fallback. Device-Id is recovered from the same account when possible and otherwise omitted rather than fabricated. The recovered context is used for Clerk login/JWT refresh and Suno API requests.",
        },
        "provider": "direct_suno_unofficial",
        "auth_required": true,
        "default_model": "chirp-hawk (v6 Pro; requires a successful billing read and exact can_use validation; override with --model, config, or SUNOX_DEFAULT_MODEL)",
    });
    info["generation_json_contract"] = serde_json::json!({
        "preserves_exact_upstream_response": true,
        "clips_envelope": {
            "commands": ["create", "clip cover", "clip reuse", "clip inspire", "clip extend", "clip underpaint", "clip overpaint", "clip stems", "clip remaster"],
            "data_path": ".data",
            "submitted_clip_id_path": ".data.clips[].id"
        },
        "bare_clip": {
            "commands": ["clip concat"],
            "data_path": ".data",
            "submitted_clip_id_path": ".data.id"
        },
        "table_output": "uses parsed clip fields"
    });
    info["command_notes"]["lyrics"] = serde_json::json!({
        "route": "current-confirmed GET /api/generate/cowrite-lyrics/models/ followed by POST /api/generate/cowrite-lyrics/",
        "mode": "apply_user_request",
        "request_contract": "current model discovery exposes id, display_name, family, and supports_thinking; --model resolves by ID or display name, otherwise the Web literal default is used; --thinking is rejected unless supported. The current Web submit body sends selected, context_before, context_after, instruction, title, style, mode, references, num_variants, lyricist_id, metadata.lyrics_model, metadata.enable_thinking, create_session_token, and lyrics_project_id; fresh CLI lyrics use empty selection/context/title/style and references, with the user prompt as instruction; no submit/status polling is used",
        "response_contract": "current bundle and a 2026-08-23 account submit confirm edited_lyrics, lyrics_request_id, lyrics_id, variants, artist_to_tag_mapping, next_prompts, plus preserved unknown response fields"
    });
    info["command_notes"]["clip get-stems"] = serde_json::json!({
        "routes": ["GET /api/clip/{clip_id}/stems/pages", "GET /api/clip/{clip_id}/stems?page={zero_based_page}"],
        "default": "read every existing stem bank without starting a new extraction",
        "hydration": "page rows are ID references and are hydrated through exact clip reads; missing IDs remain explicit and make --download fail before any file is written",
        "download": "--download gates every stem on the parent source clip, authorizes that parent at most once, and shares the unlock across all stem files. Stem MP3s explicitly skip aligned-lyrics generation; prepared formats may be plan-metered, OPUS is legacy compatibility, and global --read-only works only when the parent is already unlocked"
    });
    info["command_notes"]["voice"] = serde_json::json!({
        "read_only": ["voice phrase", "voice processed-status", "voice verification-status"],
        "create": "requires the active persona plan feature, refetches and matches the exact current phrase ID, uploads a pre-trimmed singing sample and dynamic-phrase recording as voice_recording assets, processes both through /api/processed_clip/voice-vox-stem, verifies ownership through /api/voice-verification/, creates a private vox Persona, then polls read-only detail until is_public=false and the vox type converge",
        "input_boundary": "current Web accepts a source from 3 seconds; a source below 10 seconds is selected in full, while longer selection is 10..240 seconds. The CLI does not silently re-encode: --sample must be a valid WAV already trimmed to exactly the rounded --sample-duration. Verification is a valid WAV and the Web recorder targets about 15 seconds. Name/styles/description use current HTML UTF-16 limits 80/256/2000. The CLI does not capture microphone audio itself",
        "polling": "processed audio is capped at 1s x 120 and verification at 1.5s x 40; user polling configuration may shorten but cannot expand those current Web budgets",
        "generation": "the resulting Voice is managed through persona commands and selected with create --persona; the selected live account model must advertise the current vox capability and condition combination",
        "safety": "--confirm-rights, --confirm-eligibility, and --confirm-biometric-consent are separate mandatory attestations. Suno Terms/Privacy, disclosed training use and account choices, Statsig gates, and server eligibility remain authoritative. Every server ID is atomically checkpointed for inspection, but the multi-write workflow has no generic safe resume command and no mutation POST is automatically replayed after an uncertain response"
    });
    info["command_notes"]["models custom"] = serde_json::json!({
        "routes": ["POST /api/custom-model/create/", "GET /api/custom-model/pending/", "POST /api/custom-model/archive/"],
        "train": "requires 6 to 100 distinct complete, explicitly non-trashed owned-source clip IDs, a non-empty name of at most 16 Unicode characters, --confirm-rights, the live custom_models entitlement, and --confirm-ui-available after visibly confirming that the current Suno Web account exposes training; without either attestation no training POST is sent, and server eligibility and charging remain authoritative",
        "ready_models": "ready Custom Models are account billing model rows and are selected through create --model; pending training rows use models custom pending",
        "archive": "archive is destructive from the CLI user's perspective, requires -y/--yes, and is not described as permanent deletion or recoverable; accepted archive state is verified with bounded pending/billing GETs without replaying POST"
    });
    info["command_notes"]["lyrics projects"] = serde_json::json!({
        "routes": ["GET/POST /api/lyrics-projects", "GET/PATCH/DELETE /api/lyrics-projects/{id}", "POST /api/lyrics-projects/{id}/flush"],
        "commands": ["list", "info", "create", "rename", "flush", "delete"],
        "song_link": "create --lyrics-project-id <id> is valid only with explicit custom lyrics; it GET-validates the exact project identity before generation and sends the unchanged ID",
        "safety": "delete requires -y/--yes; single-project reads require the exact requested ID and required title/lyrics fields; write responses are verified through stable project reads where the current protocol permits"
    });
    info["command_notes"]["lyrics rewrite"] = serde_json::json!({
        "route": "POST /api/generate/lyrics-infill/",
        "body": ["prompt", "context_lyrics_prefix", "context_lyrics_edit", "context_lyrics_suffix", "create_session_token", "title"],
        "boundary": "one synchronous Lyrics 2.0 selection rewrite with a 30-second request timeout; transport ambiguity is never replayed automatically"
    });
    info["command_notes"]["lyrics mashup"] = serde_json::json!({
        "routes": ["POST /api/generate/lyrics-mashup", "GET /api/generate/lyrics/{mashup_id}"],
        "body": "lyrics_a, lyrics_b, create_session_token, source=create_ui",
        "wait": "polling uses a 2.5-second interval and a configurable deadline (150 seconds by default); --no-wait returns IDs, and mashup-status is a read-only status/recovery command whose --timeout requires --wait",
        "safety": "a submit that returned an ID is reported as a partial mutation if later observation fails; the read-only status command preserves ordinary read errors and never claims it submitted the job"
    });
    info["command_notes"]["clip generate-image"] = serde_json::json!({
        "route": "POST /api/gen/prompt_image/ with {prompt}, then POST /api/gen/{clip_id}/set_metadata/ with the returned image_url",
        "scope": "this is the direct prompt-image composition; use clip cover-art for the separate multi-result image/video batch workflow",
        "safety": "requires exact authenticated ownership, the account feature, explicit non-trashed state, and an enabled generate_cover_art source action; the applied URL is read back from the clip"
    });
    info["command_notes"]["clip cover-art"] = serde_json::json!({
        "routes": ["GET /api/video_gen/model-configs", "POST /api/video_gen/cost/image", "POST /api/video_gen/cost/video", "POST /api/video_gen/image/generate", "POST /api/video_gen/video/generate", "POST /api/video_gen/pending_batches", "POST /api/video_gen/history", "POST /api/video_gen/poll_batches", "POST /api/gen/{clip_id}/set_metadata/"],
        "workflow": "models/pending/history/status are inspection surfaces; image/video submit two-result batches after dynamic model and cost preflight; apply-image/apply-video require the batch ID, prove that the exact completed result belongs to the selected clip, and perform clip readback",
        "safety": "all writes require the exact Suno user-ID claim to match clip.user_id, explicit non-trashed state, enabled generate_cover_art action, and the matching plan feature; generation never auto-applies a result; a lost submit or apply response is never replayed because the protocol has no client idempotency key"
    });
    info["command_notes"]["clip generate-video"] = serde_json::json!({
        "routes": ["POST /api/video/generate/{clip_id}/ with no body", "GET /api/video/generate/{clip_id}/status/"],
        "status_command": "clip video-status is read-only; add --wait for a bounded poll without replaying submit",
        "safety": "the route is known but no exact current response seam proves per-clip ownership and download eligibility; generate-video therefore fails closed before POST and does not guess an action name"
    });
    if let Some(models) = info["models"].as_object_mut() {
        models.insert(
            "_source".into(),
            serde_json::json!("known aliases only; use authenticated sunox capabilities for the current account model list and selectors"),
        );
    }
    if let Some(models) = info["remaster_models"].as_object_mut() {
        models.insert(
            "_source".into(),
            serde_json::json!("known aliases only; use authenticated sunox capabilities for the current account remaster list"),
        );
    }
    info["protocol_safety"] = serde_json::json!({
        "live_account_command": "sunox capabilities --json",
        "clip_action_preflight": "sunox clip actions <clip_id> --json",
        "read_only": "global --read-only rejects account writes before the first write request; downloads require an already unlocked source and never POST authorization",
        "model_selectors": "display name, external key, or account model ID; ambiguity and unusable models fail closed",
        "download_policy": "strict is_download_unlocked=true skips POST /api/download/authorize; otherwise authorize one unique source once. MP3/M4A/WAV/mp4 are prepared-first, OPUS and other legacy fallbacks require source unlock, and Stems reuse the parent source authorization",
        "download_billing": "read current_period_downloads_limit, current_period_downloads_used, additional_download_remaining, and download_credit_packs from live billing; never hard-code quota by plan name",
        "mutation_transport": "Suno business writes use a no-redirect client and are never replayed after 401; transport loss, 3xx, 5xx, and unreadable accepted bodies are ambiguous",
        "mutation_uncertainty": "ambiguous_mutation means the write may have succeeded; inspect operation_id and exact readback before any retry, and retry only when recovery.resumable=true"
    });
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

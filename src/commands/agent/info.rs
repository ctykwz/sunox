use crate::app::AppContext;
use crate::core::CliError;

pub async fn agent_info(_ctx: &AppContext) -> Result<(), CliError> {
    let auth_path = crate::core::project_config_dir()
        .map(|dir| dir.join("auth.json").display().to_string())
        .unwrap_or_else(|| "~/.config/sunox/auth.json".into());

    let info = serde_json::json!({
        "name": "sunox",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Suno AI music generation CLI — direct Suno web workflow",
        "commands": [
            "create", "download", "add", "lyrics", "clip", "persona", "playlist",
            "credits", "models", "login", "logout", "auth", "config", "doctor", "agent-info",
            "install-skill", "update"
        ],
        "models": {
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
        "model_selection": "Model availability, the account default, and max_lengths are account-specific. Generation reads `/api/billing/info/` directly; `sunox models --json` returns generation and remaster arrays from the same account data. default_model=auto selects the account's usable default, while an explicit --model or configured model is validated against account capability data. v5.5 is used only when the billing read itself is unavailable; a successful empty model response is an error.",
        "remaster_models": {
            "v5.5": "chirp-flounder",
            "v5": "chirp-carp",
            "v4.5+": "chirp-bass",
        },
        "workflow": {
            "create": "submit generation or description and return clip payload",
            "clip wait": "poll clip ids until complete or error",
            "clip download": "download completed media; default CDN MP3 embeds lyrics; explicit --format supports mp3|m4a|wav|opus and --video. Output directories are created automatically; existing files require explicit --force to replace. Downloads have a two-hour total deadline and 2 GiB limit. Non-auth timed-lyrics failures preserve available plain lyrics and appear in the success envelope's warnings; auth/rate-limit errors abort. Batch failures return partial_download details.",
            "post_submit_workflow": "When create or a generation-backed edit, including clip inspire, returns new or processing clip IDs, call `sunox clip wait <clip_id> --json` before download, quality filtering, or playlist decisions unless the caller explicitly wants submit-only behavior.",
            "audio_analysis": {
                "simple": "For simple audio analysis, use existing clip media: read audio_url and song-page context from `sunox clip info <clip_id> --json` or run `sunox clip download <clip_id> --json` for the default CDN MP3; non-auth supplemental read failures appear in supplemental_errors. Do not create new Suno resources just to inspect audio.",
                "deep": "Use heavier WAV or generation-backed stems only when the user explicitly asks for WAV, stems, lossless audio, or deep spectral analysis; do not silently downgrade a WAV/lossless request to MP3."
            },
            "download_formats": {
                "current_cli": "current CLI download uses clip.audio_url for the default CDN MP3 path; explicit --format mp3|m4a|wav|opus uses Suno's official download endpoints; --video uses clip.video_url when present. Download preparation and edit polling use poll_timeout_secs and poll_interval_secs from config, including in-flight requests and auth retries.",
                "web_pro_choices": "Suno Web exposes Pro download choices such as WAV Audio, Get Stems, and Video. This CLI supports explicit WAV download via --format wav, but `sunox clip stems` is generation-backed stems extraction and is not the same as Suno Web Pro Get Stems export.",
                "agent_default": "Use the default CDN MP3 for routine listening, preview, transcription, and lightweight analysis. Use --format mp3|m4a|wav|opus, stems, or video only when explicitly requested and supported."
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
            "sunox clip list --json",
            "sunox clip list --liked --public --sort popular --json",
            "sunox clip search <query> --all --json",
            "sunox clip info <clip_id> --json",
            "sunox clip wait <clip_id> --json",
            "sunox clip upload-status <upload_id> --json",
            "sunox clip inspire <clip_id> --title <title> --tags <tags> --lyrics-file <path> --json",
            "sunox clip download <clip_id> --json",
            "sunox clip download <clip_id> --format wav --json",
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
                "run sunox agent-info for current capabilities",
                "prefer --json for machine-readable command output",
                "when create or a command in async_clip_edits.returns_new_or_processing returns clip IDs, call clip wait before downstream work unless submit-only behavior was requested; crop and fade already wait for their result clip to complete",
                "do not pass --parallel or disable serial_mutations unless the user explicitly opts into same-account concurrent writes",
                "for simple audio analysis, use existing clip audio_url or the default CDN download; reserve explicit --format or generation-backed stems for explicit format, deep-analysis, or lossless requests",
                "do not publish, make public, or run destructive commands unless the user explicitly asked for that action; destructive commands require -y/--yes",
                "use semantic exit codes to decide retry, auth, and config actions"
            ]
        },
        "agent_safety": {
            "parallel_writes": "do not pass --parallel or disable serial_mutations unless the user explicitly asks to allow same-account concurrent writes",
            "paid_or_credit_work": "create, inspire, cover, extend, stems, remaster, speed, reverse, crop, fade, upload, and explicit non-default download/export workflows can be stateful or credit/plan-sensitive; only run the amount, operation, and format the user requested",
            "download_quality": "current CLI download defaults to CDN MP3 and supports explicit --format mp3|m4a|wav|opus; agents should use an explicit format only when requested",
            "public_visibility": "do not publish clips, playlists, or personas or make them public unless the user explicitly asks",
            "persona_create_visibility": "persona create is private by default and requires explicit --public to create a public persona",
            "destructive_actions": "do not run delete, trash, purge, empty-trash, or other destructive commands unless the user explicitly asks. clip purge and clip empty-trash are irreversible and require -y/--yes.",
            "captcha": "do not force --captcha unless the user asks for the browser-backed solver; prefer normal challenge preflight and externally supplied --token when provided",
            "secrets": "never print, persist in project files, or include auth cookies, Clerk values, JWTs, or challenge tokens in prompts, logs, README examples, or commits; prefer auth --cookie-stdin or --jwt-stdin over argv"
        },
        "command_notes": {
            "create": {
                "default_challenge": "preflights POST /api/c/check with ctype=generation; if Suno reports a challenge and stored Clerk refresh material exists, refreshes the JWT once and repeats the preflight before surfacing the challenge; submits with token=null and token_provider=null only when no challenge is required; does not run the browser solver unless --captcha is supplied",
                "challenge_flags": {
                    "--token": "use an externally supplied solved challenge token; submit body uses token_provider=1",
                    "--captcha": "force the browser-backed challenge solver; submit body uses token_provider=1 when a token is produced",
                    "--no-captcha": "do not force the browser-backed solver; generation challenge preflight still runs"
                },
                "modes": "description mode when a non-instrumental prompt is provided; custom lyrics mode when --lyrics or --lyrics-file is provided; custom instrumental mode when --instrumental is provided, with the prompt folded into style tags",
                "web_context": "generation metadata.user_tier and default model are resolved from current account /api/billing/info/ when available; default_model=auto falls back to chirp-fenix only when that read is unavailable",
                "enhance_tags": "pass --enhance-tags only when the user wants Suno to enhance style tags; it first calls /api/prompts/upsample, carries the returned tags plus request_id into metadata.last_tags_generation, and marks override_fields=[\"tags\"]; personalization_enabled follows the captured web submit shape",
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
                    "GET /api/feed/?ids=<clip_id>",
                    "GET /api/clips/{clip_id}/attribution",
                    "GET /api/gen/{clip_id}/comments?order=most_liked",
                    "GET /api/clips/direct_children_count?clip_id=<clip_id>",
                    "GET /api/clips/get_similar/?id=<clip_id>"
                ],
                "json_shape": "main clip fields remain top-level; attribution, comments, direct_children_count, and similar_clips are added as semantic song-page context; if a non-auth, non-rate-limit supplemental read fails, the base clip is still returned with supplemental_errors; auth and rate-limit errors still abort normally"
            },
            "clip remaster": {
                "route": "POST /api/generate/upsample",
                "body": {
                    "clip_id": "<source clip id>",
                    "model_name": "chirp-flounder|chirp-carp|chirp-bass",
                    "variation_category": "subtle|normal|high"
                },
                "defaults": "--variation defaults to normal; use subtle to preserve more of the source or high for the strongest variation",
                "response": "generation response with submitted clips"
            },
            "clip download": {
                "route": "GET /api/download/clip/{clip_id}?format=mp3|m4a for prepared MP3/M4A; POST /api/gen/{clip_id}/convert_wav/ then GET /api/gen/{clip_id}/wav_file/ for WAV; GET/POST /api/gen/{clip_id}/opus_file|convert_opus for OPUS",
                "defaults": "without --format, downloads clip.audio_url as MP3 and embeds lyrics into ID3 tags; explicit --format mp3|m4a|wav|opus uses the official endpoint for that format",
                "constraints": "--video uses clip.video_url and cannot be combined with --format. WAV and OPUS preparation are serialized as account-scoped mutations; OPUS still reuses an existing file URL without requesting conversion. Output directories are created automatically; existing output is preserved unless --force is explicit."
            },
            "clip stems": {
                "route": "POST /api/generate/v2-web/",
                "status": "generation-backed stems extraction; not the same as Suno Web Pro Get Stems export",
                "body_constraints": "task=gen_stem, mv=chirp-v3-0, make_instrumental=true, stem_type_id=91, stem_type_group_name=Twelve, stem_task=twelve",
                "response": "generation response with multiple chirp-stem clips"
            },
            "clip extend": {
                "route": "GET /api/feed/?ids=<clip_id>, optional POST /api/feed/v3 metadata fallback, then POST /api/generate/v2-web/",
                "defaults": "fetches the source clip before submit; when feed/?ids lacks source style metadata, searches feed/v3 by source.title and merges the exact source id; title defaults to source.title, tags defaults to source.metadata.tags, negative_tags defaults to source.metadata.negative_tags when available, and make_instrumental defaults to source.metadata.make_instrumental",
                "overrides": "--title overrides the submitted title; --tags overrides inherited style tags; --exclude overrides inherited negative_tags; --instrumental forces make_instrumental=true; --no-instrumental forces make_instrumental=false",
                "body_constraints": "task=extend, metadata.create_mode=custom, metadata.is_remix=true, metadata.lyrics_updated=true, mv=chirp-fenix, continue_clip_id=<source clip id>, continue_at=<seconds>, continued_aligned_prompt=<source context or empty string>, title must be a string",
                "response": "generation response with submitted continuation clips"
            },
            "clip cover": {
                "route": "GET /api/feed/?ids=<clip_id>, then POST /api/generate/v2-web/",
                "defaults": "fetches the source clip before submit and always sends title as source.title because Suno requires a string title for the cover generation variant",
                "body_constraints": "metadata.create_mode=cover, cover_clip_id=<source clip id>, title=<source title string>",
                "response": "generation response with submitted cover clips"
            },
            "clip inspire": {
                "route": "POST /api/prompts/upsample, then POST /api/generate/v2-web/",
                "status": "implemented from the live-captured playlist-conditioned Use as Inspiration request",
                "constraints": "accepts exactly one source clip; requires --title, --tags, and --lyrics or --lyrics-file; does not expose instrumental or multi-source variants because those were not captured",
                "body_constraints": "task=playlist_condition, mv=chirp-fenix, playlist_id=inspiration, playlist_clip_ids=[<source clip id>], metadata.create_mode=custom, lyrics in prompt, no gpt_description_prompt, upsample response carried in metadata.last_tags_generation, override_fields=[]",
                "response": "generation response with submitted clips"
            },
            "clip concat": {
                "route": "POST /api/generate/concat/v2/",
                "input_constraint": "use a source with original Suno generation history. A live July 10 validation accepted metadata.type=gen and completed; an edit_fade result was rejected by Suno with Bad history.",
                "response": "queued or processing clip; wait for the returned ID before downstream work"
            },
            "challenge_capable_generation_commands": {
                "commands": ["create", "clip cover", "clip inspire", "clip extend", "clip stems"],
                "create_description_mode": "sunox create <description> is a mode of the create command; there is no standalone describe subcommand",
                "challenge_flags": "only these commands expose --token, --captcha, and --no-captcha because they submit through /api/generate/v2-web/ and can hit the generation challenge gate"
            },
            "async_clip_edits": {
                "returns_new_or_processing": ["clip cover", "clip inspire", "clip extend", "clip concat", "clip stems", "clip remaster", "clip speed", "clip reverse"],
                "waits_for_complete": ["clip crop", "clip fade"],
                "post_submit_workflow": "commands in returns_new_or_processing require clip wait before downstream work; clip crop and clip fade already wait for the resulting clip to complete and do not require another wait after success",
                "challenge_note": "only clip cover, clip inspire, clip extend, and clip stems expose challenge flags; clip concat, clip remaster, clip speed, clip reverse, clip crop, and clip fade use their own edit routes and do not expose --token, --captcha, or --no-captcha"
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
            "tags", "enhance_tags", "negative_tags", "vocal_gender",
            "weirdness", "style_influence",
            "instrumental", "extend", "concat", "cover", "clip_inspiration", "remaster",
            "stems", "clip_speed", "clip_reverse", "clip_crop", "clip_fade",
            "download_formats", "lyrics", "timed_lyrics", "set_metadata",
            "set_visibility", "search", "delete", "clip_restore", "clip_purge", "clip_trash_query",
            "clip_like", "clip_dislike", "optional_captcha_solver", "audio_upload", "audio_upload_status",
            "id3_lyrics_embedding", "clip_list_filters", "voice_persona", "persona_list",
            "persona_info", "persona_clips", "persona_create",
            "persona_set_metadata", "persona_processed_clip",
            "persona_set_visibility", "persona_love",
            "persona_unlove", "persona_toggle_love", "persona_delete",
            "persona_restore", "persona_purge",
            "playlist_list", "playlist_info", "playlist_create", "playlist_set_metadata",
            "playlist_set_visibility", "playlist_reorder_tracks",
            "playlist_add_tracks", "playlist_remove_tracks",
            "playlist_save", "playlist_unsave",
            "playlist_like", "playlist_dislike",
            "playlist_restore", "playlist_delete", "playlist_cover_upload",
            "image_upload", "clip_info"
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
            "social_profile_project_video": {
                "status": "bundle_discovered_unverified",
                "reason": "profile, social, project, and video surfaces are outside the current music creation/resource-management scope"
            },
            "voice_verification": {
                "status": "stale_or_flow_specific",
                "reason": "older captures include voice verification paths, but the refreshed non-Studio bundle did not confirm them"
            },
            "studio_multitrack_export": {
                "status": "out_of_scope",
                "reason": "Studio functionality is outside this CLI's scope"
            }
        },
        "config": {
            "set": "sunox config set <key> <value> persists to config.toml",
            "env_override": "SUNOX_* environment variables override persisted config values",
            "keys": ["default_model", "poll_interval_secs", "poll_timeout_secs", "output_dir", "serial_mutations"]
        },
        "resource_management": {
            "clip": {
                "commands": [
                    "clip list", "clip search", "clip info", "clip status", "clip wait",
                    "clip download", "clip upload", "clip upload-status", "clip delete", "clip restore", "clip purge", "clip empty-trash",
                    "clip like", "clip dislike", "clip set", "clip publish",
                    "clip timed-lyrics", "clip extend", "clip concat",
                    "clip cover", "clip inspire", "clip remaster", "clip speed", "clip reverse",
                    "clip crop", "clip fade", "clip stems"
                ],
                "cover_status": "clip set supports --image-url, --image-file, --remove-cover, and --remove-video-cover; local image files use POST /api/uploads/image/, presigned S3 form upload, POST /api/uploads/image/{id}/upload-finish/, then POST /api/gen/{clip_id}/set_metadata/ with image_url"
            },
            "persona": {
                "commands": [
                    "persona list", "persona info", "persona clips", "persona create",
                    "persona set", "persona processed-clip",
                    "persona publish", "persona unpublish",
                    "persona love", "persona unlove", "persona toggle-love",
                    "persona delete", "persona restore", "persona purge"
                ],
                "clips_status": "implemented via GET /api/persona/get-persona-paginated/{id}/?page=N",
                "edit_status": "implemented via PUT /api/persona/edit-persona/{id}/",
                "processed_clip_status": "implemented via GET /api/processed_clip/{id}",
                "visibility_status": "implemented via PUT /api/persona/set_visibility/{id}/?is_public=true|false",
                "trash_status": "implemented via PUT /api/persona/bulk-trash-personas/ with undo=false, hide=false",
                "restore_status": "implemented via PUT /api/persona/bulk-trash-personas/ with undo=true, hide=false",
                "purge_status": "implemented via PUT /api/persona/bulk-trash-personas/ with undo=false, hide=true"
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
                "cover_status": "playlist set/create support --image-file for local image upload; uploaded covers use POST /api/uploads/image/, presigned S3 form upload, POST /api/uploads/image/{id}/upload-finish/, then PATCH /api/playlist/v2/{id} with metadata.cover_url, metadata.cover_image_s3_id, and metadata.cover_is_user_set=true",
                "cover_url_status": "playlist set --image-url accepts existing Suno uploaded image URLs such as https://cdn2.suno.ai/image_<upload_id>.jpeg and maps them to the same v2 cover metadata patch; arbitrary external URLs still use the legacy set_metadata route",
                "info_json_shape": "playlist info keeps normalized top-level fields for compatibility and also preserves the complete metadata, relationship, and stats objects from the v2 response; unknown top-level response fields remain under extra",
                "multi_step_failure": "playlist create/set expose completed_steps, playlist_id, and failed.step/code/message through partial_mutation when an earlier server mutation succeeded",
                "remove_status": "playlist remove accepts multiple clip IDs but submits one POST /api/playlist/v2/{playlist_id}/tracks/remove request per clip ID because larger batch remove requests can return Suno 500s. If a later item fails, the command returns partial_mutation with error.details containing requested_clip_ids, succeeded_clip_ids, failed, and not_attempted_clip_ids."
            }
        },
        "exit_codes": {
            "0": "success",
            "1": "runtime, web endpoint, partial mutation or partial download error; inspect error.code and error.details before retrying",
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
            "generation_challenge": "Commands that submit through /api/generate/v2-web/ preflight POST /api/c/check with ctype=generation. If Suno reports a challenge and stored Clerk refresh material exists, Sunox refreshes the JWT once and repeats the preflight before surfacing the challenge. If no challenge is required, submit uses token=null/token_provider=null. Use --token <solved> to supply a token or --captcha to force the browser-backed solver; the solver launches an invocation-owned browser on a random loopback CDP port and deletes its temporary profile after use.",
            "browser_environment": "Browser-cookie login links auth to the matching local profile and probes the same installed browser binary for runtime user-agent, accept-language, and client hints without a visible window or Suno navigation. Legacy auth is repaired before authenticated commands. Fresh values win per field, stored values survive failed probes, and built-in constants are only the final fallback. The recovered context is used for Clerk login/JWT refresh and Suno API requests.",
        },
        "provider": "direct_suno_unofficial",
        "auth_required": true,
        "default_model": "auto (account usable default; chirp-fenix fallback only when billing info is unavailable)",
    });
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

use crate::app::AppContext;
use crate::core::CliError;

pub async fn agent_info(_ctx: &AppContext) -> Result<(), CliError> {
    let auth_path = directories::ProjectDirs::from("com", "sunox", "sunox")
        .map(|d| d.config_dir().join("auth.json").display().to_string())
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
            "v4.5": "chirp-auk",
            "v4": "chirp-v4",
            "v3.5": "chirp-v3-5",
            "v3": "chirp-v3-0",
            "v2": "chirp-v2-xxl-alpha",
        },
        "remaster_models": {
            "v5.5": "chirp-flounder",
            "v5": "chirp-carp",
            "v4.5+": "chirp-bass",
        },
        "workflow": {
            "create": "submit generation or description and return clip payload",
            "clip wait": "poll clip ids until complete or error",
            "clip download": "download completed media and embed MP3 lyrics",
            "post_submit_workflow": "When create or a generation-backed edit returns new or processing clip IDs, call `sunox clip wait <clip_id> --json` before download, quality filtering, or playlist decisions unless the caller explicitly wants submit-only behavior.",
            "audio_analysis": {
                "simple": "For simple audio analysis, use existing clip media: read audio_url and song-page context from `sunox clip info <clip_id> --json` or run `sunox clip download <clip_id> --json`; non-auth supplemental read failures appear in supplemental_errors. Do not create new Suno resources just to inspect audio.",
                "deep": "Use heavier WAV, stems, or Studio export workflows only when the user explicitly asks for WAV, stems, lossless audio, or deep spectral analysis; do not silently downgrade a WAV/lossless request to MP3."
            },
            "download_formats": {
                "current_cli": "current CLI download supports MP3 audio from clip.audio_url and `--video` from clip.video_url when present",
                "web_pro_choices": "Suno Web exposes Pro download choices such as WAV Audio, Get Stems, and Video; do not assume they are available through this CLI unless agent-info reports a command for them. `sunox clip stems` is generation-backed stems extraction and is not the same as Suno Web Pro Get Stems export.",
                "agent_default": "Use MP3/audio_url for routine listening, preview, transcription, and lightweight analysis. Use Pro/WAV/stems/video paths only when explicitly requested and supported."
            }
        },
        "execution_policy": {
            "default_mutations": "account-scoped serial execution for Suno create, upload, edit, playlist, persona, and other write commands",
            "config_disable": "set serial_mutations=false with `sunox config set serial_mutations false`, `-c serial_mutations=false`, or SUNO_SERIAL_MUTATIONS=false to disable the account-scoped mutation lock",
            "native_batch": "commands may still use a Suno endpoint's native batch body when the endpoint is reliable; playlist remove is intentionally one request per clip because large remove batches can return Suno 500s",
            "parallel_override": "pass --parallel for a single invocation override; it takes precedence over serial_mutations",
            "agent_parallel_guidance": "Agents should not pass --parallel or disable serial_mutations unless the user explicitly asks to allow same-account concurrent writes."
        },
        "human_commands": [
            "sunox <prompt>",
            "sunox create <prompt>",
            "sunox download <clip_id>",
            "sunox add <clip_id> --to <playlist_id>",
            "sunox login",
            "sunox doctor"
        ],
        "machine_commands": [
            "sunox agent-info --json",
            "sunox clip list --json",
            "sunox clip list --liked --public --sort popular --json",
            "sunox clip info <clip_id> --json",
            "sunox clip wait <clip_id> --json",
            "sunox clip download <clip_id> --json",
            "sunox playlist add <playlist_id> <clip_id> --json",
            "sunox persona list --json",
            "sunox config show --json"
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
                "after any command returns new clip IDs, call clip wait before download, filtering, or playlist decisions unless submit-only behavior was requested",
                "do not pass --parallel or disable serial_mutations unless the user explicitly opts into same-account concurrent writes",
                "for simple audio analysis, use existing clip audio_url or clip download; reserve WAV, stems, or Studio export workflows for explicit deep-analysis or lossless requests",
                "do not publish, make public, or run destructive commands unless the user explicitly asked for that action; destructive commands require -y/--yes",
                "use semantic exit codes to decide retry, auth, and config actions"
            ]
        },
        "agent_safety": {
            "parallel_writes": "do not pass --parallel or disable serial_mutations unless the user explicitly asks to allow same-account concurrent writes",
            "paid_or_credit_work": "create, cover, extend, stems, remaster, speed, upload, and Pro download/export workflows can be stateful or credit/plan-sensitive; only run the amount and format the user requested",
            "download_quality": "current CLI download supports MP3 by default; Suno Web exposes Pro download choices including WAV Audio, Get Stems, and Video, but agents should only use supported Pro/export commands when explicitly requested",
            "public_visibility": "do not publish clips, playlists, or personas or make them public unless the user explicitly asks",
            "destructive_actions": "do not run delete, trash, purge, or other destructive commands unless the user explicitly asks; when explicitly requested, pass -y/--yes because destructive commands require it",
            "captcha": "do not force --captcha unless the user asks for the browser-backed solver; prefer normal challenge preflight and externally supplied --token when provided",
            "secrets": "never print, persist in project files, or include auth cookies, Clerk values, JWTs, or challenge tokens in prompts, logs, README examples, or commits"
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
                "web_context": "generation metadata.user_tier is filled from current account /api/billing/info/ plan.id when available, with an empty fallback if that read is unavailable",
                "enhance_tags": "pass --enhance-tags only when the user wants Suno to enhance style tags; it first calls /api/prompts/upsample, carries the returned tags plus request_id into metadata.last_tags_generation, and marks override_fields=[\"tags\"]; personalization_enabled follows the captured web submit shape",
                "response_derived_metadata": "do not fabricate tag-upsample metadata; metadata.last_tags_generation is only valid after a real /api/prompts/upsample response and should otherwise be omitted",
                "title": "optional; omitted title is sent as an empty string for description mode because Suno currently requires params.title to be a string"
            },
            "clip upload": {
                "status": "user-facing CLI workflow is available",
                "workflow": "create presigned upload, post local bytes to S3 form, finish upload, poll processing, initialize clip, then set title/lyrics/cover metadata when available"
            },
            "clip list": {
                "route": "POST /api/feed/v3",
                "filters": "--public, --liked, --upload, --cover, and --extend map to the current web feed filters; --sort popular maps to sortBy=upvote_count, sortDirection=desc",
                "scope": "query-only listing; this is not a library sync or local mirror workflow"
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
                    "variation_category": "normal"
                },
                "response": "generation response with submitted clips"
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
            "challenge_capable_generation_commands": {
                "commands": ["create", "describe", "clip cover", "clip extend", "clip stems"],
                "challenge_flags": "only these commands expose --token, --captcha, and --no-captcha because they submit through /api/generate/v2-web/ and can hit the generation challenge gate"
            },
            "async_clip_edits": {
                "commands": ["clip cover", "clip extend", "clip concat", "clip stems", "clip remaster", "clip speed"],
                "post_submit_workflow": "these commands can return new or processing clip IDs; wait for returned clip IDs before downstream download, filtering, or playlist mutation",
                "challenge_note": "only clip cover, clip extend, and clip stems expose challenge flags; clip concat, clip remaster, and clip speed use their own edit routes and do not expose --token, --captcha, or --no-captcha"
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
            }
        },
        "features": [
            "tags", "enhance_tags", "negative_tags", "vocal_gender",
            "weirdness", "style_influence",
            "instrumental", "extend", "concat", "cover", "remaster",
            "stems", "clip_speed", "lyrics", "timed_lyrics", "set_metadata",
            "set_visibility", "search", "delete", "clip_restore",
            "clip_like", "clip_dislike", "optional_captcha_solver", "audio_upload",
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
            "playlist_condition_generation": {
                "status": "live_captured_not_exposed",
                "route": "POST /api/generate/v2-web/",
                "reason": "captured task=playlist_condition uses playlist_id=inspiration and playlist_clip_ids, but it is a separate inspiration/remix surface from normal create"
            },
            "fade_edit": {
                "status": "live_captured_not_exposed",
                "route": "POST /api/edit/fade/{clip_id}/",
                "reason": "captured flow returns action_clip_id and requires polling /api/edit/action/{action_clip_id}/"
            },
            "studio_multitrack_export": {
                "status": "live_captured_not_exposed",
                "route": "POST /api/studio/render-state-multitrack",
                "reason": "stem zip export requires full Studio arrangement state plus rights-key handling, not just clip download"
            }
        },
        "config": {
            "set": "sunox config set <key> <value> persists to config.toml",
            "env_override": "SUNO_* environment variables override persisted config values",
            "keys": ["default_model", "poll_interval_secs", "poll_timeout_secs", "output_dir", "serial_mutations"]
        },
        "resource_management": {
            "clip": {
                "commands": [
                    "clip list", "clip search", "clip info", "clip status", "clip wait",
                    "clip download", "clip upload", "clip delete", "clip restore",
                    "clip like", "clip dislike", "clip set", "clip publish",
                    "clip timed-lyrics", "clip extend", "clip concat",
                    "clip cover", "clip remaster", "clip speed", "clip stems"
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
                "remove_status": "playlist remove accepts multiple clip IDs but submits one POST /api/playlist/v2/{playlist_id}/tracks/remove request per clip ID because larger batch remove requests can return Suno 500s. If a later item fails, the command returns partial_mutation with error.details containing requested_clip_ids, succeeded_clip_ids, failed, and not_attempted_clip_ids."
            }
        },
        "exit_codes": {
            "0": "success",
            "1": "runtime, web endpoint, or partial mutation error; inspect error.code and error.details before retrying",
            "2": "configuration error — check config",
            "3": "auth error — run `sunox login`",
            "4": "rate limited — wait and retry",
            "5": "not found — verify resource ID"
        },
        "env_prefix": "SUNO_",
        "auth_path": auth_path,
        "auth": {
            "recommended": "sunox login",
            "methods": [
                "browser_cookie_extract",
                "interactive_browser_login",
                "full_cookie_header",
                "raw_clerk_client_cookie",
                "direct_jwt",
                "stored_clerk_refresh",
            ],
            "login_fallback": "`sunox login` first probes existing browser cookies; if that fails, it opens a dedicated Sunox Chrome/Edge-compatible browser profile and captures the Clerk session after the user logs in.",
            "logout": "`sunox logout` removes stored auth and the dedicated interactive browser profile",
            "generation_challenge": "Commands that submit through /api/generate/v2-web/ preflight POST /api/c/check with ctype=generation. If Suno reports a challenge and stored Clerk refresh material exists, Sunox refreshes the JWT once and repeats the preflight before surfacing the challenge. If no challenge is required, submit uses token=null/token_provider=null. Use --token <solved> to supply a token or --captcha to force the browser-backed solver; solved-token submits use token_provider=1.",
            "browser_environment": "Browser-cookie login records a stable source browser id and best-effort public profile settings such as accept-language, but does not fabricate user-agent from that label. Interactive login captures runtime user-agent and accept-language via CDP. API calls reuse available fields independently, derive Chromium client hints from the selected user-agent, send stable browser fetch metadata headers, and fall back field-by-field when unavailable.",
        },
        "provider": "direct_suno_unofficial",
        "auth_required": true,
        "default_model": "chirp-fenix (v5.5)",
    });
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

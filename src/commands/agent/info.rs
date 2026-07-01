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
            "clip download": "download completed media and embed MP3 lyrics"
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
                "treat create as submit-only; use clip wait then clip download for finished audio",
                "use semantic exit codes to decide retry, auth, and config actions"
            ]
        },
        "command_notes": {
            "create": {
                "default_challenge": "preflights POST /api/c/check with ctype=generation; submits with token=null and token_provider=null only when no challenge is required; does not run the browser solver unless --captcha is supplied",
                "challenge_flags": {
                    "--token": "use an externally supplied solved challenge token; submit body uses token_provider=1",
                    "--captcha": "force the browser-backed challenge solver; submit body uses token_provider=1 when a token is produced",
                    "--no-captcha": "do not force the browser-backed solver; generation challenge preflight still runs"
                },
                "modes": "description mode when a non-instrumental prompt is provided; custom lyrics mode when --lyrics or --lyrics-file is provided; custom instrumental mode when --instrumental is provided, with the prompt folded into style tags",
                "title": "optional; omitted title is sent as an empty string for description mode because Suno currently requires params.title to be a string"
            },
            "clip upload": {
                "status": "user-facing CLI workflow is available",
                "workflow": "create presigned upload, post local bytes to S3 form, finish upload, poll processing, initialize clip, then set title/lyrics/cover metadata when available"
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
                "body_constraints": "task=gen_stem, mv=chirp-v3-0, make_instrumental=true, stem_type_id=91, stem_type_group_name=Twelve, stem_task=twelve",
                "response": "generation response with multiple chirp-stem clips"
            },
            "generate_backed_clip_edits": {
                "commands": ["clip cover", "clip extend", "clip stems"],
                "challenge_flags": "these commands expose --token, --captcha, and --no-captcha because they submit through /api/generate/v2-web/ and can hit the same generation challenge gate"
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
            "tags", "negative_tags", "vocal_gender",
            "weirdness", "style_influence",
            "instrumental", "extend", "concat", "cover", "remaster",
            "stems", "clip_speed", "lyrics", "timed_lyrics", "set_metadata",
            "set_visibility", "search", "delete", "clip_restore",
            "clip_like", "clip_dislike", "optional_captcha_solver", "audio_upload",
            "id3_lyrics_embedding", "voice_persona", "persona_list",
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
            "keys": ["default_model", "poll_interval_secs", "poll_timeout_secs", "output_dir"]
        },
        "resource_management": {
            "clip": {
                "commands": [
                    "clip list", "clip search", "clip info", "clip status", "clip wait",
                    "clip download", "clip upload", "clip delete", "clip restore",
                    "clip like", "clip dislike", "clip set", "clip publish",
                    "clip timed-lyrics", "clip extend", "clip concat",
                    "clip cover", "clip remaster", "clip speed", "clip stems"
                ]
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
                "cover_url_status": "playlist set --image-url accepts existing Suno uploaded image URLs such as https://cdn2.suno.ai/image_<upload_id>.jpeg and maps them to the same v2 cover metadata patch; arbitrary external URLs still use the legacy set_metadata route"
            }
        },
        "exit_codes": {
            "0": "success",
            "1": "transient error (network, web endpoint) — retry",
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
            "generation_challenge": "Commands that submit through /api/generate/v2-web/ preflight POST /api/c/check with ctype=generation. If no challenge is required, submit uses token=null/token_provider=null. Use --token <solved> to supply a token or --captcha to force the browser-backed solver; solved-token submits use token_provider=1.",
            "browser_environment": "Browser-cookie login records a stable source browser id and best-effort public profile settings such as accept-language, but does not fabricate user-agent from that label. Interactive login captures runtime user-agent and accept-language via CDP. API calls reuse available fields independently and fall back field-by-field when unavailable.",
        },
        "provider": "direct_suno_unofficial",
        "auth_required": true,
        "default_model": "chirp-fenix (v5.5)",
    });
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

//! Recovery identity contracts for the observed mutation routes. Do not recursively collect `id`:
//! nested metadata can contain unrelated people, credentials, and arbitrary user content.

use std::collections::BTreeMap;

use serde_json::Value;

pub(super) type Identifiers = BTreeMap<String, Vec<String>>;

pub(super) fn request(
    path: &str,
    body: Option<&Value>,
    context: &[(&'static str, Value)],
    identifiers: &mut Identifiers,
) {
    let path = path.trim_end_matches('/');
    if let Some(body) = body {
        named_fields(path, body, identifiers);
        match path {
            "/api/clips/delete" => array_field(body, "ids", "clip_ids", identifiers),
            "/api/download/authorize" if body["item_type"] == "clip" => {
                field(body, "item_id", "clip_ids", identifiers);
            }
            "/api/custom-model/archive" => field(body, "id", "model_ids", identifiers),
            _ => {}
        }
        if path.starts_with("/api/generate/") {
            for key in ["cover_clip_id", "artist_clip_id", "continue_clip_id"] {
                field(body, key, "clip_ids", identifiers);
            }
            array_field(body, "playlist_clip_ids", "clip_ids", identifiers);
            if let Some(metadata) = body.get("metadata") {
                field(metadata, "recreated_from_clip_id", "clip_ids", identifiers);
            }
        }
        if path.starts_with("/api/persona/") {
            field(body, "root_clip_id", "clip_ids", identifiers);
            field(body, "vox_audio_id", "processed_ids", identifiers);
        }
        if path.starts_with("/api/playlist/") {
            for position in body
                .get("positions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                field(position, "clip_id", "clip_ids", identifiers);
            }
        }
        if let Some(id) = body.pointer("/cover_image/id").and_then(Value::as_str)
            && path.starts_with("/api/gen/")
            && path.ends_with("/set_metadata")
        {
            add(identifiers, "image_ids", id);
        }
    }
    // MutationSpec already knows targets whose request body is empty or uses a route-specific
    // alias. Reuse only its allowlisted identifiers, never its free-form resource or messages.
    for (key, value) in context {
        let object = Value::Object(serde_json::Map::from_iter([(
            key.to_string(),
            value.clone(),
        )]));
        named_fields(path, &object, identifiers);
    }
    let segments = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["api", "uploads", "audio", id, ..] => add(identifiers, "upload_ids", id),
        ["api", "uploads", "image", id, ..] => add(identifiers, "image_upload_ids", id),
        ["api", "playlist", "v2", id, ..] => add(identifiers, "playlist_ids", id),
        ["api", "playlist_reaction", id, "update_reaction_type"] => {
            add(identifiers, "playlist_ids", id)
        }
        [
            "api",
            "persona",
            "edit-persona" | "set_visibility" | "trash-persona",
            id,
        ] => {
            add(identifiers, "persona_ids", id);
        }
        ["api", "persona", id, "toggle_love"] => add(identifiers, "persona_ids", id),
        ["api", "lyrics-projects", id, ..] => add(identifiers, "lyrics_project_ids", id),
        ["api", "edit", "crop" | "fade", id] => add(identifiers, "clip_ids", id),
        [
            "api",
            "gen",
            id,
            "set_metadata"
            | "set_visibility"
            | "update_reaction_type"
            | "convert_wav"
            | "convert_opus",
        ]
        | ["api", "gen", id, "aligned_lyrics", "v3"]
        | ["api", "video", "generate", id] => add(identifiers, "clip_ids", id),
        _ => {}
    }
}

pub(super) fn response(path: &str, value: &Value, identifiers: &mut Identifiers) {
    let path = path.trim_end_matches('/');
    named_fields(path, value, identifiers);
    match path {
        "/api/playlist/create" => {
            // Match decode_playlist's supported wrappers and PlaylistInfo's deferred metadata.
            for candidate in [Some(value), value.get("playlist"), value.get("data")]
                .into_iter()
                .flatten()
            {
                field(candidate, "id", "playlist_ids", identifiers);
                if let Some(metadata) = candidate.get("metadata") {
                    field(metadata, "id", "playlist_ids", identifiers);
                }
            }
        }
        _ if path == "/api/persona/create"
            || path.starts_with("/api/persona/edit-persona/")
            || path.starts_with("/api/persona/set_visibility/") =>
        {
            for candidate in [Some(value), value.get("persona"), value.get("data")]
                .into_iter()
                .flatten()
            {
                field(candidate, "id", "persona_ids", identifiers);
                field(candidate, "vocal_clip_id", "processed_ids", identifiers);
            }
        }
        "/api/uploads/audio" => field(value, "id", "upload_ids", identifiers),
        "/api/uploads/image" => field(value, "id", "image_upload_ids", identifiers),
        "/api/custom-model/create" => field(value, "id", "model_ids", identifiers),
        "/api/lyrics-projects" => field(value, "id", "lyrics_project_ids", identifiers),
        _ if path.starts_with("/api/lyrics-projects/") => {
            field(value, "id", "lyrics_project_ids", identifiers)
        }
        "/api/prompts/upsample" => field(value, "request_id", "prompt_request_ids", identifiers),
        "/api/processed_clip/voice-vox-stem" => field(value, "id", "processed_ids", identifiers),
        "/api/voice-verification" => field(value, "id", "verification_ids", identifiers),
        "/api/generate/concat/v2" | "/api/clips/adjust-speed" | "/api/clips/reverse-clip" => {
            field(value, "id", "clip_ids", identifiers);
        }
        // Preserve a direct identity even when a response schema has drifted. Its route is kept
        // in the write record; avoid guessing which resource command can inspect an unknown ID.
        _ => field(value, "id", "resource_ids", identifiers),
    }
}

fn named_fields(path: &str, value: &Value, identifiers: &mut Identifiers) {
    for (source, target) in [
        ("transaction_uuid", "transaction_uuid"),
        ("clip_id", "clip_ids"),
        ("source_clip_id", "clip_ids"),
        ("underpainting_clip_id", "clip_ids"),
        ("overpainting_clip_id", "clip_ids"),
        ("action_clip_id", "clip_ids"),
        ("playlist_id", "playlist_ids"),
        ("persona_id", "persona_ids"),
        ("batch_id", "batch_ids"),
        ("verification_id", "verification_ids"),
        ("phrase_id", "phrase_ids"),
        ("voice_recording_id", "voice_recording_ids"),
        ("verification_recording_id", "verification_recording_ids"),
        ("lyrics_project_id", "lyrics_project_ids"),
        ("lyrics_request_id", "lyrics_request_ids"),
        ("lyrics_id", "lyrics_ids"),
        ("mashup_id", "mashup_ids"),
    ] {
        field(value, source, target, identifiers);
    }
    let upload_key =
        if path.starts_with("/api/uploads/image/") || path.starts_with("/api/playlist/") {
            "image_upload_ids"
        } else {
            "upload_ids"
        };
    field(value, "upload_id", upload_key, identifiers);
    for key in ["clip_ids", "batch_ids", "image_ids", "video_ids"] {
        array_field(value, key, key, identifiers);
    }
    for clip in value
        .get("clips")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(value.get("clip"))
    {
        field(clip, "id", "clip_ids", identifiers);
    }
}

fn field(value: &Value, source: &str, target: &str, identifiers: &mut Identifiers) {
    if let Some(id) = value.get(source).and_then(Value::as_str) {
        add(identifiers, target, id);
    }
}

fn array_field(value: &Value, source: &str, target: &str, identifiers: &mut Identifiers) {
    for id in value
        .get(source)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = id.as_str().or_else(|| id.get("id").and_then(Value::as_str)) {
            add(identifiers, target, id);
        }
    }
}

fn add(identifiers: &mut Identifiers, key: &str, id: &str) {
    // Identifiers are displayed in generated shell inspection commands. Never persist payloads
    // or credentials merely because an unexpected server field is named `id`.
    if id.len() > 256
        || !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return;
    }
    let values = identifiers.entry(key.to_string()).or_default();
    if !values.iter().any(|value| value == id) {
        values.push(id.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_nested_resources_are_collected_without_walking_arbitrary_metadata() {
        for wrapper in ["playlist", "data"] {
            for deferred in [false, true] {
                let mut ids = Identifiers::new();
                let resource = if deferred {
                    json!({"metadata":{"id":"playlist-known"}})
                } else {
                    json!({"id":"playlist-known"})
                };
                response(
                    "/api/playlist/create/",
                    &json!({wrapper:resource,"unrelated":{"id":"SECRET"}}),
                    &mut ids,
                );
                assert_eq!(
                    ids,
                    Identifiers::from([("playlist_ids".into(), vec!["playlist-known".into()])])
                );
            }
        }
        let mut ids = Identifiers::new();
        response(
            "/api/persona/create/",
            &json!({"data":{"id":"persona-known"}}),
            &mut ids,
        );
        assert_eq!(ids["persona_ids"], ["persona-known"]);
    }

    #[test]
    fn route_aliases_and_spec_context_keep_actual_targets_without_secrets() {
        let mut ids = Identifiers::new();
        request(
            "/api/clips/delete/",
            Some(&json!({"ids":["clip-a","clip-b"]})),
            &[],
            &mut ids,
        );
        assert_eq!(ids["clip_ids"], ["clip-a", "clip-b"]);
        let mut ids = Identifiers::new();
        request(
            "/api/playlist/set_metadata",
            Some(
                &json!({"playlist_id":"playlist-a","image_url":"https://secret.invalid/signature"}),
            ),
            &[
                ("clip_ids", json!(["clip-a"])),
                ("resource", json!("PRIVATE TITLE")),
            ],
            &mut ids,
        );
        assert_eq!(ids.len(), 2);
        assert_eq!(ids["playlist_ids"], ["playlist-a"]);
        assert_eq!(ids["clip_ids"], ["clip-a"]);
        let mut unrelated = Identifiers::new();
        request(
            "/unknown",
            Some(&json!({"ids":["not-a-known-resource"]})),
            &[],
            &mut unrelated,
        );
        assert!(unrelated.is_empty());
    }

    #[test]
    fn audio_and_image_uploads_keep_distinct_inspection_identities() {
        let mut ids = Identifiers::new();
        response(
            "/api/uploads/image/",
            &json!({"id":"image-upload","fields":{"id":"SECRET"}}),
            &mut ids,
        );
        request(
            "/api/uploads/image/image-upload/upload-finish/",
            None,
            &[("upload_id", json!("image-upload"))],
            &mut ids,
        );
        assert_eq!(
            ids,
            Identifiers::from([("image_upload_ids".into(), vec!["image-upload".into()])])
        );
        response(
            "/api/uploads/audio/",
            &json!({"id":"audio-upload"}),
            &mut ids,
        );
        assert_eq!(ids["upload_ids"], ["audio-upload"]);
    }

    #[test]
    fn mutation_request_aliases_preserve_targets_without_promoting_payloads() {
        for (path, body, expected) in [
            (
                "/api/download/authorize",
                json!({"item_id":"clip-download","item_type":"clip"}),
                Identifiers::from([("clip_ids".into(), vec!["clip-download".into()])]),
            ),
            (
                "/api/download/authorize",
                json!({"item_id":"unknown-resource","item_type":"other"}),
                Identifiers::new(),
            ),
            (
                "/api/custom-model/archive/",
                json!({"id":"model-archived","name":"PrivateModelTitle"}),
                Identifiers::from([("model_ids".into(), vec!["model-archived".into()])]),
            ),
            (
                "/api/generate/v2-web/",
                json!({"underpainting_clip_id":"vocal-source","overpainting_clip_id":"instrumental-source","prompt":"PrivateLyrics"}),
                Identifiers::from([(
                    "clip_ids".into(),
                    vec!["vocal-source".into(), "instrumental-source".into()],
                )]),
            ),
            (
                "/api/playlist/v2/playlist-known/tracks/reorder-by-index",
                json!({"positions":[{"clip_id":"clip-position","index":3}]}),
                Identifiers::from([
                    ("playlist_ids".into(), vec!["playlist-known".into()]),
                    ("clip_ids".into(), vec!["clip-position".into()]),
                ]),
            ),
            (
                "/api/persona/create/",
                json!({"root_clip_id":"source-clip","vox_audio_id":"voice-processed","name":"PrivatePersonaTitle"}),
                Identifiers::from([
                    ("clip_ids".into(), vec!["source-clip".into()]),
                    ("processed_ids".into(), vec!["voice-processed".into()]),
                ]),
            ),
        ] {
            let mut ids = Identifiers::new();
            request(path, Some(&body), &[], &mut ids);
            assert_eq!(ids, expected, "request contract for {path}");
        }
    }

    #[test]
    fn bodyless_mutations_keep_route_identity_and_upload_kind() {
        for (path, key, id) in [
            (
                "/api/uploads/audio/audio-known/upload-finish/",
                "upload_ids",
                "audio-known",
            ),
            (
                "/api/uploads/image/image-known/upload-finish/",
                "image_upload_ids",
                "image-known",
            ),
            (
                "/api/persona/persona-known/toggle_love/",
                "persona_ids",
                "persona-known",
            ),
            (
                "/api/lyrics-projects/project-known",
                "lyrics_project_ids",
                "project-known",
            ),
            ("/api/gen/clip-known/convert_wav/", "clip_ids", "clip-known"),
            ("/api/video/generate/clip-known/", "clip_ids", "clip-known"),
            ("/api/edit/crop/clip-known/", "clip_ids", "clip-known"),
        ] {
            let mut ids = Identifiers::new();
            request(path, None, &[], &mut ids);
            assert_eq!(
                ids,
                Identifiers::from([(key.into(), vec![id.into()])]),
                "bodyless request contract for {path}"
            );
        }
    }

    #[test]
    fn successful_response_ids_retain_their_resource_type() {
        for (path, key) in [
            ("/api/generate/concat/v2/", "clip_ids"),
            ("/api/clips/adjust-speed/", "clip_ids"),
            ("/api/clips/reverse-clip/", "clip_ids"),
            ("/api/processed_clip/voice-vox-stem", "processed_ids"),
            ("/api/voice-verification/", "verification_ids"),
            ("/api/lyrics-projects", "lyrics_project_ids"),
            ("/api/custom-model/create/", "model_ids"),
        ] {
            let mut ids = Identifiers::new();
            response(
                path,
                &json!({"id":"resource-known","metadata":{"id":"PrivateMetadataValue"},"prompt":"PrivatePromptValue"}),
                &mut ids,
            );
            assert_eq!(
                ids,
                Identifiers::from([(key.into(), vec!["resource-known".into()])]),
                "response contract for {path}"
            );
        }
        let mut ids = Identifiers::new();
        response(
            "/api/uploads/audio/upload-known/initialize-clip/",
            &json!({"clip_id":"clip-known","clip":{"id":"clip-known"}}),
            &mut ids,
        );
        assert_eq!(
            ids,
            Identifiers::from([("clip_ids".into(), vec!["clip-known".into()])])
        );
        let mut ids = Identifiers::new();
        response(
            "/api/video_gen/image/generate",
            &json!({"batch_id":"batch-known","image_ids":["image-known"],"prompt":"PrivatePromptValue"}),
            &mut ids,
        );
        assert_eq!(
            ids,
            Identifiers::from([
                ("batch_ids".into(), vec!["batch-known".into()]),
                ("image_ids".into(), vec!["image-known".into()]),
            ])
        );
    }
}

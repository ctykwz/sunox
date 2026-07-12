use serde_json::json;

use crate::api::types::{CreatePersonaRequest, EditPersonaRequest, PersonaListScope};
use crate::app::AppContext;
use crate::cli::{
    PersonaArgs, PersonaClipsArgs, PersonaCommand, PersonaCreateArgs, PersonaDeleteArgs,
    PersonaInfoArgs, PersonaListArgs, PersonaListKind, PersonaLoveArgs, PersonaProcessedClipArgs,
    PersonaPublishArgs, PersonaRestoreArgs, PersonaSetArgs, PersonaToggleLoveArgs,
};
use crate::core::{CliError, ensure_destructive_confirmed, ensure_time_range};
use crate::output::{self, OutputFormat};

pub async fn run(args: PersonaArgs, ctx: &AppContext) -> Result<(), CliError> {
    match args.command {
        PersonaCommand::List(args) => list(args, ctx).await,
        PersonaCommand::Info(args) => info(args, ctx).await,
        PersonaCommand::Clips(args) => clips(args, ctx).await,
        PersonaCommand::Create(args) => create(*args, ctx).await,
        PersonaCommand::Set(args) => set(args, ctx).await,
        PersonaCommand::ProcessedClip(args) => processed_clip(args, ctx).await,
        PersonaCommand::Publish(args) => publish(args, ctx, true).await,
        PersonaCommand::Unpublish(args) => publish(args, ctx, false).await,
        PersonaCommand::Love(args) => love(args, ctx).await,
        PersonaCommand::Unlove(args) => unlove(args, ctx).await,
        PersonaCommand::ToggleLove(args) => toggle_love(args, ctx).await,
        PersonaCommand::Delete(args) => update_trash(args, ctx, PersonaTrashAction::Trash).await,
        PersonaCommand::Restore(args) => restore(args, ctx).await,
        PersonaCommand::Purge(args) => update_trash(args, ctx, PersonaTrashAction::Purge).await,
    }
}

async fn list(args: PersonaListArgs, ctx: &AppContext) -> Result<(), CliError> {
    let response = ctx
        .client()
        .await?
        .list_personas(
            persona_scope(args.kind),
            args.page,
            args.continuation_token.as_deref(),
        )
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&response),
        OutputFormat::Table => {
            output::table::personas(&response.personas);
            eprintln!(
                "Page {} · total personas: {}",
                response.current_page, response.total_results
            );
            if let Some(token) = &response.continuation_token {
                eprintln!("Continuation token: {token}");
            }
        }
    }
    Ok(())
}

async fn info(args: PersonaInfoArgs, ctx: &AppContext) -> Result<(), CliError> {
    let persona = ctx.client().await?.get_persona(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&persona),
        OutputFormat::Table => output::table::persona(&persona),
    }
    Ok(())
}

async fn clips(args: PersonaClipsArgs, ctx: &AppContext) -> Result<(), CliError> {
    let response = ctx
        .client()
        .await?
        .get_persona_clips(&args.id, args.page)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&response),
        OutputFormat::Table => {
            let clips = response
                .persona
                .persona_clips
                .iter()
                .map(|entry| entry.clip.clone())
                .collect::<Vec<_>>();
            output::table::clips(&clips);
            eprintln!(
                "Page {} · total clips: {}",
                response.current_page, response.total_results
            );
        }
    }
    Ok(())
}

async fn create(args: PersonaCreateArgs, ctx: &AppContext) -> Result<(), CliError> {
    let req = build_create_persona_request(args)?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let persona = client.create_persona(&req).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&persona),
        OutputFormat::Table => {
            output::table::persona(&persona);
            eprintln!("Created persona {}", persona.id);
        }
    }
    Ok(())
}

fn build_create_persona_request(args: PersonaCreateArgs) -> Result<CreatePersonaRequest, CliError> {
    ensure_time_range("persona vocal range", args.vocal_start, args.vocal_end)?;
    Ok(CreatePersonaRequest {
        root_clip_id: Some(args.root_clip_id),
        name: args.name,
        description: args.description,
        image_s3_id: args.image_s3_id,
        is_public: Some(args.public),
        is_suno_persona: None,
        persona_type: args.persona_type,
        vox_audio_id: args.vox_audio_id,
        vocal_start_s: args.vocal_start,
        vocal_end_s: args.vocal_end,
        user_input_styles: args.user_input_styles,
        source: args.source,
        singer_skill_level: args.singer_skill_level,
        clips: None,
        is_voice_recording: None,
        voice_recording_id: None,
        verification_id: None,
    })
}

async fn set(args: PersonaSetArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_time_range("persona vocal range", args.vocal_start, args.vocal_end)?;
    if args.name.is_none()
        && args.description.is_none()
        && args.public.is_none()
        && args.persona_type.is_none()
        && args.user_input_styles.is_none()
        && args.vox_audio_id.is_none()
        && args.vocal_start.is_none()
        && args.vocal_end.is_none()
    {
        return Err(CliError::Config(
            "provide at least one persona field to update".into(),
        ));
    }

    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let current = client.get_persona(&args.id).await?;
    let req = build_edit_persona_request(args, current);
    ensure_time_range("persona vocal range", req.vocal_start_s, req.vocal_end_s)?;
    let persona = client.edit_persona(&req).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&persona),
        OutputFormat::Table => {
            output::table::persona(&persona);
            eprintln!("Updated persona {}", persona.id);
        }
    }
    Ok(())
}

async fn processed_clip(args: PersonaProcessedClipArgs, ctx: &AppContext) -> Result<(), CliError> {
    let processed = ctx.client().await?.get_processed_clip(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&processed),
        OutputFormat::Table => {
            println!("ID: {}", processed.id);
            println!("Status: {}", processed.status);
            if let Some(start) = processed.vocal_start_s {
                println!("Vocal start: {start:.2}s");
            }
            if let Some(end) = processed.vocal_end_s {
                println!("Vocal end: {end:.2}s");
            }
            if let Some(url) = processed.vocal_audio_url {
                println!("Vocal audio: {url}");
            }
        }
    }
    Ok(())
}

async fn publish(
    args: PersonaPublishArgs,
    ctx: &AppContext,
    is_public: bool,
) -> Result<(), CliError> {
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let persona = client.set_persona_visibility(&args.id, is_public).await?;
    let state = if is_public { "public" } else { "private" };
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "persona_id": persona.id,
            "is_public": persona.is_public,
            "requested_public": is_public
        })),
        OutputFormat::Table => eprintln!("Set persona {} to {state}", persona.id),
    }
    Ok(())
}

async fn love(args: PersonaLoveArgs, ctx: &AppContext) -> Result<(), CliError> {
    set_love(args, ctx, true).await
}

async fn unlove(args: PersonaLoveArgs, ctx: &AppContext) -> Result<(), CliError> {
    set_love(args, ctx, false).await
}

async fn toggle_love(args: PersonaToggleLoveArgs, ctx: &AppContext) -> Result<(), CliError> {
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let response = client.toggle_persona_love(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&response),
        OutputFormat::Table => {
            let state = if response.loved { "loved" } else { "not loved" };
            eprintln!("Persona {} is now {state}", args.id);
        }
    }
    Ok(())
}

async fn set_love(args: PersonaLoveArgs, ctx: &AppContext, loved: bool) -> Result<(), CliError> {
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let response = client.set_persona_love(&args.id, loved).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&response),
        OutputFormat::Table => {
            let state = if response.loved { "loved" } else { "not loved" };
            eprintln!("Persona {} is now {state}", args.id);
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum PersonaTrashAction {
    Trash,
    Purge,
}

impl PersonaTrashAction {
    fn command(self) -> &'static str {
        match self {
            Self::Trash => "sunox persona delete",
            Self::Purge => "sunox persona purge",
        }
    }

    fn result_key(self) -> &'static str {
        match self {
            Self::Trash => "deleted",
            Self::Purge => "purged",
        }
    }

    fn past_tense(self) -> &'static str {
        match self {
            Self::Trash => "Deleted",
            Self::Purge => "Permanently deleted",
        }
    }
}

async fn update_trash(
    args: PersonaDeleteArgs,
    ctx: &AppContext,
    action: PersonaTrashAction,
) -> Result<(), CliError> {
    ensure_destructive_confirmed(args.yes, action.command())?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let ids = std::slice::from_ref(&args.id);
    let response = match action {
        PersonaTrashAction::Trash => client.trash_personas(ids).await?,
        PersonaTrashAction::Purge => client.purge_personas(ids).await?,
    };
    let changed = response.updated_persona_ids.contains(&args.id);
    match ctx.fmt {
        OutputFormat::Json => {
            let mut result = json!({
                "persona_id": args.id,
                "action": action.result_key(),
                "changed": changed,
                "updated_persona_ids": response.updated_persona_ids,
                "voice_persona_count": response.voice_persona_count,
                "max_voice_personas": response.max_voice_personas
            });
            result[action.result_key()] = json!(changed);
            output::json::success(result)
        }
        OutputFormat::Table => eprintln!("{} persona {}", action.past_tense(), args.id),
    }
    Ok(())
}

async fn restore(args: PersonaRestoreArgs, ctx: &AppContext) -> Result<(), CliError> {
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let ids = std::slice::from_ref(&args.id);
    let response = client.restore_personas(ids).await?;
    let changed = response.updated_persona_ids.contains(&args.id);
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "persona_id": args.id,
            "action": "restored",
            "changed": changed,
            "restored": changed,
            "updated_persona_ids": response.updated_persona_ids,
            "voice_persona_count": response.voice_persona_count,
            "max_voice_personas": response.max_voice_personas
        })),
        OutputFormat::Table => eprintln!("Restored persona {}", args.id),
    }
    Ok(())
}

fn persona_scope(kind: PersonaListKind) -> PersonaListScope {
    match kind {
        PersonaListKind::Mine => PersonaListScope::Mine,
        PersonaListKind::Loved => PersonaListScope::Loved,
        PersonaListKind::Followed => PersonaListScope::Followed,
    }
}

fn build_edit_persona_request(
    args: PersonaSetArgs,
    current: crate::api::types::PersonaInfo,
) -> EditPersonaRequest {
    EditPersonaRequest {
        persona_id: args.id,
        name: args.name.or(Some(current.name)),
        description: args.description.or(current.description),
        is_public: args.public.or(current.is_public),
        persona_type: args.persona_type.or(current.persona_type),
        user_input_styles: args.user_input_styles.or(current.user_input_styles),
        vox_audio_id: args.vox_audio_id.or(current.vocal_clip_id),
        vocal_start_s: args.vocal_start.or(current.vocal_start_s),
        vocal_end_s: args.vocal_end.or(current.vocal_end_s),
    }
}

#[cfg(test)]
mod tests {
    use crate::api::types::PersonaInfo;
    use crate::cli::{PersonaCreateArgs, PersonaSetArgs};

    use super::{build_create_persona_request, build_edit_persona_request};

    #[test]
    fn create_request_is_private_by_default() {
        let request = build_create_persona_request(PersonaCreateArgs {
            root_clip_id: "clip-1".into(),
            name: None,
            description: None,
            image_s3_id: None,
            public: false,
            persona_type: None,
            vox_audio_id: None,
            vocal_start: Some(0.0),
            vocal_end: Some(10.0),
            user_input_styles: None,
            source: None,
            singer_skill_level: None,
        })
        .expect("valid create request");

        assert_eq!(request.is_public, Some(false));
    }

    #[test]
    fn edit_request_preserves_existing_web_fields_when_not_overridden() {
        let current = PersonaInfo {
            id: "persona-1".into(),
            name: "Lead Voice".into(),
            description: Some("Warm".into()),
            image_s3_id: None,
            user_display_name: None,
            user_handle: None,
            user_image_url: None,
            persona_type: Some("vox".into()),
            root_clip_id: None,
            is_loved: false,
            is_owned: true,
            is_public: Some(true),
            is_trashed: false,
            is_hidden: false,
            clip_count: Some(4),
            follower_count: None,
            is_following: false,
            source: Some("generated_clip".into()),
            user_input_styles: Some("soul".into()),
            vocal_start_s: Some(0.43),
            vocal_end_s: Some(22.56),
            vocal_clip_id: Some("processed-1".into()),
            clip: None,
            persona_clips: Vec::new(),
        };
        let args = PersonaSetArgs {
            id: "persona-1".into(),
            name: Some("Renamed".into()),
            description: None,
            public: None,
            persona_type: None,
            user_input_styles: None,
            vox_audio_id: None,
            vocal_start: None,
            vocal_end: None,
        };

        let req = build_edit_persona_request(args, current);

        assert_eq!(req.name.as_deref(), Some("Renamed"));
        assert_eq!(req.description.as_deref(), Some("Warm"));
        assert_eq!(req.is_public, Some(true));
        assert_eq!(req.persona_type.as_deref(), Some("vox"));
        assert_eq!(req.user_input_styles.as_deref(), Some("soul"));
        assert_eq!(req.vox_audio_id.as_deref(), Some("processed-1"));
        assert_eq!(req.vocal_start_s, Some(0.43));
        assert_eq!(req.vocal_end_s, Some(22.56));
    }

    #[test]
    fn edit_request_sends_visibility_only_when_requested() {
        let current = PersonaInfo {
            id: "persona-1".into(),
            name: "Lead Voice".into(),
            description: None,
            image_s3_id: None,
            user_display_name: None,
            user_handle: None,
            user_image_url: None,
            persona_type: None,
            root_clip_id: None,
            is_loved: false,
            is_owned: true,
            is_public: None,
            is_trashed: false,
            is_hidden: false,
            clip_count: None,
            follower_count: None,
            is_following: false,
            source: None,
            user_input_styles: None,
            vocal_start_s: None,
            vocal_end_s: None,
            vocal_clip_id: None,
            clip: None,
            persona_clips: Vec::new(),
        };
        let args = PersonaSetArgs {
            id: "persona-1".into(),
            name: Some("Renamed".into()),
            description: None,
            public: Some(true),
            persona_type: None,
            user_input_styles: None,
            vox_audio_id: None,
            vocal_start: None,
            vocal_end: None,
        };

        let req = build_edit_persona_request(args, current);

        assert_eq!(req.is_public, Some(true));
    }
}

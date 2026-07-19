use serde_json::{Value, json};

use crate::api::types::{ClipReaction, SetMetadataRequest};
use crate::app::AppContext;
use crate::cli::{
    DeleteArgs, EmptyTrashArgs, PublishArgs, PurgeArgs, ReactionArgs, RestoreArgs, SetArgs,
};
use crate::core::{CliError, ensure_clip_ids, ensure_destructive_confirmed};
use crate::output::{self, OutputFormat};
use crate::workflow::image_upload;

pub async fn delete(args: DeleteArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    ensure_destructive_confirmed(args.yes, "sunox clip delete")?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    client.delete_clips(&args.ids).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(clip_ids_result(&args.ids, "deleted", true)),
        OutputFormat::Table => eprintln!("Deleted {} clip(s)", args.ids.len()),
    }
    Ok(())
}

pub async fn restore(args: RestoreArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    client.restore_clips(&args.ids).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(clip_ids_result(&args.ids, "restored", true)),
        OutputFormat::Table => eprintln!("Restored {} clip(s)", args.ids.len()),
    }
    Ok(())
}

pub async fn purge(args: PurgeArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    ensure_destructive_confirmed(args.yes, "sunox clip purge")?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    client.purge_clips(&args.ids).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(clip_ids_result(&args.ids, "purged", true)),
        OutputFormat::Table => eprintln!("Permanently deleted {} clip(s)", args.ids.len()),
    }
    Ok(())
}

pub async fn empty_trash(args: EmptyTrashArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_destructive_confirmed(args.yes, "sunox clip empty-trash")?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let purged = client.empty_clip_trash().await?;

    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "purged_clip_ids": purged,
            "purged": true
        })),
        OutputFormat::Table => eprintln!("Permanently deleted {} trashed clip(s)", purged.len()),
    }
    Ok(())
}

pub async fn like(args: ReactionArgs, ctx: &AppContext) -> Result<(), CliError> {
    react(args, ctx, ClipReaction::Like).await
}

pub async fn dislike(args: ReactionArgs, ctx: &AppContext) -> Result<(), CliError> {
    react(args, ctx, ClipReaction::Dislike).await
}

pub async fn set(args: SetArgs, ctx: &AppContext) -> Result<(), CliError> {
    let changes = set_changed_fields(&args);
    if changes.is_empty() {
        return Err(CliError::Config(
            "provide at least one metadata field: --title, --lyrics, --lyrics-file, --caption, --image-url, --image-file, --remove-cover, or --remove-video-cover".into(),
        ));
    }

    let lyrics = match (&args.lyrics, &args.lyrics_file) {
        (Some(l), _) => Some(l.clone()),
        (_, Some(path)) => Some(std::fs::read_to_string(path)?),
        _ => None,
    };
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let uploaded_cover = if let Some(image_file) = args.image_file.as_deref() {
        if !ctx.quiet {
            eprintln!("Uploading clip cover image...");
        }
        Some(image_upload::run(&client, image_file).await?)
    } else {
        None
    };
    let image_url = uploaded_cover
        .as_ref()
        .map(|uploaded| uploaded.image_url.clone())
        .or_else(|| args.image_url.clone());
    let req = SetMetadataRequest {
        title: args.title.clone(),
        lyrics,
        caption: args.caption.clone(),
        image_url: uploaded_cover.is_none().then_some(image_url).flatten(),
        image_s3_id: uploaded_cover
            .as_ref()
            .map(|uploaded| uploaded.cover_image_s3_id.clone()),
        is_audio_upload_tos_accepted: None,
        remove_image_cover: if args.remove_cover { Some(true) } else { None },
        remove_video_cover: if args.remove_video_cover {
            Some(true)
        } else {
            None
        },
    };
    match uploaded_cover.as_ref() {
        Some(cover) => {
            image_upload::apply_uploaded_cover_to_clip(&client, &args.id, &req, cover).await?
        }
        None => client.set_metadata(&args.id, &req).await?,
    }
    match ctx.fmt {
        OutputFormat::Json => output::json::success(set_result(&args.id, &changes)),
        OutputFormat::Table => eprintln!("Updated: {}", changes.join(", ")),
    }
    Ok(())
}

pub async fn publish(args: PublishArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let is_public = !args.private;
    let mut succeeded = Vec::with_capacity(args.ids.len());
    for (index, id) in args.ids.iter().enumerate() {
        if let Err(error) = client.set_visibility(id, is_public).await {
            return Err(clip_mutation_error(
                if is_public { "publish" } else { "unpublish" },
                &succeeded,
                id,
                &args.ids[index + 1..],
                error,
            ));
        }
        succeeded.push(id.clone());
    }
    let state = if is_public { "public" } else { "private" };
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "clip_ids": args.ids,
            "is_public": is_public
        })),
        OutputFormat::Table => eprintln!("Set {} clip(s) to {state}", args.ids.len()),
    }
    Ok(())
}

async fn react(
    args: ReactionArgs,
    ctx: &AppContext,
    reaction: ClipReaction,
) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let next_reaction = if args.clear { None } else { Some(reaction) };
    let operation = match (reaction, args.clear) {
        (ClipReaction::Like, false) => "like",
        (ClipReaction::Like, true) => "clear_like",
        (ClipReaction::Dislike, false) => "dislike",
        (ClipReaction::Dislike, true) => "clear_dislike",
    };
    let mut succeeded = Vec::with_capacity(args.ids.len());
    for (index, id) in args.ids.iter().enumerate() {
        if let Err(error) = client.set_clip_reaction(id, next_reaction).await {
            return Err(clip_mutation_error(
                operation,
                &succeeded,
                id,
                &args.ids[index + 1..],
                error,
            ));
        }
        succeeded.push(id.clone());
    }
    let action = match (reaction, args.clear) {
        (ClipReaction::Like, false) => "Liked",
        (ClipReaction::Like, true) => "Cleared like for",
        (ClipReaction::Dislike, false) => "Disliked",
        (ClipReaction::Dislike, true) => "Cleared dislike for",
    };
    match ctx.fmt {
        OutputFormat::Json => {
            output::json::success(reaction_result(&args.ids, reaction, args.clear))
        }
        OutputFormat::Table => eprintln!("{action} {} clip(s)", args.ids.len()),
    }
    Ok(())
}

fn clip_ids_result(ids: &[String], key: &str, value: bool) -> Value {
    let mut result = serde_json::Map::new();
    result.insert("clip_ids".to_string(), json!(ids));
    result.insert(key.to_string(), json!(value));
    Value::Object(result)
}

fn clip_mutation_error(
    operation: &str,
    succeeded_clip_ids: &[String],
    failed_clip_id: &str,
    not_attempted_clip_ids: &[String],
    error: CliError,
) -> CliError {
    if succeeded_clip_ids.is_empty() {
        return error;
    }

    CliError::PartialMutation {
        message: format!(
            "{operation} completed for {} clip(s), failed for {failed_clip_id}, and left {} clip(s) not attempted",
            succeeded_clip_ids.len(),
            not_attempted_clip_ids.len()
        ),
        details: json!({
            "operation": operation,
            "succeeded_clip_ids": succeeded_clip_ids,
            "failed": {
                "clip_id": failed_clip_id,
                "code": error.error_code(),
                "message": error.to_string()
            },
            "not_attempted_clip_ids": not_attempted_clip_ids
        }),
    }
}

fn reaction_result(ids: &[String], reaction: ClipReaction, cleared: bool) -> Value {
    json!({
        "clip_ids": ids,
        "reaction": reaction.as_api_value(),
        "cleared": cleared
    })
}

fn set_changed_fields(args: &SetArgs) -> Vec<&'static str> {
    let mut changes = Vec::new();
    if args.title.is_some() {
        changes.push("title");
    }
    if args.lyrics.is_some() || args.lyrics_file.is_some() {
        changes.push("lyrics");
    }
    if args.caption.is_some() {
        changes.push("caption");
    }
    if args.image_url.is_some() || args.image_file.is_some() || args.remove_cover {
        changes.push("cover");
    }
    if args.remove_video_cover {
        changes.push("video_cover");
    }
    changes
}

fn set_result(clip_id: &str, changes: &[&str]) -> Value {
    json!({
        "clip_id": clip_id,
        "updated": changes
    })
}

#[cfg(test)]
mod tests {
    use super::{
        clip_ids_result, clip_mutation_error, reaction_result, set_changed_fields, set_result,
    };
    use crate::api::types::ClipReaction;
    use crate::cli::SetArgs;

    #[test]
    fn delete_result_reports_deleted_clip_ids() {
        let ids = vec!["clip-a".to_string(), "clip-b".to_string()];

        let value = clip_ids_result(&ids, "deleted", true);

        assert_eq!(
            value,
            serde_json::json!({
                "clip_ids": ["clip-a", "clip-b"],
                "deleted": true
            })
        );
    }

    #[test]
    fn later_serial_clip_mutation_failure_reports_recoverable_progress() {
        let error = clip_mutation_error(
            "publish",
            &["clip-a".into()],
            "clip-b",
            &["clip-c".into()],
            crate::core::CliError::Api {
                code: "api_error",
                message: "server rejected mutation".into(),
            },
        );

        assert_eq!(error.error_code(), "partial_mutation");
        assert_eq!(
            error.details().expect("partial mutation details"),
            &serde_json::json!({
                "operation": "publish",
                "succeeded_clip_ids": ["clip-a"],
                "failed": {
                    "clip_id": "clip-b",
                    "code": "api_error",
                    "message": "API error: server rejected mutation"
                },
                "not_attempted_clip_ids": ["clip-c"]
            })
        );
    }

    #[test]
    fn first_serial_clip_mutation_failure_preserves_semantic_error() {
        let error = clip_mutation_error(
            "like",
            &[],
            "clip-a",
            &["clip-b".into()],
            crate::core::CliError::RateLimited,
        );

        assert_eq!(error.error_code(), "rate_limited");
    }

    #[test]
    fn reaction_result_reports_clear_state() {
        let ids = vec!["clip-a".to_string()];

        let value = reaction_result(&ids, ClipReaction::Dislike, true);

        assert_eq!(
            value,
            serde_json::json!({
                "clip_ids": ["clip-a"],
                "reaction": "DISLIKE",
                "cleared": true
            })
        );
    }

    #[test]
    fn set_result_reports_changed_fields() {
        let value = set_result("clip-a", &["title", "lyrics"]);

        assert_eq!(
            value,
            serde_json::json!({
                "clip_id": "clip-a",
                "updated": ["title", "lyrics"]
            })
        );
    }

    #[test]
    fn set_changed_fields_reports_cover_updates() {
        let args = SetArgs {
            id: "clip-a".into(),
            title: None,
            lyrics: None,
            lyrics_file: None,
            caption: None,
            image_url: Some("https://cdn2.suno.ai/image_upload-1.jpeg".into()),
            image_file: None,
            remove_cover: false,
            remove_video_cover: true,
        };

        assert_eq!(set_changed_fields(&args), vec!["cover", "video_cover"]);
    }
}

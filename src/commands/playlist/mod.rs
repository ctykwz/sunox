use serde_json::json;

use crate::api::types::PlaylistReaction;
use crate::app::AppContext;
use crate::cli::{
    PlaylistArgs, PlaylistCommand, PlaylistCreateArgs, PlaylistDeleteArgs, PlaylistInfoArgs,
    PlaylistListArgs, PlaylistPublishArgs, PlaylistReactionArgs, PlaylistReorderArgs,
    PlaylistRestoreArgs, PlaylistSaveArgs, PlaylistSetArgs, PlaylistTracksArgs,
};
use crate::core::{CliError, ensure_clip_ids, ensure_destructive_confirmed};
use crate::output::{self, OutputFormat};
use crate::workflow::{image_upload, playlist as playlist_workflow};

pub async fn run(args: PlaylistArgs, ctx: &AppContext) -> Result<(), CliError> {
    match args.command {
        PlaylistCommand::List(args) => list(args, ctx).await,
        PlaylistCommand::Info(args) => info(args, ctx).await,
        PlaylistCommand::Create(args) => create(args, ctx).await,
        PlaylistCommand::Set(args) => set(args, ctx).await,
        PlaylistCommand::Add(args) => add(args, ctx).await,
        PlaylistCommand::Remove(args) => remove(args, ctx).await,
        PlaylistCommand::Publish(args) => publish(args, ctx).await,
        PlaylistCommand::Reorder(args) => reorder(args, ctx).await,
        PlaylistCommand::Restore(args) => restore(args, ctx).await,
        PlaylistCommand::Save(args) => save(args, ctx).await,
        PlaylistCommand::Unsave(args) => unsave(args, ctx).await,
        PlaylistCommand::Like(args) => like(args, ctx).await,
        PlaylistCommand::Dislike(args) => dislike(args, ctx).await,
        PlaylistCommand::Delete(args) => delete(args, ctx).await,
    }
}

async fn list(args: PlaylistListArgs, ctx: &AppContext) -> Result<(), CliError> {
    let response = ctx.client().await?.list_playlists(args.page).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&response),
        OutputFormat::Table => {
            output::table::playlists(&response.playlists);
            eprintln!(
                "Page {} · total playlists: {}",
                response.current_page, response.num_total_results
            );
        }
    }
    Ok(())
}

async fn info(args: PlaylistInfoArgs, ctx: &AppContext) -> Result<(), CliError> {
    let playlist = ctx.client().await?.get_playlist(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&playlist),
        OutputFormat::Table => output::table::playlist_detail(&playlist),
    }
    Ok(())
}

async fn create(args: PlaylistCreateArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let client = ctx.client().await?;
    let uploaded_cover = if let Some(image_file) = args.image_file.as_deref() {
        if !ctx.quiet {
            eprintln!("Uploading playlist cover image...");
        }
        Some(image_upload::run(&client, image_file).await?)
    } else {
        None
    };
    let image_url_upload_id = args
        .image_url
        .as_deref()
        .and_then(crate::api::playlist::upload_id_from_suno_image_url);
    let external_image_url = args
        .image_url
        .as_deref()
        .filter(|_| image_url_upload_id.is_none());
    let cover = uploaded_cover
        .as_ref()
        .map(|uploaded| {
            playlist_workflow::CoverReference::uploaded(&uploaded.upload_id, &uploaded.image_url)
        })
        .or_else(|| {
            image_url_upload_id
                .as_deref()
                .zip(args.image_url.as_deref())
                .map(|(upload_id, image_url)| {
                    playlist_workflow::CoverReference::existing(upload_id, image_url)
                })
        });
    let playlist = playlist_workflow::create(
        &client,
        playlist_workflow::CreatePlaylistInput {
            name: &args.name,
            description: args.description.as_deref(),
            external_image_url,
            cover,
        },
    )
    .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&playlist),
        OutputFormat::Table => {
            output::table::playlist_detail(&playlist);
            eprintln!("Created playlist {}", playlist.id);
        }
    }
    Ok(())
}

async fn set(args: PlaylistSetArgs, ctx: &AppContext) -> Result<(), CliError> {
    if args.name.is_none()
        && args.description.is_none()
        && args.image_url.is_none()
        && args.image_file.is_none()
    {
        return Err(CliError::Config(
            "provide at least one of --name, --description, --image-url, or --image-file".into(),
        ));
    }

    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let client = ctx.client().await?;
    let uploaded_cover = if let Some(image_file) = args.image_file.as_deref() {
        if !ctx.quiet {
            eprintln!("Uploading playlist cover image...");
        }
        Some(image_upload::run(&client, image_file).await?)
    } else {
        None
    };
    let image_url_upload_id = args
        .image_url
        .as_deref()
        .and_then(crate::api::playlist::upload_id_from_suno_image_url);
    let external_image_url = args
        .image_url
        .as_deref()
        .filter(|_| image_url_upload_id.is_none());
    let cover = uploaded_cover
        .as_ref()
        .map(|uploaded| {
            playlist_workflow::CoverReference::uploaded(&uploaded.upload_id, &uploaded.image_url)
        })
        .or_else(|| {
            image_url_upload_id
                .as_deref()
                .zip(args.image_url.as_deref())
                .map(|(upload_id, image_url)| {
                    playlist_workflow::CoverReference::existing(upload_id, image_url)
                })
        });
    let playlist = playlist_workflow::set(
        &client,
        playlist_workflow::SetPlaylistInput {
            playlist_id: &args.id,
            name: args.name.as_deref(),
            description: args.description.as_deref(),
            external_image_url,
            cover,
        },
    )
    .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&playlist),
        OutputFormat::Table => {
            output::table::playlist_detail(&playlist);
            eprintln!("Updated playlist {}", playlist.id);
        }
    }
    Ok(())
}

async fn add(args: PlaylistTracksArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.clip_ids)?;
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client()
        .await?
        .add_clips_to_playlist(&args.id, &args.clip_ids)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "playlist_id": args.id,
            "clip_ids": args.clip_ids,
            "action": "add"
        })),
        OutputFormat::Table => eprintln!("Added {} clip(s)", args.clip_ids.len()),
    }
    Ok(())
}

async fn remove(args: PlaylistTracksArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.clip_ids)?;
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let report = ctx
        .client()
        .await?
        .remove_clips_from_playlist(&args.id, &args.clip_ids)
        .await?;
    if !report.is_success() {
        return Err(CliError::PartialMutation {
            message: format!(
                "playlist remove for {} succeeded for {} clip(s), failed for {} clip(s), and left {} clip(s) not attempted",
                args.id,
                report.succeeded_clip_ids.len(),
                report.failed.len(),
                report.not_attempted_clip_ids.len()
            ),
            details: serde_json::to_value(&report)?,
        });
    }
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&report),
        OutputFormat::Table => eprintln!("Removed {} clip(s)", report.succeeded_clip_ids.len()),
    }
    Ok(())
}

async fn publish(args: PlaylistPublishArgs, ctx: &AppContext) -> Result<(), CliError> {
    let is_public = !args.private;
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client()
        .await?
        .set_playlist_visibility(&args.id, is_public)
        .await?;
    let state = if is_public { "public" } else { "private" };
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "playlist_id": args.id,
            "is_public": is_public
        })),
        OutputFormat::Table => eprintln!("Set playlist {} to {state}", args.id),
    }
    Ok(())
}

async fn reorder(args: PlaylistReorderArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client()
        .await?
        .reorder_playlist_clip(&args.id, &args.clip_id, args.index)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "playlist_id": args.id,
            "clip_id": args.clip_id,
            "index": args.index
        })),
        OutputFormat::Table => eprintln!(
            "Moved clip {} in playlist {} to index {}",
            args.clip_id, args.id, args.index
        ),
    }
    Ok(())
}

async fn restore(args: PlaylistRestoreArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client().await?.restore_playlist(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => {
            output::json::success(json!({ "playlist_id": args.id, "restored": true }))
        }
        OutputFormat::Table => eprintln!("Restored playlist {}", args.id),
    }
    Ok(())
}

async fn save(args: PlaylistSaveArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client().await?.save_playlist(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => {
            output::json::success(json!({ "playlist_id": args.id, "saved": true }))
        }
        OutputFormat::Table => eprintln!("Saved playlist {}", args.id),
    }
    Ok(())
}

async fn unsave(args: PlaylistSaveArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client().await?.unsave_playlist(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => {
            output::json::success(json!({ "playlist_id": args.id, "saved": false }))
        }
        OutputFormat::Table => eprintln!("Removed saved playlist {}", args.id),
    }
    Ok(())
}

async fn like(args: PlaylistReactionArgs, ctx: &AppContext) -> Result<(), CliError> {
    react(args, ctx, PlaylistReaction::Like).await
}

async fn dislike(args: PlaylistReactionArgs, ctx: &AppContext) -> Result<(), CliError> {
    react(args, ctx, PlaylistReaction::Dislike).await
}

async fn react(
    args: PlaylistReactionArgs,
    ctx: &AppContext,
    reaction: PlaylistReaction,
) -> Result<(), CliError> {
    let next_reaction = if args.clear { None } else { Some(reaction) };
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client()
        .await?
        .set_playlist_reaction(&args.id, next_reaction)
        .await?;
    let action = match (reaction, args.clear) {
        (PlaylistReaction::Like, false) => "Liked",
        (PlaylistReaction::Like, true) => "Cleared like for",
        (PlaylistReaction::Dislike, false) => "Disliked",
        (PlaylistReaction::Dislike, true) => "Cleared dislike for",
    };
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "playlist_id": args.id,
            "reaction": reaction.as_api_value(),
            "cleared": args.clear
        })),
        OutputFormat::Table => eprintln!("{action} playlist {}", args.id),
    }
    Ok(())
}

async fn delete(args: PlaylistDeleteArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_destructive_confirmed(args.yes, "sunox playlist delete")?;
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    ctx.client().await?.trash_playlist(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => {
            output::json::success(json!({ "playlist_id": args.id, "deleted": true }))
        }
        OutputFormat::Table => eprintln!("Deleted playlist {}", args.id),
    }
    Ok(())
}

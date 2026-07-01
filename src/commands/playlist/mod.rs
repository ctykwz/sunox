use serde_json::json;

use crate::api::types::PlaylistReaction;
use crate::app::AppContext;
use crate::cli::{
    PlaylistArgs, PlaylistCommand, PlaylistCreateArgs, PlaylistDeleteArgs, PlaylistInfoArgs,
    PlaylistListArgs, PlaylistPublishArgs, PlaylistReactionArgs, PlaylistReorderArgs,
    PlaylistRestoreArgs, PlaylistSaveArgs, PlaylistSetArgs, PlaylistTracksArgs,
};
use crate::core::{CliError, ensure_clip_ids};
use crate::output::{self, OutputFormat};
use crate::workflow::image_upload;

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
    let client = ctx.client().await?;
    let uploaded_cover = if let Some(image_file) = args.image_file.as_deref() {
        if !ctx.quiet {
            eprintln!("Uploading playlist cover image...");
        }
        Some(image_upload::run(&client, image_file).await?)
    } else {
        None
    };

    let mut playlist = client
        .create_playlist(
            &args.name,
            args.description.as_deref(),
            args.image_url.as_deref(),
        )
        .await?;
    if let Some(uploaded) = uploaded_cover {
        playlist = client
            .set_playlist_uploaded_cover(&playlist.id, &uploaded.upload_id)
            .await?;
    }
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

    let client = ctx.client().await?;
    let mut playlist =
        if args.name.is_some() || args.description.is_some() || args.image_url.is_some() {
            client
                .set_playlist_metadata(
                    &args.id,
                    args.name.as_deref(),
                    args.description.as_deref(),
                    args.image_url.as_deref(),
                )
                .await?
        } else {
            client.get_playlist(&args.id).await?
        };

    if let Some(image_file) = args.image_file.as_deref() {
        if !ctx.quiet {
            eprintln!("Uploading playlist cover image...");
        }
        let uploaded = image_upload::run(&client, image_file).await?;
        playlist = client
            .set_playlist_uploaded_cover(&args.id, &uploaded.upload_id)
            .await?;
    }
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
    ctx.client()
        .await?
        .remove_clips_from_playlist(&args.id, &args.clip_ids)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(json!({
            "playlist_id": args.id,
            "clip_ids": args.clip_ids,
            "action": "remove"
        })),
        OutputFormat::Table => eprintln!("Removed {} clip(s)", args.clip_ids.len()),
    }
    Ok(())
}

async fn publish(args: PlaylistPublishArgs, ctx: &AppContext) -> Result<(), CliError> {
    let is_public = !args.private;
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
    if !args.yes {
        eprintln!("Deleting playlist {}", args.id);
        eprintln!("Use -y to skip confirmation, or press Ctrl+C to cancel");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    ctx.client().await?.trash_playlist(&args.id).await?;
    match ctx.fmt {
        OutputFormat::Json => {
            output::json::success(json!({ "playlist_id": args.id, "deleted": true }))
        }
        OutputFormat::Table => eprintln!("Deleted playlist {}", args.id),
    }
    Ok(())
}

use crate::api::types::FeedFilters;
use crate::app::AppContext;
use crate::cli::{InfoArgs, ListArgs, ListSort, SearchArgs, StatusArgs};
use crate::core::{CliError, ensure_clip_ids};
use crate::output::{self, OutputFormat};
use crate::workflow::tasks;

pub async fn list(args: ListArgs, ctx: &AppContext) -> Result<(), CliError> {
    if args.limit == Some(0) {
        return Err(CliError::Config("--limit must be greater than 0".into()));
    }
    let filters = list_filters(&args);
    let feed = ctx
        .client()
        .await?
        .feed(args.cursor, args.limit, filters)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&feed),
        OutputFormat::Table => {
            output::table::clips(&feed.clips);
            if let Some(cursor) = &feed.next_cursor {
                eprintln!("Next cursor: {cursor}");
            }
            if feed.has_more && feed.next_cursor.is_none() {
                eprintln!("More results available");
            }
        }
    }
    Ok(())
}

pub async fn search(args: SearchArgs, ctx: &AppContext) -> Result<(), CliError> {
    let feed = ctx.client().await?.search(&args.query).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&feed.clips),
        OutputFormat::Table => {
            if feed.clips.is_empty() {
                eprintln!("No clips matching \"{}\"", args.query);
            } else {
                output::table::clips(&feed.clips);
            }
        }
    }
    Ok(())
}

pub async fn info(args: InfoArgs, ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let clips = client.get_clips(std::slice::from_ref(&args.id)).await?;
    if clips.is_empty() {
        return Err(CliError::NotFound(format!("clip: {}", args.id)));
    }
    let info = client.clip_info(clips[0].clone()).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&info),
        OutputFormat::Table => output::table::clip_detail(&info),
    }
    Ok(())
}

pub async fn status(args: StatusArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    let clips =
        tasks::require_found_clips(&args.ids, ctx.client().await?.get_clips(&args.ids).await?)?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&clips),
        OutputFormat::Table => output::table::clips(&clips),
    }
    Ok(())
}

fn list_filters(args: &ListArgs) -> FeedFilters {
    let mut filters = if args.trashed {
        FeedFilters::trashed()
    } else {
        FeedFilters::default_workspace()
    };
    if args.public {
        filters = filters.with_public();
    }
    if args.liked {
        filters = filters.with_liked();
    }
    if args.upload {
        filters = filters.with_upload();
    }
    if args.cover {
        filters = filters.with_cover();
    }
    if args.extend {
        filters = filters.with_extend();
    }
    if matches!(args.sort, Some(ListSort::Popular)) {
        filters = filters.with_popular_sort();
    }
    filters
}

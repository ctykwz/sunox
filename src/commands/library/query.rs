use std::collections::HashSet;

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
    if args.limit == Some(0) {
        return Err(CliError::Config("--limit must be greater than 0".into()));
    }
    let client = ctx.client().await?;
    let mut feed = client
        .search_page(&args.query, args.cursor.clone(), args.limit)
        .await?;
    if args.all {
        let mut seen = HashSet::new();
        if let Some(cursor) = args.cursor {
            seen.insert(cursor);
        }
        while feed.has_more {
            let cursor = feed.next_cursor.clone().ok_or_else(|| CliError::Api {
                code: "pagination_error",
                message: "Suno reported more search results without a next cursor".into(),
            })?;
            if !seen.insert(cursor.clone()) {
                return Err(CliError::Api {
                    code: "pagination_error",
                    message: "Suno repeated a search pagination cursor".into(),
                });
            }
            let page = client
                .search_page(&args.query, Some(cursor), args.limit)
                .await?;
            feed.clips.extend(page.clips);
            feed.next_cursor = page.next_cursor;
            feed.has_more = page.has_more;
        }
    }
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&feed),
        OutputFormat::Table => {
            if feed.clips.is_empty() {
                eprintln!("No clips matching \"{}\"", args.query);
            } else {
                output::table::clips(&feed.clips);
            }
            if let Some(cursor) = &feed.next_cursor {
                eprintln!("Next cursor: {cursor}");
            } else if feed.has_more {
                eprintln!("More results available, but Suno did not return a cursor");
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

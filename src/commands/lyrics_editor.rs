use std::path::PathBuf;
use std::time::Duration;

use crate::api::PollingOptions;
use crate::api::lyrics_editor::{
    LYRICS_MASHUP_POLL_INTERVAL, LyricsMashupOptions, LyricsRewriteOptions,
};
use crate::api::types::{LyricsMashupStatus, LyricsMashupSubmission, LyricsRewriteResult};
use crate::app::AppContext;
use crate::cli::{
    LYRICS_MASHUP_DEFAULT_TIMEOUT_SECS, LyricsMashupArgs, LyricsMashupStatusArgs, LyricsRewriteArgs,
};
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn rewrite(args: LyricsRewriteArgs, ctx: &AppContext) -> Result<(), CliError> {
    let prompt = require_nonempty("--prompt", args.prompt)?;
    let prefix = text_or_file("prefix", args.prefix, args.prefix_file, false)?;
    let edit = text_or_file("edit", args.edit, args.edit_file, false)?;
    let suffix = text_or_file("suffix", args.suffix, args.suffix_file, false)?;
    let session_token = create_session_token(args.session_token)?;

    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let response = client
        .rewrite_lyrics(LyricsRewriteOptions {
            prompt: &prompt,
            prefix: &prefix,
            edit: &edit,
            suffix: &suffix,
            title: &args.title,
            create_session_token: &session_token,
        })
        .await?;
    render_rewrite(
        response.into_editor_result(&prefix, &edit, &suffix),
        ctx.fmt,
    );
    Ok(())
}

pub async fn mashup(args: LyricsMashupArgs, ctx: &AppContext) -> Result<(), CliError> {
    let lyrics_a = text_or_file(
        "first lyrics source",
        args.lyrics_a,
        args.lyrics_a_file,
        true,
    )?;
    let lyrics_b = text_or_file(
        "second lyrics source",
        args.lyrics_b,
        args.lyrics_b_file,
        true,
    )?;
    let session_token = create_session_token(args.session_token)?;
    let polling = PollingOptions {
        timeout: Duration::from_secs(args.timeout),
        interval: LYRICS_MASHUP_POLL_INTERVAL,
    };
    if !args.no_wait {
        polling.validate()?;
    }

    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let submission = client
        .start_lyrics_mashup(LyricsMashupOptions {
            lyrics_a: &lyrics_a,
            lyrics_b: &lyrics_b,
            create_session_token: &session_token,
        })
        .await?;
    if args.no_wait {
        render_submission(submission, ctx.fmt);
        return Ok(());
    }
    let status = client
        .wait_for_submitted_lyrics_mashup(&submission.mashup_id, polling)
        .await?;
    render_mashup_with_submission(submission, status, ctx.fmt);
    Ok(())
}

pub async fn mashup_status(args: LyricsMashupStatusArgs, ctx: &AppContext) -> Result<(), CliError> {
    let id = require_nonempty("lyrics mashup ID", args.id)?;
    let client = ctx.client().await?;
    let status = if args.wait {
        let polling = PollingOptions {
            timeout: Duration::from_secs(
                args.timeout.unwrap_or(LYRICS_MASHUP_DEFAULT_TIMEOUT_SECS),
            ),
            interval: LYRICS_MASHUP_POLL_INTERVAL,
        };
        polling.validate()?;
        client.wait_for_existing_lyrics_mashup(&id, polling).await?
    } else {
        client.lyrics_mashup_status(&id).await?
    };
    render_status(status, ctx.fmt);
    Ok(())
}

fn text_or_file(
    label: &str,
    text: Option<String>,
    file: Option<PathBuf>,
    require_nonempty_text: bool,
) -> Result<String, CliError> {
    let value = match (text, file) {
        (Some(value), None) => value,
        (None, Some(path)) => std::fs::read_to_string(path)?,
        (None, None) if !require_nonempty_text => String::new(),
        (None, None) => {
            return Err(CliError::Config(format!(
                "provide {label} as text or a UTF-8 file"
            )));
        }
        (Some(_), Some(_)) => {
            return Err(CliError::Config(format!(
                "provide only one text or file input for {label}"
            )));
        }
    };
    if require_nonempty_text && value.trim().is_empty() {
        return Err(CliError::Config(format!("{label} must not be empty")));
    }
    Ok(value)
}

fn require_nonempty(label: &str, value: String) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CliError::Config(format!("{label} must not be empty")));
    }
    Ok(value.to_string())
}

fn create_session_token(token: Option<String>) -> Result<String, CliError> {
    match token {
        Some(token) => require_nonempty("--session-token", token),
        None => Ok(uuid::Uuid::new_v4().to_string()),
    }
}

fn render_rewrite(result: LyricsRewriteResult, format: OutputFormat) {
    match format {
        OutputFormat::Json => output::json::success(result),
        OutputFormat::Table => println!("{}", result.full_text),
    }
}

fn render_submission(submission: LyricsMashupSubmission, format: OutputFormat) {
    match format {
        OutputFormat::Json => output::json::success(submission),
        OutputFormat::Table => println!("{}\tsubmitted", submission.mashup_id),
    }
}

fn render_mashup_with_submission(
    submission: LyricsMashupSubmission,
    status: LyricsMashupStatus,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "submission": submission,
            "result": status,
        })),
        OutputFormat::Table => render_status(status, format),
    }
}

fn render_status(status: LyricsMashupStatus, format: OutputFormat) {
    match format {
        OutputFormat::Json => output::json::success(status),
        OutputFormat::Table => {
            println!("{}\t{}", status.id.as_deref().unwrap_or("-"), status.status);
            if let Some(title) = status.title.as_deref().filter(|title| !title.is_empty()) {
                println!("{title}");
            }
            if let Some(text) = status.text.as_deref() {
                println!("{text}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{create_session_token, text_or_file};

    #[test]
    fn create_session_tokens_are_fresh_uuid_values_by_default() {
        let first = create_session_token(None).expect("first token");
        let second = create_session_token(None).expect("second token");
        assert_ne!(first, second);
        uuid::Uuid::parse_str(&first).expect("UUID session token");
    }

    #[test]
    fn mashup_sources_reject_blank_text_before_auth_or_submit() {
        let error = text_or_file("first lyrics source", Some("  ".into()), None, true)
            .expect_err("blank mashup source");
        assert!(matches!(error, crate::core::CliError::Config(_)));
    }
}

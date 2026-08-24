use std::collections::HashSet;

use crate::api::stems::StemResults;
use crate::app::AppContext;
use crate::cli::{DownloadArgs, GetStemsArgs};
use crate::core::CliError;
use crate::output::{self, OutputFormat};

/// Read or export stem clips that Suno has already produced for a source.
/// This lookup never starts a paid extraction. WAV/OPUS conversion requested
/// by the nested download flow remains subject to its own mutation guard.
pub async fn get(args: GetStemsArgs, ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let pages = client.stem_result_pages(&args.clip_id).await?;
    let results = if let Some(page) = args.page {
        if page >= pages {
            return Err(CliError::NotFound(format!(
                "stem-result page {page} for clip {}; available pages are 0..{}",
                args.clip_id,
                pages.saturating_sub(1)
            )));
        }
        StemResults {
            clip_id: args.clip_id.clone(),
            pages,
            banks: vec![client.get_stem_result_page(&args.clip_id, page).await?],
        }
    } else {
        client.get_all_stem_results(&args.clip_id).await?
    };

    if args.download {
        ensure_complete_hydration(&results)?;
        if !ctx.quiet {
            eprintln!(
                "Downloading existing stems (Suno may meter prepared downloads; stem MP3s skip aligned-lyrics generation, while missing WAV/OPUS files may start conversion unless --no-convert or --read-only is set)..."
            );
        }
        let mut seen = HashSet::new();
        let ids = results
            .banks
            .iter()
            .flat_map(|bank| bank.stems.iter())
            .filter_map(|clip| seen.insert(clip.id.clone()).then_some(clip.id.clone()))
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Err(CliError::NotFound(format!(
                "existing stem results for clip {}",
                args.clip_id
            )));
        }
        return super::media::download(
            DownloadArgs {
                ids,
                output: args.output,
                force: args.force,
                video: false,
                format: args.format,
                no_convert: args.no_convert,
                skip_timed_lyrics: true,
            },
            ctx,
        )
        .await;
    }

    match ctx.fmt {
        OutputFormat::Json => output::json::success(&results),
        OutputFormat::Table => {
            if results.banks.is_empty() {
                eprintln!("No existing stem results for {}", results.clip_id);
            }
            for bank in &results.banks {
                eprintln!("Stem result page {}", bank.page);
                output::table::clips(&bank.stems);
                if !bank.missing_clip_ids.is_empty() {
                    eprintln!(
                        "Unhydrated stem references: {}",
                        bank.missing_clip_ids.join(", ")
                    );
                }
            }
            eprintln!("Stored stem-result pages: {}", results.pages);
        }
    }
    Ok(())
}

fn ensure_complete_hydration(results: &StemResults) -> Result<(), CliError> {
    let missing = results
        .banks
        .iter()
        .flat_map(|bank| bank.missing_clip_ids.iter().cloned())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(CliError::Diagnostic {
        code: "stem_hydration_incomplete",
        message: format!(
            "refusing a partial stem download because {} referenced clip(s) were not returned by clip hydration",
            missing.len()
        ),
        details: serde_json::json!({
            "source_clip_id": results.clip_id,
            "missing_clip_ids": missing,
            "download_started": false,
            "recovery": "retry the read-only clip get-stems command after Suno's feed converges"
        }),
    })
}

#[cfg(test)]
mod tests {
    use crate::api::stems::{StemBank, StemResults};

    use super::ensure_complete_hydration;

    #[test]
    fn partial_stem_hydration_fails_before_any_download() {
        let results = StemResults {
            clip_id: "source-1".into(),
            pages: 1,
            banks: vec![StemBank {
                page: 0,
                stems: Vec::new(),
                missing_clip_ids: vec!["stem-missing".into()],
            }],
        };

        let error = ensure_complete_hydration(&results).expect_err("partial read must fail");
        assert_eq!(error.error_code(), "stem_hydration_incomplete");
        let details = error.details().expect("diagnostic details");
        assert_eq!(details["download_started"], false);
        assert_eq!(details["missing_clip_ids"][0], "stem-missing");
    }
}

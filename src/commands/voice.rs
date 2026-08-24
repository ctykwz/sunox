use std::time::Duration;

use crate::api::PollingOptions;
use crate::app::AppContext;
use crate::cli::{
    VoiceArgs, VoiceCommand, VoiceCreateArgs, VoicePhraseArgs, VoiceProcessedStatusArgs,
    VoiceVerificationStatusArgs,
};
use crate::core::CliError;
use crate::output::{self, OutputFormat};
use crate::workflow::voice::{self, VoiceCreateInput};

pub async fn run(args: VoiceArgs, ctx: &AppContext) -> Result<(), CliError> {
    match args.command {
        VoiceCommand::Phrase(args) => phrase(args, ctx).await,
        VoiceCommand::ProcessedStatus(args) => processed_status(args, ctx).await,
        VoiceCommand::VerificationStatus(args) => verification_status(args, ctx).await,
        VoiceCommand::Create(args) => create(*args, ctx).await,
    }
}

async fn phrase(args: VoicePhraseArgs, ctx: &AppContext) -> Result<(), CliError> {
    let language = required_text("--language", &args.language)?;
    let phrase = ctx.client().await?.get_voice_phrase(language).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&phrase),
        OutputFormat::Table => {
            println!("{}", phrase.phrase_text);
            eprintln!("Phrase ID: {}", phrase.phrase_id);
        }
    }
    Ok(())
}

async fn processed_status(
    args: VoiceProcessedStatusArgs,
    ctx: &AppContext,
) -> Result<(), CliError> {
    let processed_id = required_text("processed_id", &args.processed_id)?;
    let status = ctx
        .client()
        .await?
        .get_processed_voice_status(processed_id)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&status),
        OutputFormat::Table => {
            println!("{}", status.status);
            eprintln!(
                "Processed ID: {}",
                status.id.as_deref().unwrap_or(processed_id)
            );
            if let Some(recording_id) = status.voice_recording_id {
                eprintln!("Voice recording ID: {recording_id}");
            }
        }
    }
    Ok(())
}

async fn verification_status(
    args: VoiceVerificationStatusArgs,
    ctx: &AppContext,
) -> Result<(), CliError> {
    let verification_id = required_text("verification_id", &args.verification_id)?;
    let verification = ctx
        .client()
        .await?
        .get_voice_verification(verification_id)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&verification),
        OutputFormat::Table => {
            println!("{}", verification.status);
            eprintln!("Verification ID: {}", verification.id);
            if let Some(reason) = verification.rejection_reason {
                eprintln!("Rejection reason: {reason}");
            }
        }
    }
    Ok(())
}

async fn create(args: VoiceCreateArgs, ctx: &AppContext) -> Result<(), CliError> {
    let input = VoiceCreateInput {
        sample_file: args.sample,
        verification_file: args.verification,
        phrase_id: args.phrase_id,
        language: args.language,
        sample_duration: args.sample_duration,
        name: args.name,
        description: args.description,
        user_input_styles: args.styles,
        singer_skill_level: args.singer_skill_level,
        confirm_rights: args.confirm_rights,
        confirm_eligibility: args.confirm_eligibility,
        confirm_biometric_consent: args.confirm_biometric_consent,
        checkpoint_dir: None,
        polling: PollingOptions {
            timeout: Duration::from_secs(ctx.config.poll_timeout_secs),
            interval: Duration::from_secs(ctx.config.poll_interval_secs),
        },
    };

    // Fail local file/rights/duration validation before authentication and
    // before acquiring the single lock held across every Voice write.
    voice::preflight(&input).await?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = voice::run(&client, input).await?;

    match ctx.fmt {
        OutputFormat::Json => output::json::success(&result),
        OutputFormat::Table => {
            output::table::persona(&result.persona);
            eprintln!("Created private Voice {}", result.persona.id);
            eprintln!("Workflow ID: {}", result.workflow_id);
            eprintln!("Verification ID: {}", result.verification.id);
            eprintln!("Checkpoint: {}", result.checkpoint_path.display());
        }
    }
    Ok(())
}

fn required_text<'a>(name: &str, value: &'a str) -> Result<&'a str, CliError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CliError::Config(format!("{name} cannot be blank")));
    }
    Ok(value)
}

use std::path::PathBuf;

use clap::Subcommand;

#[derive(clap::Args)]
pub struct VoiceArgs {
    #[command(subcommand)]
    pub command: VoiceCommand,
}

#[derive(Subcommand)]
pub enum VoiceCommand {
    /// Fetch the current dynamic phrase used to verify ownership of a Voice
    Phrase(VoicePhraseArgs),

    /// Inspect processing state for an uploaded Voice sample
    ProcessedStatus(VoiceProcessedStatusArgs),

    /// Inspect ownership-verification state for a Voice
    VerificationStatus(VoiceVerificationStatusArgs),

    /// Create a private verified Voice from a singing sample and phrase recording
    Create(Box<VoiceCreateArgs>),
}

#[derive(clap::Args)]
pub struct VoicePhraseArgs {
    /// Verification phrase language code
    ///
    /// Current Web choices: en, es, fr, pt, de, ja, ko, zh, hi, ru.
    #[arg(
        long,
        default_value = "en",
        value_parser = ["en", "es", "fr", "pt", "de", "ja", "ko", "zh", "hi", "ru"]
    )]
    pub language: String,
}

#[derive(clap::Args)]
pub struct VoiceProcessedStatusArgs {
    /// Processed clip ID returned by voice-vox-stem
    pub processed_id: String,
}

#[derive(clap::Args)]
pub struct VoiceVerificationStatusArgs {
    /// Voice verification ID
    pub verification_id: String,
}

#[derive(clap::Args)]
pub struct VoiceCreateArgs {
    /// Local WAV singing sample containing only your own voice
    #[arg(long, value_name = "PATH")]
    pub sample: PathBuf,

    /// Local WAV recording of the exact dynamic phrase fetched with `voice phrase`
    ///
    /// The current Web recorder targets roughly 15 seconds for this phrase.
    #[arg(long, value_name = "PATH")]
    pub verification: PathBuf,

    /// Phrase ID returned with the recorded dynamic phrase
    #[arg(long)]
    pub phrase_id: String,

    /// Language used when fetching the dynamic verification phrase
    #[arg(
        long,
        default_value = "en",
        value_parser = ["en", "es", "fr", "pt", "de", "ja", "ko", "zh", "hi", "ru"]
    )]
    pub language: String,

    /// Selected singing-sample duration in seconds
    ///
    /// Current Web requires the full duration for a 3-10s source; longer
    /// uploads can select 10-240s. This CLI does not locally re-encode audio,
    /// so --sample must already be trimmed to this exact rounded duration.
    #[arg(long)]
    pub sample_duration: f64,

    /// Voice name
    #[arg(long)]
    pub name: String,

    /// Voice description
    #[arg(long)]
    pub description: Option<String>,

    /// Voice style text
    #[arg(long)]
    pub styles: Option<String>,

    /// Singer skill level used by current Suno Web
    #[arg(
        long,
        value_parser = ["Beginner", "Intermediate", "Advanced", "Professional"]
    )]
    pub singer_skill_level: Option<String>,

    /// Confirm both recordings contain only your own voice and violate no third-party rights
    #[arg(long, required = true)]
    pub confirm_rights: bool,

    /// Confirm you are at least 18 and Voice/audio upload is available in your region
    ///
    /// Suno remains authoritative and may reject an ineligible account.
    #[arg(long, required = true)]
    pub confirm_eligibility: bool,

    /// Consent to Suno collecting and processing the recordings as possible biometric data
    ///
    /// Suno's Terms, Privacy Policy, account choices, and server-side consent
    /// gate remain authoritative, including any disclosed model-training use.
    #[arg(long, required = true)]
    pub confirm_biometric_consent: bool,
}

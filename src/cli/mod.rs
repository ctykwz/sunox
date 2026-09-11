//! Clap argument schema grouped by user-facing command area.

mod agent;
mod auth;
mod browser_extension;
mod clip;
mod config;
mod create;
mod doctor;
mod library;
mod lyrics_editor;
mod lyrics_project;
mod media;
mod models;
mod persona;
mod playlist;
mod update;
mod voice;
mod wait;

pub use crate::api::types::{RemasterStyleProfile, RemasterVariation};
pub use agent::{InstallSkillArgs, SkillTarget};
pub use auth::AuthArgs;
pub use browser_extension::InstallBrowserExtensionArgs;
pub use clip::{
    ClipArgs, ClipCommand, CoverArtArgs, CoverArtCommand, CoverArtHistoryArgs, CoverArtImageArgs,
    CoverArtPromptImageArg, CoverArtPromptImageKind, CoverArtStatusArgs, CoverArtVideoArgs,
    GenerateImageArgs, GenerateVideoArgs, GetStemsArgs, VideoStatusArgs,
};
pub use config::{ConfigAction, ConfigArgs};
pub use create::{
    ConcatArgs, CoverArgs, CreateArgs, CropArgs, DescribeArgs, ExtendArgs, FadeArgs, GenerateArgs,
    InspireArgs, LyricsArgs, PaintArgs, RemasterArgs, ReuseArgs, ReverseArgs, SpeedArgs, StemGroup,
    StemMode, StemsArgs,
};
pub use doctor::DoctorArgs;
pub use library::{
    DeleteArgs, EmptyTrashArgs, InfoArgs, ListArgs, ListSort, PublishArgs, PurgeArgs, ReactionArgs,
    RestoreArgs, SearchArgs, SetArgs, StatusArgs,
};
pub use lyrics_editor::{
    LYRICS_MASHUP_DEFAULT_TIMEOUT_SECS, LyricsMashupArgs, LyricsMashupStatusArgs, LyricsRewriteArgs,
};
pub use lyrics_project::{LyricsCommand, LyricsProjectCommand, LyricsProjectsArgs};
pub use media::{DownloadArgs, DownloadFormat, TimedLyricsArgs, UploadArgs, UploadStatusArgs};
pub use models::{
    CustomModelCommand, CustomModelsArgs, ModelsArgs, ModelsCommand, RemasterModel, VocalGender,
};
pub use persona::{
    PersonaArgs, PersonaClipsArgs, PersonaCommand, PersonaCreateArgs, PersonaDeleteArgs,
    PersonaInfoArgs, PersonaListArgs, PersonaListKind, PersonaLoveArgs, PersonaPublishArgs,
    PersonaRestoreArgs, PersonaSetArgs, PersonaToggleLoveArgs,
};
pub use playlist::{
    AddArgs, PlaylistArgs, PlaylistCommand, PlaylistCreateArgs, PlaylistDeleteArgs,
    PlaylistInfoArgs, PlaylistListArgs, PlaylistPublishArgs, PlaylistReactionArgs,
    PlaylistReorderArgs, PlaylistRestoreArgs, PlaylistSaveArgs, PlaylistSetArgs,
    PlaylistTracksArgs,
};
pub use update::UpdateArgs;
pub use voice::{
    VoiceArgs, VoiceCommand, VoiceCreateArgs, VoicePhraseArgs, VoiceProcessedStatusArgs,
    VoiceVerificationStatusArgs,
};
pub use wait::WaitArgs;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "sunox",
    version,
    about = "Suno AI music generation CLI — direct Suno web workflow"
)]
pub struct Cli {
    /// Optional song description. When no subcommand is provided, this starts `sunox create`.
    pub prompt: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Override a configuration value for this invocation.
    ///
    /// Use `key=value`, for example `-c default_model=v5.5` or
    /// `-c output_dir=./songs`.
    #[arg(short = 'c', long = "config", value_name = "key=value", global = true)]
    pub config_overrides: Vec<String>,

    /// Output JSON (auto-detected when piped)
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-essential output
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Allow this invocation to run Suno write requests concurrently with other sunox processes
    #[arg(long, global = true)]
    pub parallel: bool,

    /// Refuse Suno account mutations while still allowing read-only requests
    #[arg(long, global = true)]
    pub read_only: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate music from a prompt or custom lyrics
    Create(CreateArgs),

    /// Download completed song audio
    Download(DownloadArgs),

    /// Add clip(s) to a playlist
    Add(AddArgs),

    /// Generate lyrics using current Cowrite model discovery and submit contract
    Lyrics(LyricsArgs),

    /// Manage clips
    Clip(ClipArgs),

    /// Manage voice personas
    Persona(PersonaArgs),

    /// Create and verify Voices from your own recordings
    Voice(VoiceArgs),

    /// Manage playlists
    Playlist(PlaylistArgs),

    /// Show credit balance and plan info
    Credits,

    /// List available models
    Models(ModelsArgs),

    /// Compare live account entitlements, models, and limits with CLI support
    Capabilities,

    /// Set up authentication
    Auth(AuthArgs),

    /// Log in from browser cookies, falling back to an interactive Chrome/Edge window
    Login,

    /// Remove stored authentication credentials and the interactive login profile
    Logout,

    /// Manage configuration
    Config(ConfigArgs),

    /// Diagnose local configuration and authentication
    Doctor(DoctorArgs),

    /// Machine-readable capabilities (for AI agents)
    AgentInfo,

    /// Install the agent skill (teaches Codex / coding agents how to use this CLI)
    InstallSkill(InstallSkillArgs),

    /// Extract the Chrome extension used for silent generation challenges
    InstallBrowserExtension(InstallBrowserExtensionArgs),

    /// Self-update from GitHub Releases
    Update(UpdateArgs),
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, ClipCommand, Commands, CustomModelCommand, ModelsCommand, RemasterStyleProfile,
        RemasterVariation, StemGroup, StemMode, VoiceCommand,
    };
    use clap::Parser;

    #[test]
    fn remaster_preserves_an_omitted_variation_for_model_specific_encoding() {
        let cli = Cli::try_parse_from(["sunox", "clip", "remaster", "clip-a"])
            .expect("valid remaster command");

        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Remaster(args) = clip.command else {
            panic!("expected remaster command");
        };
        assert!(args.variation.is_none());
    }

    #[test]
    fn remaster_preserves_an_explicit_variation() {
        let cli =
            Cli::try_parse_from(["sunox", "clip", "remaster", "clip-a", "--variation", "high"])
                .expect("valid remaster command");

        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Remaster(args) = clip.command else {
            panic!("expected remaster command");
        };
        assert!(matches!(args.variation, Some(RemasterVariation::High)));
    }

    #[test]
    fn remaster_accepts_the_reported_external_key_alias() {
        let cli = Cli::try_parse_from([
            "sunox",
            "clip",
            "remaster",
            "clip-a",
            "--model",
            "chirp-flounder",
        ])
        .expect("reported remaster selector must be accepted");

        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Remaster(args) = clip.command else {
            panic!("expected remaster command");
        };
        assert_eq!(
            args.model.expect("explicit model").to_api_key(),
            "chirp-flounder"
        );
    }

    #[test]
    fn remaster_accepts_v6_display_name_and_external_key() {
        for selector in ["v6", "chirp-halibut"] {
            let cli =
                Cli::try_parse_from(["sunox", "clip", "remaster", "clip-a", "--model", selector])
                    .expect("v6 remaster selector must be accepted");

            let Some(Commands::Clip(clip)) = cli.command else {
                panic!("expected clip command");
            };
            let ClipCommand::Remaster(args) = clip.command else {
                panic!("expected remaster command");
            };
            assert_eq!(
                args.model.expect("explicit model").to_api_key(),
                "chirp-halibut"
            );
        }
    }

    #[test]
    fn clip_actions_accepts_an_exact_clip_id() {
        let cli = Cli::try_parse_from(["sunox", "clip", "actions", "clip-a"])
            .expect("valid clip actions command");

        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Actions(args) = clip.command else {
            panic!("expected actions command");
        };
        assert_eq!(args.id, "clip-a");
    }

    #[test]
    fn create_accepts_account_model_selector_and_duration() {
        let cli = Cli::try_parse_from([
            "sunox",
            "create",
            "future bass",
            "--model",
            "My Custom Model",
            "--duration",
            "245.5",
        ])
        .expect("dynamic generation model selector");

        let Some(Commands::Create(args)) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(args.model.as_deref(), Some("My Custom Model"));
        assert_eq!(args.duration, Some(245.5));
    }

    #[test]
    fn create_accepts_v6_custom_controls() {
        let cli = Cli::try_parse_from([
            "sunox",
            "create",
            "--mumble",
            "--model",
            "v6",
            "--duration",
            "180",
            "--variety",
            "3",
            "--max-mode",
        ])
        .expect("v6 Custom controls");

        let Some(Commands::Create(args)) = cli.command else {
            panic!("expected create command");
        };
        assert!(args.mumble);
        assert!(args.max_mode);
        assert_eq!(args.variety, Some(3));
        assert_eq!(args.duration, Some(180.0));
    }

    #[test]
    fn clip_reuse_and_paint_commands_parse_protocol_controls() {
        let reuse = Cli::try_parse_from([
            "sunox",
            "clip",
            "reuse",
            "source-1",
            "--lyrics",
            "new lyrics",
            "--variety",
            "2",
        ])
        .expect("reuse command");
        let Some(Commands::Clip(clip)) = reuse.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Reuse(args) = clip.command else {
            panic!("expected reuse command");
        };
        assert_eq!(args.clip_id, "source-1");
        assert_eq!(args.lyrics.as_deref(), Some("new lyrics"));
        assert_eq!(args.variety, Some(2));

        for command in ["underpaint", "overpaint"] {
            let parsed = Cli::try_parse_from([
                "sunox",
                "clip",
                command,
                "source-1",
                "--model",
                "v6",
                "--tags",
                "chamber pop",
            ])
            .expect("paint command");
            let Some(Commands::Clip(clip)) = parsed.command else {
                panic!("expected clip command");
            };
            match clip.command {
                ClipCommand::Underpaint(args) | ClipCommand::Overpaint(args) => {
                    assert_eq!(args.clip_id, "source-1");
                    assert_eq!(args.model.as_deref(), Some("v6"));
                    assert_eq!(args.tags.as_deref(), Some("chamber pop"));
                }
                _ => panic!("expected paint command"),
            }
        }
    }

    #[test]
    fn remaster_accepts_v6_variation_and_style_profile() {
        let cli = Cli::try_parse_from([
            "sunox",
            "clip",
            "remaster",
            "clip-a",
            "--model",
            "v6",
            "--variation",
            "high",
            "--style-profile",
            "clarity",
        ])
        .expect("v6 Remaster controls");

        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Remaster(args) = clip.command else {
            panic!("expected remaster command");
        };
        assert!(matches!(args.variation, Some(RemasterVariation::High)));
        assert!(matches!(
            args.style_profile,
            Some(RemasterStyleProfile::Clarity)
        ));
    }

    #[test]
    fn create_accepts_a_lyrics_project_only_with_explicit_custom_lyrics() {
        let cli = Cli::try_parse_from([
            "sunox",
            "create",
            "--lyrics",
            "[Verse]\nhello",
            "--lyrics-project-id",
            "project-1",
        ])
        .expect("lyrics project custom generation");

        let Some(Commands::Create(args)) = cli.command else {
            panic!("expected create command");
        };
        assert_eq!(args.lyrics_project_id.as_deref(), Some("project-1"));
    }

    #[test]
    fn cover_accepts_account_model_id() {
        let cli = Cli::try_parse_from([
            "sunox",
            "clip",
            "cover",
            "clip-a",
            "--model",
            "model-account-7",
        ])
        .expect("dynamic cover model selector");

        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Cover(args) = clip.command else {
            panic!("expected cover command");
        };
        assert_eq!(args.model.as_deref(), Some("model-account-7"));
    }

    #[test]
    fn custom_model_training_parses_both_confirmations_and_clip_ids() {
        let cli = Cli::try_parse_from([
            "sunox",
            "models",
            "custom",
            "train",
            "--name",
            "My Sound",
            "--confirm-rights",
            "--confirm-ui-available",
            "clip-1",
            "clip-2",
            "clip-3",
            "clip-4",
            "clip-5",
            "clip-6",
        ])
        .expect("valid custom model training command");

        let Some(Commands::Models(models)) = cli.command else {
            panic!("expected models command");
        };
        let Some(ModelsCommand::Custom(custom)) = models.command else {
            panic!("expected custom models command");
        };
        let CustomModelCommand::Train(args) = custom.command else {
            panic!("expected train command");
        };
        assert_eq!(args.name, "My Sound");
        assert!(args.confirm_rights);
        assert!(args.confirm_ui_available);
        assert_eq!(args.clip_ids.len(), 6);
    }

    #[test]
    fn custom_model_delete_alias_still_requires_confirmation_in_the_command_payload() {
        let cli = Cli::try_parse_from(["sunox", "models", "custom", "delete", "model-1", "-y"])
            .expect("valid custom model delete alias");

        let Some(Commands::Models(models)) = cli.command else {
            panic!("expected models command");
        };
        let Some(ModelsCommand::Custom(custom)) = models.command else {
            panic!("expected custom models command");
        };
        let CustomModelCommand::Archive(args) = custom.command else {
            panic!("expected archive command");
        };
        assert_eq!(args.id, "model-1");
        assert!(args.yes);
    }

    #[test]
    fn stems_preserves_the_legacy_auto_split_default() {
        let cli = Cli::try_parse_from(["sunox", "clip", "stems", "clip-a"])
            .expect("legacy auto split command");
        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Stems(args) = clip.command else {
            panic!("expected stems command");
        };
        assert_eq!(args.mode, StemMode::Auto);
        assert!(args.stem.is_none());
    }

    #[test]
    fn stems_accepts_the_current_split_from_mix_shape() {
        let cli = Cli::try_parse_from([
            "sunox",
            "clip",
            "stems",
            "clip-a",
            "--mode",
            "split-from-mix",
            "--stem",
            "vocals",
        ])
        .expect("split from mix command");
        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::Stems(args) = clip.command else {
            panic!("expected stems command");
        };
        assert_eq!(args.mode, StemMode::Split);
        assert_eq!(args.stem, Some(StemGroup::Vocals));
    }

    #[test]
    fn get_stems_can_request_existing_wav_exports_without_conversion() {
        let cli = Cli::try_parse_from([
            "sunox",
            "clip",
            "get-stems",
            "clip-a",
            "--page",
            "1",
            "--download",
            "--format",
            "wav",
            "--no-convert",
        ])
        .expect("existing stem export command");
        let Some(Commands::Clip(clip)) = cli.command else {
            panic!("expected clip command");
        };
        let ClipCommand::GetStems(args) = clip.command else {
            panic!("expected get-stems command");
        };
        assert_eq!(args.clip_id, "clip-a");
        assert_eq!(args.page, Some(1));
        assert!(args.download);
        assert!(args.no_convert);
    }

    #[test]
    fn voice_phrase_defaults_to_english() {
        let cli = Cli::try_parse_from(["sunox", "voice", "phrase"]).expect("voice phrase command");
        let Some(Commands::Voice(voice)) = cli.command else {
            panic!("expected voice command");
        };
        let VoiceCommand::Phrase(args) = voice.command else {
            panic!("expected phrase command");
        };
        assert_eq!(args.language, "en");
    }

    #[test]
    fn voice_create_requires_both_files_phrase_duration_and_rights() {
        let cli = Cli::try_parse_from([
            "sunox",
            "voice",
            "create",
            "--sample",
            "sample.wav",
            "--verification",
            "phrase.wav",
            "--phrase-id",
            "phrase-1",
            "--sample-duration",
            "42.35",
            "--name",
            "My Voice",
            "--confirm-rights",
            "--confirm-eligibility",
            "--confirm-biometric-consent",
        ])
        .expect("voice create command");
        let Some(Commands::Voice(voice)) = cli.command else {
            panic!("expected voice command");
        };
        let VoiceCommand::Create(args) = voice.command else {
            panic!("expected voice create command");
        };
        assert_eq!(args.sample.to_string_lossy(), "sample.wav");
        assert_eq!(args.verification.to_string_lossy(), "phrase.wav");
        assert_eq!(args.phrase_id, "phrase-1");
        assert_eq!(args.sample_duration, 42.35);
        assert_eq!(args.language, "en");
        assert!(args.confirm_rights);
        assert!(args.confirm_eligibility);
        assert!(args.confirm_biometric_consent);

        assert!(
            Cli::try_parse_from([
                "sunox",
                "voice",
                "create",
                "--sample",
                "sample.wav",
                "--verification",
                "phrase.wav",
                "--phrase-id",
                "phrase-1",
                "--sample-duration",
                "42.35",
                "--name",
                "My Voice",
            ])
            .is_err(),
            "rights confirmation must be explicit"
        );

        assert!(
            Cli::try_parse_from([
                "sunox",
                "voice",
                "create",
                "--sample",
                "sample.wav",
                "--verification",
                "phrase.wav",
                "--phrase-id",
                "phrase-1",
                "--sample-duration",
                "42.35",
                "--name",
                "My Voice",
                "--confirm-rights",
            ])
            .is_err(),
            "age and regional eligibility confirmation must be explicit"
        );

        assert!(
            Cli::try_parse_from([
                "sunox",
                "voice",
                "create",
                "--sample",
                "sample.wav",
                "--verification",
                "phrase.wav",
                "--phrase-id",
                "phrase-1",
                "--sample-duration",
                "42.35",
                "--name",
                "My Voice",
                "--confirm-rights",
                "--confirm-eligibility",
            ])
            .is_err(),
            "biometric processing consent must be explicit"
        );
    }
}

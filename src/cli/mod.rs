//! Clap argument schema grouped by user-facing command area.

mod agent;
mod auth;
mod browser_extension;
mod clip;
mod config;
mod create;
mod doctor;
mod library;
mod media;
mod models;
mod persona;
mod playlist;
mod update;
mod wait;

pub use crate::api::types::RemasterVariation;
pub use agent::{InstallSkillArgs, SkillTarget};
pub use auth::AuthArgs;
pub use browser_extension::InstallBrowserExtensionArgs;
pub use clip::{ClipArgs, ClipCommand};
pub use config::{ConfigAction, ConfigArgs};
pub use create::{
    ConcatArgs, CoverArgs, CreateArgs, CropArgs, DescribeArgs, ExtendArgs, FadeArgs, GenerateArgs,
    InspireArgs, LyricsArgs, RemasterArgs, ReverseArgs, SpeedArgs, StemsArgs,
};
pub use doctor::DoctorArgs;
pub use library::{
    DeleteArgs, EmptyTrashArgs, InfoArgs, ListArgs, ListSort, PublishArgs, PurgeArgs, ReactionArgs,
    RestoreArgs, SearchArgs, SetArgs, StatusArgs,
};
pub use media::{DownloadArgs, DownloadFormat, TimedLyricsArgs, UploadArgs, UploadStatusArgs};
pub use models::{RemasterModel, VocalGender};
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

    /// Manage playlists
    Playlist(PlaylistArgs),

    /// Show credit balance and plan info
    Credits,

    /// List available models
    Models,

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
    use super::{Cli, ClipCommand, Commands, RemasterVariation};
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
}

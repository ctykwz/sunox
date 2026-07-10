//! Clap argument schema grouped by user-facing command area.

mod agent;
mod auth;
mod clip;
mod config;
mod create;
mod library;
mod media;
mod models;
mod persona;
mod playlist;
mod update;
mod wait;

pub use agent::{InstallSkillArgs, SkillTarget};
pub use auth::AuthArgs;
pub use clip::{ClipArgs, ClipCommand};
pub use config::{ConfigAction, ConfigArgs};
pub use create::{
    ConcatArgs, CoverArgs, CreateArgs, CropArgs, DescribeArgs, ExtendArgs, FadeArgs, GenerateArgs,
    LyricsArgs, RemasterArgs, ReverseArgs, SpeedArgs, StemsArgs,
};
pub use library::{
    DeleteArgs, EmptyTrashArgs, InfoArgs, ListArgs, ListSort, PublishArgs, PurgeArgs, ReactionArgs,
    RestoreArgs, SearchArgs, SetArgs, StatusArgs,
};
pub use media::{DownloadArgs, DownloadFormat, TimedLyricsArgs, UploadArgs, UploadStatusArgs};
pub use models::{ModelVersion, RemasterModel, VocalGender};
pub use persona::{
    PersonaArgs, PersonaClipsArgs, PersonaCommand, PersonaCreateArgs, PersonaDeleteArgs,
    PersonaInfoArgs, PersonaListArgs, PersonaListKind, PersonaLoveArgs, PersonaProcessedClipArgs,
    PersonaPublishArgs, PersonaRestoreArgs, PersonaSetArgs, PersonaToggleLoveArgs,
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
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate music from a prompt or custom lyrics
    Create(CreateArgs),

    /// Download completed song audio
    Download(DownloadArgs),

    /// Add clip(s) to a playlist
    Add(AddArgs),

    /// Generate lyrics only (free, no credits used)
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

    /// Set up authentication
    Auth(AuthArgs),

    /// Log in from browser cookies, falling back to an interactive Chrome/Edge window
    Login,

    /// Remove stored authentication credentials and the interactive login profile
    Logout,

    /// Manage configuration
    Config(ConfigArgs),

    /// Diagnose local configuration and authentication
    Doctor,

    /// Machine-readable capabilities (for AI agents)
    AgentInfo,

    /// Install the agent skill (teaches Codex / coding agents how to use this CLI)
    InstallSkill(InstallSkillArgs),

    /// Self-update from GitHub Releases
    Update(UpdateArgs),
}

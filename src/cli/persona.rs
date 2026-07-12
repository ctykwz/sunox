use clap::{Subcommand, ValueEnum};

#[derive(clap::Args)]
pub struct PersonaArgs {
    #[command(subcommand)]
    pub command: PersonaCommand,
}

#[derive(Subcommand)]
pub enum PersonaCommand {
    /// List voice personas
    List(PersonaListArgs),

    /// Show voice persona details
    Info(PersonaInfoArgs),

    /// List songs attached to a voice persona
    Clips(PersonaClipsArgs),

    /// Create a voice persona from an existing clip
    Create(Box<PersonaCreateArgs>),

    /// Update voice persona metadata
    Set(PersonaSetArgs),

    /// Show processed vocal clip status
    ProcessedClip(PersonaProcessedClipArgs),

    /// Make a voice persona public
    Publish(PersonaPublishArgs),

    /// Make a voice persona private
    Unpublish(PersonaPublishArgs),

    /// Ensure a voice persona is loved/favorited
    Love(PersonaLoveArgs),

    /// Ensure a voice persona is not loved/favorited
    Unlove(PersonaLoveArgs),

    /// Toggle loved/favorite state for a voice persona
    ToggleLove(PersonaToggleLoveArgs),

    /// Move a voice persona to trash
    Delete(PersonaDeleteArgs),

    /// Restore a trashed voice persona
    Restore(PersonaRestoreArgs),

    /// Permanently delete a trashed voice persona
    Purge(PersonaDeleteArgs),
}

#[derive(clap::Args)]
pub struct PersonaListArgs {
    /// Persona collection to list
    #[arg(long, value_enum, default_value_t = PersonaListKind::Mine)]
    pub kind: PersonaListKind,

    /// Page number
    #[arg(long, default_value_t = 1)]
    pub page: u32,

    /// Continuation token from a previous response
    #[arg(long)]
    pub continuation_token: Option<String>,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum PersonaListKind {
    /// Personas created by the authenticated user
    Mine,

    /// Personas loved/favorited by the authenticated user
    Loved,

    /// Personas from followed creators
    Followed,
}

#[derive(clap::Args)]
pub struct PersonaInfoArgs {
    /// Persona ID to inspect
    pub id: String,
}

#[derive(clap::Args)]
pub struct PersonaClipsArgs {
    /// Persona ID to inspect
    pub id: String,

    /// Page number
    #[arg(long, default_value_t = 1)]
    pub page: u32,
}

#[derive(clap::Args)]
pub struct PersonaCreateArgs {
    /// Root clip ID to create the persona from
    pub root_clip_id: String,

    /// Persona name
    #[arg(long)]
    pub name: Option<String>,

    /// Persona description
    #[arg(long)]
    pub description: Option<String>,

    /// Existing Suno image S3 ID for persona artwork
    #[arg(long)]
    pub image_s3_id: Option<String>,

    /// Explicitly make the new persona public (default: private)
    #[arg(long)]
    pub public: bool,

    /// Persona type to pass through to Suno Web
    #[arg(long)]
    pub persona_type: Option<String>,

    /// Optional vox audio ID used by Suno Web persona creation
    #[arg(long)]
    pub vox_audio_id: Option<String>,

    /// Vocal range start in seconds
    #[arg(long)]
    pub vocal_start: Option<f64>,

    /// Vocal range end in seconds
    #[arg(long)]
    pub vocal_end: Option<f64>,

    /// User-input style text
    #[arg(long)]
    pub user_input_styles: Option<String>,

    /// Source marker to pass through to Suno Web
    #[arg(long)]
    pub source: Option<String>,

    /// Singer skill level marker to pass through to Suno Web
    #[arg(long)]
    pub singer_skill_level: Option<String>,
}

#[derive(clap::Args)]
pub struct PersonaSetArgs {
    /// Persona ID to update
    pub id: String,

    /// New persona name
    #[arg(long)]
    pub name: Option<String>,

    /// New persona description
    #[arg(long)]
    pub description: Option<String>,

    /// Set public/private visibility with the edit endpoint
    #[arg(long)]
    pub public: Option<bool>,

    /// Persona type to pass through to Suno Web
    #[arg(long)]
    pub persona_type: Option<String>,

    /// User-input style text
    #[arg(long)]
    pub user_input_styles: Option<String>,

    /// Processed vocal audio ID
    #[arg(long)]
    pub vox_audio_id: Option<String>,

    /// Vocal range start in seconds
    #[arg(long)]
    pub vocal_start: Option<f64>,

    /// Vocal range end in seconds
    #[arg(long)]
    pub vocal_end: Option<f64>,
}

#[derive(clap::Args)]
pub struct PersonaProcessedClipArgs {
    /// Processed clip ID to inspect
    pub id: String,
}

#[derive(clap::Args)]
pub struct PersonaLoveArgs {
    /// Persona ID to update
    pub id: String,
}

#[derive(clap::Args)]
pub struct PersonaPublishArgs {
    /// Persona ID to update
    pub id: String,
}

#[derive(clap::Args)]
pub struct PersonaToggleLoveArgs {
    /// Persona ID to toggle
    pub id: String,
}

#[derive(clap::Args)]
pub struct PersonaDeleteArgs {
    /// Persona ID to delete or purge
    pub id: String,

    /// Confirm this destructive action
    #[arg(short = 'y', long)]
    pub yes: bool,
}

#[derive(clap::Args)]
pub struct PersonaRestoreArgs {
    /// Persona ID to restore
    pub id: String,
}

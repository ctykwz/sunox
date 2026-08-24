use clap::{Subcommand, ValueEnum};

#[derive(clap::Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub command: Option<ModelsCommand>,
}

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// Train and manage account-scoped Custom Models
    Custom(CustomModelsArgs),
}

#[derive(clap::Args)]
pub struct CustomModelsArgs {
    #[command(subcommand)]
    pub command: CustomModelCommand,
}

#[derive(Subcommand)]
pub enum CustomModelCommand {
    /// Show models that are still training
    Pending,

    /// Train after explicitly confirming the current account's Web UI exposes training
    Train(CustomModelTrainArgs),

    /// Archive a Custom Model
    #[command(visible_alias = "delete")]
    Archive(CustomModelArchiveArgs),
}

#[derive(clap::Args)]
pub struct CustomModelTrainArgs {
    /// Custom Model name (1-16 Unicode characters)
    #[arg(long)]
    pub name: String,

    /// Confirm that you own the rights to every selected clip
    #[arg(long)]
    pub confirm_rights: bool,

    /// Attest that the current Suno Web account visibly exposes training; server stays authoritative
    #[arg(long)]
    pub confirm_ui_available: bool,

    /// 6-100 distinct Suno clip IDs (Artist accounts can use Web for up to 200)
    #[arg(value_name = "CLIP_ID", num_args = 6..=100)]
    pub clip_ids: Vec<String>,
}

#[derive(clap::Args)]
pub struct CustomModelArchiveArgs {
    /// Custom Model ID
    pub id: String,

    /// Confirm this destructive action
    #[arg(short = 'y', long)]
    pub yes: bool,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum VocalGender {
    Male,
    Female,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum RemasterModel {
    #[value(name = "v5.5", alias = "chirp-flounder")]
    #[default]
    V55,
    #[value(name = "v5", alias = "chirp-carp")]
    V5,
    #[value(name = "v4.5+", alias = "chirp-bass")]
    V45Plus,
}

impl RemasterModel {
    pub fn to_api_key(&self) -> &'static str {
        match self {
            Self::V55 => "chirp-flounder",
            Self::V5 => "chirp-carp",
            Self::V45Plus => "chirp-bass",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::V55 => "v5.5",
            Self::V5 => "v5",
            Self::V45Plus => "v4.5+",
        }
    }

    pub fn supports_api_key(key: &str) -> bool {
        matches!(key, "chirp-flounder" | "chirp-carp" | "chirp-bass")
    }
}

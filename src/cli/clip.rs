use clap::Subcommand;

use super::{
    ConcatArgs, CoverArgs, CropArgs, DeleteArgs, DownloadArgs, EmptyTrashArgs, ExtendArgs,
    FadeArgs, InfoArgs, InspireArgs, ListArgs, PublishArgs, PurgeArgs, ReactionArgs, RemasterArgs,
    RestoreArgs, ReverseArgs, SearchArgs, SetArgs, SpeedArgs, StatusArgs, StemsArgs,
    TimedLyricsArgs, UploadArgs, UploadStatusArgs, WaitArgs,
};
use crate::api::download::DownloadFormat;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverArtMedia {
    Image,
    Video,
}

impl CoverArtMedia {
    pub fn as_protocol(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverArtPromptImageKind {
    Uploaded,
    Generated,
    S3Filename,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverArtPromptImageArg {
    pub kind: CoverArtPromptImageKind,
    pub id: String,
}

impl std::str::FromStr for CoverArtPromptImageArg {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, id) = value
            .split_once(':')
            .ok_or_else(|| "expected <uploaded|generated|s3_filename>:<id>".to_string())?;
        let kind = match kind {
            "uploaded" => CoverArtPromptImageKind::Uploaded,
            "generated" => CoverArtPromptImageKind::Generated,
            "s3_filename" => CoverArtPromptImageKind::S3Filename,
            _ => {
                return Err("prompt image type must be uploaded, generated, or s3_filename".into());
            }
        };
        if id.trim().is_empty() {
            return Err("prompt image ID must not be empty".into());
        }
        Ok(Self {
            kind,
            id: id.to_string(),
        })
    }
}

#[derive(clap::Args)]
pub struct CoverArtArgs {
    #[command(subcommand)]
    pub command: CoverArtCommand,
}

#[derive(Subcommand)]
pub enum CoverArtCommand {
    /// List current server-provided image/video models and allowed durations
    Models,
    /// List recoverable pending batch descriptors
    Pending,
    /// Read generated-media history
    History(CoverArtHistoryArgs),
    /// Submit a two-result image batch; does not apply either result
    Image(CoverArtImageArgs),
    /// Submit a two-result video batch; does not apply either result
    Video(CoverArtVideoArgs),
    /// Read or wait for one existing batch
    Status(CoverArtStatusArgs),
    /// Explicitly apply one generated image result to a clip
    ApplyImage(CoverArtApplyImageArgs),
    /// Explicitly apply one generated video upload to a clip
    ApplyVideo(CoverArtApplyVideoArgs),
}

#[derive(clap::Args)]
pub struct CoverArtHistoryArgs {
    #[arg(long)]
    pub clip_id: Option<String>,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long)]
    pub favorites: bool,
    #[arg(long, value_enum)]
    pub media: Option<CoverArtMedia>,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub limit: u32,
}

#[derive(clap::Args)]
pub struct CoverArtImageArgs {
    pub id: String,
    #[arg(long)]
    pub prompt: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub prompt_image: Option<CoverArtPromptImageArg>,
    #[arg(long)]
    pub no_wait: bool,
    #[arg(long, conflicts_with = "no_wait")]
    pub timeout: Option<u64>,
}

#[derive(clap::Args)]
pub struct CoverArtVideoArgs {
    pub id: String,
    #[arg(long)]
    pub prompt: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub duration: Option<u32>,
    #[arg(long)]
    pub start_image: Option<CoverArtPromptImageArg>,
    #[arg(long)]
    pub no_wait: bool,
    #[arg(long, conflicts_with = "no_wait")]
    pub timeout: Option<u64>,
}

#[derive(clap::Args)]
pub struct CoverArtStatusArgs {
    pub batch_id: String,
    #[arg(long, value_enum)]
    pub media: CoverArtMedia,
    #[arg(long)]
    pub wait: bool,
    #[arg(long, requires = "wait")]
    pub timeout: Option<u64>,
}

#[derive(clap::Args)]
pub struct CoverArtApplyImageArgs {
    pub id: String,
    pub batch_id: String,
    pub image_id: String,
}

#[derive(clap::Args)]
pub struct CoverArtApplyVideoArgs {
    pub id: String,
    pub batch_id: String,
    pub video_upload_id: String,
}

#[derive(clap::Args)]
pub struct ClipArgs {
    #[command(subcommand)]
    pub command: ClipCommand,
}

#[derive(clap::Args)]
pub struct GenerateImageArgs {
    /// Clip ID whose cover image will be replaced
    pub id: String,

    /// Image-generation prompt (1-200 Unicode characters)
    #[arg(long)]
    pub prompt: String,
}

#[derive(clap::Args)]
pub struct GenerateVideoArgs {
    /// Clip ID whose downloadable video should be generated or regenerated
    pub id: String,

    /// Maximum seconds to wait for a terminal video status
    #[arg(long)]
    pub timeout: Option<u64>,
}

#[derive(clap::Args)]
pub struct VideoStatusArgs {
    /// Clip ID whose video status should be read
    pub id: String,

    /// Poll until the video reaches a terminal status
    #[arg(long)]
    pub wait: bool,

    /// Maximum seconds to wait when --wait is used
    #[arg(long, requires = "wait")]
    pub timeout: Option<u64>,
}

#[derive(Subcommand)]
pub enum ClipCommand {
    /// List your songs
    List(ListArgs),

    /// Search your songs by title or tags
    Search(SearchArgs),

    /// Show detailed info for a single clip
    Info(InfoArgs),

    /// Show server-provided actions for a single clip
    Actions(InfoArgs),

    /// Check generation status
    Status(StatusArgs),

    /// Wait for generated clip(s) to finish
    Wait(WaitArgs),

    /// Download audio/video for clip(s)
    Download(DownloadArgs),

    /// Upload a local audio file into your Suno library
    Upload(UploadArgs),

    /// Show processing status for an existing audio upload
    UploadStatus(UploadStatusArgs),

    /// Delete/trash a clip
    Delete(DeleteArgs),

    /// Restore clip(s) from trash
    Restore(RestoreArgs),

    /// Permanently delete trashed clip(s)
    Purge(PurgeArgs),

    /// Permanently delete every clip currently in trash
    EmptyTrash(EmptyTrashArgs),

    /// Like clip(s), or clear likes with --clear
    Like(ReactionArgs),

    /// Dislike clip(s), or clear dislikes with --clear
    Dislike(ReactionArgs),

    /// Update clip metadata and cover
    Set(SetArgs),

    /// Toggle clip public/private
    Publish(PublishArgs),

    /// Get word-level timestamped lyrics
    TimedLyrics(TimedLyricsArgs),

    /// Continue/extend a clip from a timestamp
    Extend(ExtendArgs),

    /// Concatenate clips into a full song
    Concat(ConcatArgs),

    /// Create a cover of an existing clip
    Cover(CoverArgs),

    /// Generate a new song using a clip as loose inspiration
    Inspire(InspireArgs),

    /// Remaster a clip with a different model
    Remaster(RemasterArgs),

    /// Adjust playback speed for a clip
    Speed(SpeedArgs),

    /// Reverse a clip
    Reverse(ReverseArgs),

    /// Crop a clip or remove a section
    Crop(CropArgs),

    /// Apply fade in and/or fade out
    Fade(FadeArgs),

    /// Extract stems (vocals, instruments) from a clip
    Stems(StemsArgs),

    /// List or download existing Web Get Stems results without starting extraction
    GetStems(GetStemsArgs),

    /// Generate an image from a prompt and apply it to a clip
    GenerateImage(GenerateImageArgs),

    /// Validate legacy video generation (submit is blocked until clip eligibility is provable)
    GenerateVideo(GenerateVideoArgs),

    /// Read or wait for clip video-generation status
    VideoStatus(VideoStatusArgs),

    /// Generate, inspect, recover, and explicitly apply current batch cover art
    CoverArt(CoverArtArgs),
}

#[cfg(test)]
mod visual_tests {
    use clap::Parser;

    use super::{ClipCommand, CoverArtCommand};
    use crate::cli::{Cli, Commands};

    #[test]
    fn clip_visual_commands_keep_generation_and_status_explicit() {
        let image = Cli::try_parse_from([
            "sunox",
            "clip",
            "generate-image",
            "clip-1",
            "--prompt",
            "neon rain",
        ])
        .expect("image generation command");
        let Some(Commands::Clip(clip)) = image.command else {
            panic!("expected clip command");
        };
        let ClipCommand::GenerateImage(args) = clip.command else {
            panic!("expected generate-image command");
        };
        assert_eq!(args.id, "clip-1");
        assert_eq!(args.prompt, "neon rain");

        let video = Cli::try_parse_from(["sunox", "clip", "generate-video", "clip-1"])
            .expect("video generation command");
        let Some(Commands::Clip(clip)) = video.command else {
            panic!("expected clip command");
        };
        assert!(matches!(clip.command, ClipCommand::GenerateVideo(_)));

        let status = Cli::try_parse_from([
            "sunox",
            "clip",
            "video-status",
            "clip-1",
            "--wait",
            "--timeout",
            "60",
        ])
        .expect("video status wait command");
        let Some(Commands::Clip(clip)) = status.command else {
            panic!("expected clip command");
        };
        let ClipCommand::VideoStatus(args) = clip.command else {
            panic!("expected video-status command");
        };
        assert!(args.wait);
        assert_eq!(args.timeout, Some(60));
    }

    #[test]
    fn cover_art_commands_keep_submit_status_and_apply_separate() {
        let image = Cli::try_parse_from([
            "sunox",
            "clip",
            "cover-art",
            "image",
            "clip-1",
            "--prompt",
            "neon rain",
            "--model",
            "image-v1",
            "--prompt-image",
            "uploaded:upload-1",
            "--no-wait",
        ])
        .expect("batch image command");
        let Some(Commands::Clip(clip)) = image.command else {
            panic!("expected clip command");
        };
        let ClipCommand::CoverArt(cover_art) = clip.command else {
            panic!("expected cover-art command");
        };
        let CoverArtCommand::Image(args) = cover_art.command else {
            panic!("expected cover-art image command");
        };
        assert_eq!(args.id, "clip-1");
        assert_eq!(args.model.as_deref(), Some("image-v1"));
        assert_eq!(args.prompt_image.expect("prompt image").id, "upload-1");
        assert!(args.no_wait);

        let status = Cli::try_parse_from([
            "sunox",
            "clip",
            "cover-art",
            "status",
            "batch-1",
            "--media",
            "video",
            "--wait",
            "--timeout",
            "90",
        ])
        .expect("batch status command");
        let Some(Commands::Clip(clip)) = status.command else {
            panic!("expected clip command");
        };
        let ClipCommand::CoverArt(cover_art) = clip.command else {
            panic!("expected cover-art command");
        };
        assert!(matches!(cover_art.command, CoverArtCommand::Status(_)));

        Cli::try_parse_from([
            "sunox",
            "clip",
            "cover-art",
            "apply-image",
            "clip-1",
            "batch-image",
            "image-1",
        ])
        .expect("explicit image apply command");
        Cli::try_parse_from([
            "sunox",
            "clip",
            "cover-art",
            "apply-video",
            "clip-1",
            "batch-video",
            "upload-video-1",
        ])
        .expect("explicit video apply command");
    }
}

#[derive(clap::Args)]
pub struct GetStemsArgs {
    /// Source clip whose existing stem banks should be read
    pub clip_id: String,

    /// Read one zero-based result page instead of every stored bank
    #[arg(long)]
    pub page: Option<u32>,

    /// Download returned stems; Suno may meter downloads and convert missing WAV/OPUS files
    ///
    /// Stem MP3 downloads never request aligned-lyrics generation.
    #[arg(long)]
    pub download: bool,

    /// Output directory used with --download
    #[arg(short, long, requires = "download")]
    pub output: Option<String>,

    /// Audio format used with --download (default: prepared MP3)
    #[arg(long, value_enum, requires = "download")]
    pub format: Option<DownloadFormat>,

    /// Replace existing local files with the same stem clip IDs
    #[arg(long, requires = "download")]
    pub force: bool,

    /// Refuse missing server-side WAV/OPUS conversion
    #[arg(long, requires = "download")]
    pub no_convert: bool,
}

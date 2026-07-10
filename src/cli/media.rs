pub use crate::api::download::DownloadFormat;

#[derive(clap::Args)]
pub struct UploadArgs {
    /// Local audio file to upload
    pub file: String,

    /// Suno upload type value
    #[arg(long, default_value = "file_upload")]
    pub upload_type: String,

    /// Mark the uploaded audio as a stem mix
    #[arg(long)]
    pub stem_mix: bool,

    /// Optional clip title to set after initialization
    #[arg(short, long)]
    pub title: Option<String>,

    /// Optional lyrics to set after initialization
    #[arg(long, conflicts_with = "lyrics_file")]
    pub lyrics: Option<String>,

    /// Read optional lyrics from a file
    #[arg(long)]
    pub lyrics_file: Option<String>,

    /// Max wait time for Suno upload processing, in seconds
    #[arg(long)]
    pub timeout: Option<u64>,
}

#[derive(clap::Args)]
pub struct UploadStatusArgs {
    /// Suno audio upload ID
    pub upload_id: String,
}

#[derive(clap::Args)]
pub struct DownloadArgs {
    /// Clip ID(s) to download
    pub ids: Vec<String>,

    /// Output directory
    #[arg(short, long)]
    pub output: Option<String>,

    /// Replace an existing downloaded file with the same clip ID and format
    #[arg(long)]
    pub force: bool,

    /// Download video instead of audio
    #[arg(long)]
    pub video: bool,

    /// Audio format to download through Suno's web download endpoints
    #[arg(long, value_enum)]
    pub format: Option<DownloadFormat>,
}

#[derive(clap::Args)]
pub struct TimedLyricsArgs {
    /// Clip ID
    pub id: String,

    /// Output as LRC format
    #[arg(long)]
    pub lrc: bool,
}

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
    /// Clip ID(s) to download; locked sources are authorized once unless --read-only is set
    pub ids: Vec<String>,

    /// Output directory
    #[arg(short, long)]
    pub output: Option<String>,

    /// Replace an existing downloaded file with the same clip ID and format
    #[arg(long)]
    pub force: bool,

    /// Download prepared MP4 video instead of audio
    #[arg(long)]
    pub video: bool,

    /// Audio format; MP3/M4A/WAV are prepared-first and OPUS is legacy compatibility
    #[arg(long, value_enum)]
    pub format: Option<DownloadFormat>,

    /// Refuse legacy server-side WAV/OPUS conversion when no converted file exists
    #[arg(long, conflicts_with = "video")]
    pub no_convert: bool,

    /// Internal safety switch for media such as stems that must never trigger
    /// aligned-lyrics generation while downloading an MP3.
    #[arg(skip)]
    pub skip_timed_lyrics: bool,
}

#[derive(clap::Args)]
pub struct TimedLyricsArgs {
    /// Clip ID
    pub id: String,

    /// Output as LRC format
    #[arg(long)]
    pub lrc: bool,
}

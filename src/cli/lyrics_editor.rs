use std::path::PathBuf;

pub const LYRICS_MASHUP_DEFAULT_TIMEOUT_SECS: u64 = 150;

#[derive(clap::Args)]
pub struct LyricsRewriteArgs {
    /// Instruction for rewriting or enhancing the selected lyrics
    #[arg(long)]
    pub prompt: String,

    /// Lyrics before the selected text
    #[arg(long, conflicts_with = "prefix_file")]
    pub prefix: Option<String>,

    /// Read the lyrics before the selection from a UTF-8 file
    #[arg(long, conflicts_with = "prefix")]
    pub prefix_file: Option<PathBuf>,

    /// Selected lyrics to replace (may be empty when continuing after the prefix)
    #[arg(long, conflicts_with = "edit_file")]
    pub edit: Option<String>,

    /// Read the selected lyrics from a UTF-8 file
    #[arg(long, conflicts_with = "edit")]
    pub edit_file: Option<PathBuf>,

    /// Lyrics after the selected text
    #[arg(long, conflicts_with = "suffix_file")]
    pub suffix: Option<String>,

    /// Read the lyrics after the selection from a UTF-8 file
    #[arg(long, conflicts_with = "suffix")]
    pub suffix_file: Option<PathBuf>,

    /// Song title supplied to the rewrite model
    #[arg(long, default_value = "")]
    pub title: String,

    /// Reuse a Create session token; defaults to a fresh UUID for this command
    #[arg(long)]
    pub session_token: Option<String>,
}

#[derive(clap::Args)]
pub struct LyricsMashupArgs {
    /// First lyrics source
    #[arg(
        long,
        conflicts_with = "lyrics_a_file",
        required_unless_present = "lyrics_a_file"
    )]
    pub lyrics_a: Option<String>,

    /// Read the first lyrics source from a UTF-8 file
    #[arg(
        long,
        conflicts_with = "lyrics_a",
        required_unless_present = "lyrics_a"
    )]
    pub lyrics_a_file: Option<PathBuf>,

    /// Second lyrics source
    #[arg(
        long,
        conflicts_with = "lyrics_b_file",
        required_unless_present = "lyrics_b_file"
    )]
    pub lyrics_b: Option<String>,

    /// Read the second lyrics source from a UTF-8 file
    #[arg(
        long,
        conflicts_with = "lyrics_b",
        required_unless_present = "lyrics_b"
    )]
    pub lyrics_b_file: Option<PathBuf>,

    /// Reuse a Create session token; defaults to a fresh UUID for this command
    #[arg(long)]
    pub session_token: Option<String>,

    /// Return the submit handles without polling for terminal lyrics
    #[arg(long)]
    pub no_wait: bool,

    /// Maximum terminal-state wait in seconds (the Web helper bounds at 150 seconds)
    #[arg(long, default_value_t = LYRICS_MASHUP_DEFAULT_TIMEOUT_SECS)]
    pub timeout: u64,
}

#[derive(clap::Args)]
pub struct LyricsMashupStatusArgs {
    /// Mashup ID returned by `sunox lyrics mashup --no-wait`
    pub id: String,

    /// Poll until the mashup reaches complete or error
    #[arg(long)]
    pub wait: bool,

    /// Maximum terminal-state wait in seconds (default: 150 with --wait)
    #[arg(long, requires = "wait")]
    pub timeout: Option<u64>,
}

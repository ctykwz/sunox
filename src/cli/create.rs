use super::{CoverModel, ModelVersion, RemasterModel, RemasterVariation, VocalGender};

#[derive(clap::Args)]
pub struct CreateArgs {
    /// Description of the song you want
    pub prompt: Option<String>,

    /// Song title
    #[arg(short, long)]
    pub title: Option<String>,

    /// Style tags (optional, guides the generation)
    #[arg(long)]
    pub tags: Option<String>,

    /// Exclude styles (comma-separated): "metal, heavy"
    #[arg(long)]
    pub exclude: Option<String>,

    /// Lyrics text (with [Verse], [Chorus] tags). When provided, create uses
    /// custom lyrics mode instead of description mode.
    #[arg(short, long, conflicts_with = "lyrics_file")]
    pub lyrics: Option<String>,

    /// Read lyrics from file
    #[arg(long)]
    pub lyrics_file: Option<String>,

    /// Model version
    #[arg(short, long)]
    pub model: Option<ModelVersion>,

    /// Vocal gender
    #[arg(long)]
    pub vocal: Option<VocalGender>,

    /// Weirdness level (0-100)
    #[arg(long)]
    pub weirdness: Option<f64>,

    /// Style influence strength (0-100)
    #[arg(long)]
    pub style_influence: Option<f64>,

    /// Enhance style tags through Suno's web prompt upsample flow before submit.
    #[arg(long)]
    pub enhance_tags: bool,

    /// Generate instrumental only
    #[arg(long)]
    pub instrumental: bool,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting.
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Do not force the built-in challenge solver; challenge preflight still runs.
    #[arg(long)]
    pub no_captcha: bool,

    /// Voice persona ID (generates with your custom voice)
    #[arg(long)]
    pub persona: Option<String>,
}

#[derive(clap::Args)]
pub struct GenerateArgs {
    /// Song title
    #[arg(short, long)]
    pub title: Option<String>,

    /// Style tags (comma-separated): "pop, synths, upbeat"
    #[arg(long)]
    pub tags: Option<String>,

    /// Exclude styles (comma-separated): "metal, heavy"
    #[arg(long)]
    pub exclude: Option<String>,

    /// Lyrics text (with [Verse], [Chorus] tags)
    #[arg(short, long, conflicts_with = "lyrics_file")]
    pub lyrics: Option<String>,

    /// Read lyrics from file
    #[arg(long)]
    pub lyrics_file: Option<String>,

    /// Model version
    #[arg(short, long)]
    pub model: Option<ModelVersion>,

    /// Vocal gender
    #[arg(long)]
    pub vocal: Option<VocalGender>,

    /// Weirdness level (0-100)
    #[arg(long)]
    pub weirdness: Option<f64>,

    /// Style influence strength (0-100)
    #[arg(long)]
    pub style_influence: Option<f64>,

    /// Enhance style tags through Suno's web prompt upsample flow before submit.
    #[arg(long)]
    pub enhance_tags: bool,

    /// Generate instrumental only (no vocals)
    #[arg(long)]
    pub instrumental: bool,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting.
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Do not force the built-in challenge solver; challenge preflight still runs.
    #[arg(long)]
    pub no_captcha: bool,

    /// Voice persona ID (generates with your custom voice)
    #[arg(long)]
    pub persona: Option<String>,
}

#[derive(clap::Args)]
pub struct DescribeArgs {
    /// Song title
    #[arg(short, long)]
    pub title: Option<String>,

    /// Description of the song you want
    #[arg(short, long)]
    pub prompt: String,

    /// Style tags (optional, guides the generation)
    #[arg(long)]
    pub tags: Option<String>,

    /// Styles to avoid (negative tags)
    #[arg(long)]
    pub exclude: Option<String>,

    /// Model version
    #[arg(short, long)]
    pub model: Option<ModelVersion>,

    /// Vocal gender
    #[arg(long)]
    pub vocal: Option<VocalGender>,

    /// Weirdness level (0-100)
    #[arg(long)]
    pub weirdness: Option<f64>,

    /// Style influence strength (0-100)
    #[arg(long)]
    pub style_influence: Option<f64>,

    /// Enhance style tags through Suno's web prompt upsample flow before submit.
    #[arg(long)]
    pub enhance_tags: bool,

    /// Generate instrumental only
    #[arg(long)]
    pub instrumental: bool,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting.
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Skip the built-in challenge solver. This is the default unless
    /// `--captcha` is supplied.
    #[arg(long)]
    pub no_captcha: bool,

    /// Voice persona ID (generates with your custom voice)
    #[arg(long)]
    pub persona: Option<String>,
}

#[derive(clap::Args)]
pub struct LyricsArgs {
    /// What the song should be about
    #[arg(short, long)]
    pub prompt: String,
}

#[derive(clap::Args)]
pub struct ExtendArgs {
    /// Clip ID to extend
    pub clip_id: String,

    /// Timestamp in seconds to continue from
    #[arg(long)]
    pub at: f64,

    /// New lyrics for the extension
    #[arg(long)]
    pub lyrics: Option<String>,

    /// Title for the continued clip. Defaults to the source clip title.
    #[arg(long)]
    pub title: Option<String>,

    /// Style tags
    #[arg(long)]
    pub tags: Option<String>,

    /// Exclude styles. Defaults to the source clip's exclude tags when available.
    #[arg(long)]
    pub exclude: Option<String>,

    /// Force instrumental continuation. Defaults to the source clip setting.
    #[arg(long, conflicts_with = "no_instrumental")]
    pub instrumental: bool,

    /// Force vocal continuation instead of inheriting the source clip setting.
    #[arg(long)]
    pub no_instrumental: bool,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting.
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Do not force the built-in challenge solver; challenge preflight still runs.
    #[arg(long)]
    pub no_captcha: bool,
}

#[derive(clap::Args)]
pub struct ConcatArgs {
    /// Clip ID to concatenate into a full song
    pub clip_id: String,
}

#[derive(clap::Args)]
pub struct CoverArgs {
    /// Clip ID to create a cover of
    pub clip_id: String,

    /// Style tags for the cover
    #[arg(long)]
    pub tags: Option<String>,

    /// Model version for the cover
    #[arg(short, long)]
    pub model: Option<CoverModel>,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting.
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Do not force the built-in challenge solver; challenge preflight still runs.
    #[arg(long)]
    pub no_captcha: bool,
}

#[derive(clap::Args)]
pub struct InspireArgs {
    /// Source clip ID to use as inspiration
    pub clip_id: String,

    /// Title for the generated song
    #[arg(long)]
    pub title: String,

    /// Starting style tags; Suno expands these through its prompt upsample flow
    #[arg(long)]
    pub tags: String,

    /// Styles to exclude
    #[arg(long)]
    pub exclude: Option<String>,

    /// Lyrics text
    #[arg(
        long,
        conflicts_with = "lyrics_file",
        required_unless_present = "lyrics_file"
    )]
    pub lyrics: Option<String>,

    /// Read lyrics from file
    #[arg(long, required_unless_present = "lyrics")]
    pub lyrics_file: Option<String>,

    /// Weirdness level captured by the inspiration flow (0-100)
    #[arg(long, default_value_t = 40.0)]
    pub weirdness: f64,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Do not force the built-in challenge solver; challenge preflight still runs
    #[arg(long)]
    pub no_captcha: bool,
}

#[derive(clap::Args)]
pub struct RemasterArgs {
    /// Clip ID to remaster
    pub clip_id: String,

    /// Remaster model version
    #[arg(long)]
    pub model: Option<RemasterModel>,

    /// How strongly the remaster may vary from the source
    #[arg(long, value_enum, default_value_t)]
    pub variation: RemasterVariation,
}

#[derive(clap::Args)]
pub struct StemsArgs {
    /// Clip ID to extract stems from
    pub clip_id: String,

    /// Challenge token (overrides the built-in solver)
    #[arg(long)]
    pub token: Option<String>,

    /// Force the built-in browser challenge solver before submitting.
    #[arg(long, conflicts_with = "no_captcha")]
    pub captcha: bool,

    /// Do not force the built-in challenge solver; challenge preflight still runs.
    #[arg(long)]
    pub no_captcha: bool,
}

#[derive(clap::Args)]
pub struct SpeedArgs {
    /// Clip ID to adjust
    pub clip_id: String,

    /// Playback speed multiplier, for example 0.94 or 1.25
    #[arg(long)]
    pub multiplier: f64,

    /// Keep pitch while changing speed
    #[arg(long = "no-keep-pitch", default_value_t = true, action = clap::ArgAction::SetFalse)]
    pub keep_pitch: bool,

    /// Title for the generated speed-adjusted clip
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(clap::Args)]
pub struct ReverseArgs {
    /// Clip ID to reverse
    pub clip_id: String,

    /// Title for the generated reversed clip
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(clap::Args)]
pub struct CropArgs {
    /// Clip ID to crop
    pub clip_id: String,

    /// Start time in seconds
    #[arg(long)]
    pub start: f64,

    /// End time in seconds
    #[arg(long)]
    pub end: f64,

    /// Remove the selected section instead of keeping only the selected section
    #[arg(long)]
    pub remove_section: bool,

    /// Title for the generated edited clip
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(clap::Args)]
pub struct FadeArgs {
    /// Clip ID to fade
    pub clip_id: String,

    /// Fade in until this timestamp, in seconds
    #[arg(long = "in")]
    pub fade_in: Option<f64>,

    /// Fade out starting at this timestamp, in seconds
    #[arg(long = "out")]
    pub fade_out: Option<f64>,

    /// Title for the generated faded clip
    #[arg(long)]
    pub title: Option<String>,
}

use clap::Subcommand;

use super::{
    ConcatArgs, CoverArgs, CropArgs, DeleteArgs, DownloadArgs, EmptyTrashArgs, ExtendArgs,
    FadeArgs, InfoArgs, InspireArgs, ListArgs, PublishArgs, PurgeArgs, ReactionArgs, RemasterArgs,
    RestoreArgs, ReverseArgs, SearchArgs, SetArgs, SpeedArgs, StatusArgs, StemsArgs,
    TimedLyricsArgs, UploadArgs, UploadStatusArgs, WaitArgs,
};

#[derive(clap::Args)]
pub struct ClipArgs {
    #[command(subcommand)]
    pub command: ClipCommand,
}

#[derive(Subcommand)]
pub enum ClipCommand {
    /// List your songs
    List(ListArgs),

    /// Search your songs by title or tags
    Search(SearchArgs),

    /// Show detailed info for a single clip
    Info(InfoArgs),

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
}

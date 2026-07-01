use clap::Subcommand;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct AddArgs {
    /// Clip ID(s) to add
    pub clip_ids: Vec<String>,

    /// Playlist ID to add clips to
    #[arg(long = "to", value_name = "PLAYLIST_ID")]
    pub playlist_id: String,
}

#[derive(clap::Args)]
pub struct PlaylistArgs {
    #[command(subcommand)]
    pub command: PlaylistCommand,
}

#[derive(Subcommand)]
pub enum PlaylistCommand {
    /// List your playlists
    List(PlaylistListArgs),

    /// Show playlist details
    Info(PlaylistInfoArgs),

    /// Create a playlist
    Create(PlaylistCreateArgs),

    /// Update playlist metadata
    Set(PlaylistSetArgs),

    /// Add clips to a playlist
    Add(PlaylistTracksArgs),

    /// Remove clips from a playlist
    Remove(PlaylistTracksArgs),

    /// Toggle playlist public/private
    Publish(PlaylistPublishArgs),

    /// Move a clip to another playlist index
    Reorder(PlaylistReorderArgs),

    /// Restore a trashed playlist
    Restore(PlaylistRestoreArgs),

    /// Save a playlist to your library
    Save(PlaylistSaveArgs),

    /// Remove a saved playlist from your library
    Unsave(PlaylistSaveArgs),

    /// Like a playlist, or clear the like with --clear
    Like(PlaylistReactionArgs),

    /// Dislike a playlist, or clear the dislike with --clear
    Dislike(PlaylistReactionArgs),

    /// Delete/trash a playlist
    Delete(PlaylistDeleteArgs),
}

#[derive(clap::Args)]
pub struct PlaylistListArgs {
    /// Playlist page number
    #[arg(long, default_value_t = 1)]
    pub page: u32,
}

#[derive(clap::Args)]
pub struct PlaylistInfoArgs {
    /// Playlist ID to inspect
    pub id: String,
}

#[derive(clap::Args)]
pub struct PlaylistCreateArgs {
    /// Playlist name
    #[arg(long)]
    pub name: String,

    /// Playlist description
    #[arg(long)]
    pub description: Option<String>,

    /// Playlist cover image URL
    #[arg(long, conflicts_with = "image_file")]
    pub image_url: Option<String>,

    /// Local image file to upload and use as playlist cover
    #[arg(long)]
    pub image_file: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct PlaylistSetArgs {
    /// Playlist ID to update
    pub id: String,

    /// New playlist name
    #[arg(long)]
    pub name: Option<String>,

    /// New playlist description
    #[arg(long)]
    pub description: Option<String>,

    /// New playlist cover image URL
    #[arg(long, conflicts_with = "image_file")]
    pub image_url: Option<String>,

    /// Local image file to upload and use as playlist cover
    #[arg(long)]
    pub image_file: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct PlaylistTracksArgs {
    /// Playlist ID to update
    pub id: String,

    /// Clip ID(s)
    pub clip_ids: Vec<String>,
}

#[derive(clap::Args)]
pub struct PlaylistPublishArgs {
    /// Playlist ID to update
    pub id: String,

    /// Make private instead of public
    #[arg(long)]
    pub private: bool,
}

#[derive(clap::Args)]
pub struct PlaylistReorderArgs {
    /// Playlist ID to update
    pub id: String,

    /// Clip ID to move
    #[arg(long)]
    pub clip_id: String,

    /// Destination zero-based index
    #[arg(long)]
    pub index: u32,
}

#[derive(clap::Args)]
pub struct PlaylistRestoreArgs {
    /// Playlist ID to restore
    pub id: String,
}

#[derive(clap::Args)]
pub struct PlaylistSaveArgs {
    /// Playlist ID to save or unsave
    pub id: String,
}

#[derive(clap::Args)]
pub struct PlaylistReactionArgs {
    /// Playlist ID to update
    pub id: String,

    /// Clear this reaction instead of setting it
    #[arg(long)]
    pub clear: bool,
}

#[derive(clap::Args)]
pub struct PlaylistDeleteArgs {
    /// Playlist ID to delete/trash
    pub id: String,

    /// Skip confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

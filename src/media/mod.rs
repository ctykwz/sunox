//! Media file operations such as downloads and MP3 metadata tagging.

pub mod download;
pub mod tags;

pub use download::{download_clip, download_clip_url, stage_clip_url};
pub use tags::embed_lyrics_in_mp3;

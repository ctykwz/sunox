use crate::api::types::PlaylistInfo;

use super::{base_table, dynamic_table};

pub fn playlists(playlists: &[PlaylistInfo]) {
    let mut table = dynamic_table();
    table.set_header(vec!["ID", "Name", "Public", "Trashed", "Clips"]);

    for playlist in playlists {
        let short_id = if playlist.id.len() > 8 {
            &playlist.id[..8]
        } else {
            &playlist.id
        };
        table.add_row(vec![
            short_id,
            &playlist.name,
            playlist
                .is_public
                .map(|value| if value { "true" } else { "false" })
                .unwrap_or("-"),
            playlist
                .is_trashed
                .map(|value| if value { "true" } else { "false" })
                .unwrap_or("-"),
            &playlist.clip_count().to_string(),
        ]);
    }

    println!("{table}");
}

pub fn playlist_detail(playlist: &PlaylistInfo) {
    let mut table = base_table();
    table.set_header(vec!["Field", "Value"]);

    table.add_row(vec!["ID", &playlist.id]);
    table.add_row(vec!["Name", &playlist.name]);
    table.add_row(vec![
        "Description",
        playlist.description.as_deref().unwrap_or("-"),
    ]);
    table.add_row(vec![
        "Public",
        playlist
            .is_public
            .map(|value| if value { "true" } else { "false" })
            .unwrap_or("-"),
    ]);
    table.add_row(vec![
        "Trashed",
        playlist
            .is_trashed
            .map(|value| if value { "true" } else { "false" })
            .unwrap_or("-"),
    ]);
    table.add_row(vec!["Clips", &playlist.clip_count().to_string()]);
    if let Some(ref image_url) = playlist.image_url {
        table.add_row(vec!["Image URL", image_url]);
    }

    println!("{table}");

    if !playlist.playlist_clips.is_empty() {
        let clips: Vec<_> = playlist
            .playlist_clips
            .iter()
            .filter_map(|entry| entry.clip.clone())
            .collect();
        if !clips.is_empty() {
            super::clips(&clips);
        }
    }
}

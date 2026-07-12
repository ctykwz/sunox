//! Human-readable table renderers grouped by output domain.

use comfy_table::{ContentArrangement, Table, modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL};

mod account;
mod clip;
mod lyrics;
mod persona;
mod playlist;

pub use account::{billing, models, remaster_models};
pub use clip::{clip_detail, clips};
pub use lyrics::lyrics;
pub use persona::{persona, personas};
pub use playlist::{playlist_detail, playlists};

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS);
    table
}

fn dynamic_table() -> Table {
    let mut table = base_table();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table
}

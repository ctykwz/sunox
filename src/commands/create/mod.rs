mod inspire;
mod lyrics;
mod submit;
mod support;
mod transform;

pub use inspire::inspire;
pub use lyrics::lyrics;
#[cfg(test)]
pub(crate) use submit::{
    build_generate_args_from_create, build_generate_request, validate_lyrics_project_reference,
};
pub use submit::{create, extend};
pub use transform::{concat, cover, crop, fade, remaster, reverse, speed, stems};

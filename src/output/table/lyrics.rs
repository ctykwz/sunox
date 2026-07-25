use crate::api::types::CowriteLyricsResponse;

pub fn lyrics(result: &CowriteLyricsResponse) {
    println!("{}", result.edited_lyrics);
}

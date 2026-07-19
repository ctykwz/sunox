use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ConcatRequest {
    pub clip_id: String,
    pub is_infill: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RemasterVariation {
    Subtle,
    #[default]
    Normal,
    High,
}

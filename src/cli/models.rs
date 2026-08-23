use clap::ValueEnum;

#[derive(ValueEnum, Clone, Debug)]
pub enum VocalGender {
    Male,
    Female,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum RemasterModel {
    #[value(name = "v5.5", alias = "chirp-flounder")]
    #[default]
    V55,
    #[value(name = "v5", alias = "chirp-carp")]
    V5,
    #[value(name = "v4.5+", alias = "chirp-bass")]
    V45Plus,
}

impl RemasterModel {
    pub fn to_api_key(&self) -> &'static str {
        match self {
            Self::V55 => "chirp-flounder",
            Self::V5 => "chirp-carp",
            Self::V45Plus => "chirp-bass",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::V55 => "v5.5",
            Self::V5 => "v5",
            Self::V45Plus => "v4.5+",
        }
    }

    pub fn supports_api_key(key: &str) -> bool {
        matches!(key, "chirp-flounder" | "chirp-carp" | "chirp-bass")
    }
}

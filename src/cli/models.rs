use clap::ValueEnum;

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ModelVersion {
    #[value(name = "v5.5")]
    #[default]
    V55,
    #[value(name = "v5")]
    V5,
    #[value(name = "v4.5+")]
    V45Plus,
    #[value(name = "v4.5-all")]
    V45All,
    #[value(name = "v4.5")]
    V45,
    #[value(name = "v4")]
    V4,
    #[value(name = "v3.5")]
    V35,
    #[value(name = "v3")]
    V3,
    #[value(name = "v2")]
    V2,
}

impl ModelVersion {
    pub fn to_api_key(&self) -> &'static str {
        match self {
            Self::V55 => "chirp-fenix",
            Self::V5 => "chirp-crow",
            Self::V45Plus => "chirp-bluejay",
            Self::V45All => "chirp-auk-turbo",
            Self::V45 => "chirp-auk",
            Self::V4 => "chirp-v4",
            Self::V35 => "chirp-v3-5",
            Self::V3 => "chirp-v3-0",
            Self::V2 => "chirp-v2-xxl-alpha",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::V55 => "v5.5",
            Self::V5 => "v5",
            Self::V45Plus => "v4.5+",
            Self::V45All => "v4.5-all",
            Self::V45 => "v4.5",
            Self::V4 => "v4",
            Self::V35 => "v3.5",
            Self::V3 => "v3",
            Self::V2 => "v2",
        }
    }
}

#[derive(ValueEnum, Clone, Debug)]
pub enum CoverModel {
    #[value(name = "v5.5")]
    V55,
    #[value(name = "v5")]
    V5,
    #[value(name = "v4.5+")]
    V45Plus,
    #[value(name = "v4.5")]
    V45,
    #[value(name = "v4")]
    V4,
    #[value(name = "v3.5")]
    V35,
    #[value(name = "v3")]
    V3,
    #[value(name = "v2")]
    V2,
}

impl CoverModel {
    pub fn to_api_key(&self) -> &'static str {
        match self {
            Self::V55 => "chirp-fenix",
            Self::V5 => "chirp-crow",
            Self::V45Plus => "chirp-bluejay",
            Self::V45 => "chirp-auk",
            Self::V4 => "chirp-v4",
            Self::V35 => "chirp-v3-5",
            Self::V3 => "chirp-v3-0",
            Self::V2 => "chirp-v2-xxl-alpha",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::V55 => "v5.5",
            Self::V5 => "v5",
            Self::V45Plus => "v4.5+",
            Self::V45 => "v4.5",
            Self::V4 => "v4",
            Self::V35 => "v3.5",
            Self::V3 => "v3",
            Self::V2 => "v2",
        }
    }
}

#[derive(ValueEnum, Clone, Debug)]
pub enum VocalGender {
    Male,
    Female,
}

#[cfg(test)]
mod tests {
    use super::ModelVersion;

    #[test]
    fn v45_all_uses_the_current_free_model_key() {
        assert_eq!(ModelVersion::V45All.to_api_key(), "chirp-auk-turbo");
        assert_eq!(ModelVersion::V45All.display_name(), "v4.5-all");
    }
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum RemasterModel {
    #[value(name = "v5.5")]
    #[default]
    V55,
    #[value(name = "v5")]
    V5,
    #[value(name = "v4.5+")]
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
}

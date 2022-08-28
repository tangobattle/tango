use crate::games;

#[derive(serde::Deserialize)]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub versions: std::collections::HashMap<String, VersionMetadata>,
}

#[derive(serde::Deserialize)]
pub struct VersionMetadata {
    pub saveedit_overrides: Option<toml::value::Table>,
    pub netplay_compatiblity: Option<String>,
}

pub struct Patch {
    pub metadata: Metadata,
    pub supported_games: std::collections::HashSet<&'static (dyn games::Game + Send + Sync)>,
}

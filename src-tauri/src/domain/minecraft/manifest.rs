use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct VersionManifest {
    pub versions: Vec<ManifestVersionEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManifestVersionEntry {
    pub id: String,
    pub url: String,
    #[serde(rename = "type")]
    pub r#type: String,
}

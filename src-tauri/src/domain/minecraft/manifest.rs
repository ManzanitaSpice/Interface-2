use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<ManifestVersionEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestVersionEntry {
    pub id: String,
    pub url: String,
    #[serde(rename = "type")]
    pub r#type: String,
}

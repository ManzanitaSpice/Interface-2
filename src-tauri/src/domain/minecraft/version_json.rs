use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJsonMeta {
    pub id: String,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
}

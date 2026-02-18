use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<MinecraftSkin>,
    #[serde(default)]
    pub capes: Vec<MinecraftCape>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftSkin {
    pub id: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub variant: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftCape {
    pub id: Option<String>,
    pub state: Option<String>,
    pub url: Option<String>,
    pub alias: Option<String>,
}

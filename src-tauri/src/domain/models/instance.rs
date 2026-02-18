use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstancePayload {
    pub name: String,
    pub group: String,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
    pub required_java_major: Option<u32>,
    pub ram_mb: u32,
    pub java_args: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceResult {
    pub id: String,
    pub name: String,
    pub group: String,
    pub launcher_root: String,
    pub instance_root: String,
    pub minecraft_path: String,
    pub logs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: String,
    pub name: String,
    pub group: String,
    pub instance_root: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceMetadata {
    pub name: String,
    pub group: String,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
    pub ram_mb: u32,
    pub java_args: Vec<String>,
    pub java_path: String,
    pub java_runtime: String,
    #[serde(default)]
    pub java_version: String,
    pub last_used: Option<String>,
    pub internal_uuid: String,
}

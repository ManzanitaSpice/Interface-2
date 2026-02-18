use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MicrosoftTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct XboxAuthResponse {
    #[serde(rename = "Token")]
    pub token: String,
    #[serde(rename = "DisplayClaims")]
    pub display_claims: XboxDisplayClaims,
}

#[derive(Debug, Deserialize)]
pub struct XboxDisplayClaims {
    pub xui: Vec<XboxUserClaim>,
}

#[derive(Debug, Deserialize)]
pub struct XboxUserClaim {
    pub uhs: String,
}

#[derive(Debug, Deserialize)]
pub struct MinecraftLoginResponse {
    pub access_token: String,
}

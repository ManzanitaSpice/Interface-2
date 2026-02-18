use serde::Serialize;
use serde_json::json;

use crate::domain::auth::{
    profile::MinecraftProfile,
    tokens::{MinecraftLoginResponse, XboxAuthResponse},
};

const XBOX_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug)]
pub struct XboxLiveToken {
    pub token: String,
    pub uhs: String,
}

#[derive(Debug)]
pub struct XstsToken {
    pub token: String,
    pub uhs: String,
}

pub fn authenticate_with_xbox_live(microsoft_access_token: &str) -> Result<XboxLiveToken, String> {
    let payload = json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={microsoft_access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(XBOX_AUTH_URL)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .map_err(|err| format!("No se pudo autenticar en Xbox Live: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Xbox Live devolvió error HTTP: {err}"))?
        .json::<XboxAuthResponse>()
        .map_err(|err| format!("No se pudo leer token de Xbox Live: {err}"))?;

    let uhs = response
        .display_claims
        .xui
        .first()
        .map(|claim| claim.uhs.clone())
        .ok_or_else(|| "Xbox Live no devolvió displayClaims.xui[0].uhs".to_string())?;

    Ok(XboxLiveToken {
        token: response.token,
        uhs,
    })
}

pub fn authorize_xsts(xbox_token: &str) -> Result<XstsToken, String> {
    let payload = json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbox_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(XSTS_AUTH_URL)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .map_err(|err| format!("No se pudo autorizar XSTS: {err}"))?
        .error_for_status()
        .map_err(|err| format!("XSTS devolvió error HTTP: {err}"))?
        .json::<XboxAuthResponse>()
        .map_err(|err| format!("No se pudo leer token XSTS: {err}"))?;

    let uhs = response
        .display_claims
        .xui
        .first()
        .map(|claim| claim.uhs.clone())
        .ok_or_else(|| "XSTS no devolvió displayClaims.xui[0].uhs".to_string())?;

    Ok(XstsToken {
        token: response.token,
        uhs,
    })
}

pub fn login_minecraft_with_xbox(
    uhs: &str,
    xsts_token: &str,
) -> Result<MinecraftLoginResponse, String> {
    #[derive(Debug, Serialize)]
    struct MinecraftLoginRequest {
        #[serde(rename = "identityToken")]
        identity_token: String,
    }

    let payload = MinecraftLoginRequest {
        identity_token: format!("XBL3.0 x={uhs};{xsts_token}"),
    };

    let client = reqwest::blocking::Client::new();
    client
        .post(MINECRAFT_LOGIN_URL)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .map_err(|err| format!("No se pudo autenticar en Minecraft Services: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Minecraft Services devolvió error HTTP: {err}"))?
        .json::<MinecraftLoginResponse>()
        .map_err(|err| format!("No se pudo leer access token de Minecraft: {err}"))
}

pub fn read_minecraft_profile(minecraft_access_token: &str) -> Result<MinecraftProfile, String> {
    let client = reqwest::blocking::Client::new();
    client
        .get(MINECRAFT_PROFILE_URL)
        .header("Authorization", format!("Bearer {minecraft_access_token}"))
        .header("Accept", "application/json")
        .send()
        .map_err(|err| format!("No se pudo consultar perfil de Minecraft: {err}"))?
        .error_for_status()
        .map_err(|err| format!("La API de perfil de Minecraft devolvió error HTTP: {err}"))?
        .json::<MinecraftProfile>()
        .map_err(|err| format!("No se pudo leer perfil de Minecraft: {err}"))
}

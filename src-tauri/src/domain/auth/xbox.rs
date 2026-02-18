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

#[derive(Debug, Serialize)]
struct XstsProperties<'a> {
    #[serde(rename = "SandboxId")]
    sandbox_id: &'static str,
    #[serde(rename = "UserTokens")]
    user_tokens: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct XstsRequest<'a> {
    #[serde(rename = "Properties")]
    properties: XstsProperties<'a>,
    #[serde(rename = "RelyingParty")]
    relying_party: &'static str,
    #[serde(rename = "TokenType")]
    token_type: &'static str,
}

fn build_xsts_request(xbox_token: &str) -> XstsRequest<'_> {
    XstsRequest {
        properties: XstsProperties {
            sandbox_id: "RETAIL",
            user_tokens: vec![xbox_token],
        },
        relying_party: "rp://api.minecraftservices.com/",
        token_type: "JWT",
    }
}

fn build_minecraft_identity_token(uhs: &str, xsts_token: &str) -> String {
    format!("XBL3.0 x={uhs};{xsts_token}")
}

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

pub async fn authenticate_with_xbox_live(
    client: &reqwest::Client,
    microsoft_access_token: &str,
) -> Result<XboxLiveToken, String> {
    let payload = json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={microsoft_access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });

    let response = client
        .post(XBOX_AUTH_URL)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|err| format!("No se pudo autenticar en Xbox Live: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Xbox Live devolvió error HTTP: {status}. Body completo: {body}"
        ));
    }

    let response = response
        .json::<XboxAuthResponse>()
        .await
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

pub async fn authorize_xsts(
    client: &reqwest::Client,
    xbox_token: &str,
) -> Result<XstsToken, String> {
    let payload = build_xsts_request(xbox_token);

    let response = client
        .post(XSTS_AUTH_URL)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|err| format!("No se pudo autorizar XSTS: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "XSTS devolvió error HTTP: {status}. Body completo: {body}"
        ));
    }

    let response = response
        .json::<XboxAuthResponse>()
        .await
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

pub async fn login_minecraft_with_xbox(
    client: &reqwest::Client,
    uhs: &str,
    xsts_token: &str,
) -> Result<MinecraftLoginResponse, String> {
    #[derive(Debug, Serialize)]
    struct MinecraftLoginRequest {
        #[serde(rename = "identityToken")]
        identity_token: String,
    }

    let payload = MinecraftLoginRequest {
        identity_token: build_minecraft_identity_token(uhs, xsts_token),
    };

    let response = client
        .post(MINECRAFT_LOGIN_URL)
        .header("Accept", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|err| format!("No se pudo autenticar en Minecraft Services: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if status.as_u16() == 403 {
            return Err(format!(
                "Minecraft Services 403 Forbidden. Body completo: {body}"
            ));
        }

        return Err(format!(
            "Minecraft Services devolvió error HTTP: {status}. Body completo: {body}"
        ));
    }

    response
        .json::<MinecraftLoginResponse>()
        .await
        .map_err(|err| format!("No se pudo leer access token de Minecraft: {err}"))
}

pub async fn read_minecraft_profile(
    client: &reqwest::Client,
    minecraft_access_token: &str,
) -> Result<MinecraftProfile, String> {
    let response = client
        .get(MINECRAFT_PROFILE_URL)
        .header("Authorization", format!("Bearer {minecraft_access_token}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("No se pudo consultar perfil de Minecraft: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "La API de perfil de Minecraft devolvió error HTTP: {status}. Body completo: {body}"
        ));
    }

    response
        .json::<MinecraftProfile>()
        .await
        .map_err(|err| format!("No se pudo leer perfil de Minecraft: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{build_minecraft_identity_token, build_xsts_request};

    #[test]
    fn build_xsts_request_uses_minecraft_relying_party() {
        let payload = serde_json::to_value(build_xsts_request("xbl-token")).unwrap();

        assert_eq!(
            payload["RelyingParty"],
            serde_json::Value::String("rp://api.minecraftservices.com/".to_string())
        );
        assert_eq!(payload["Properties"]["SandboxId"], "RETAIL");
        assert_eq!(payload["Properties"]["UserTokens"][0], "xbl-token");
    }

    #[test]
    fn minecraft_identity_token_has_expected_format() {
        let token = build_minecraft_identity_token("user-hash", "xsts-token");
        assert_eq!(token, "XBL3.0 x=user-hash;xsts-token");
    }
}

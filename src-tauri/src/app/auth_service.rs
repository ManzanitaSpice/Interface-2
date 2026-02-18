use serde::Serialize;

use crate::domain::auth::{
    microsoft::{
        build_authorize_url, exchange_authorization_code, generate_code_verifier,
        MICROSOFT_REDIRECT_URI,
    },
    profile::MinecraftProfile,
    xbox::{
        authenticate_with_xbox_live, authorize_xsts, login_minecraft_with_xbox,
        read_minecraft_profile,
    },
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftAuthStart {
    pub authorize_url: String,
    pub code_verifier: String,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftAuthResult {
    pub microsoft_access_token: String,
    pub microsoft_refresh_token: Option<String>,
    pub xbox_token: String,
    pub xsts_token: String,
    pub uhs: String,
    pub minecraft_access_token: String,
    pub profile: MinecraftProfile,
}

#[tauri::command]
pub fn start_microsoft_auth() -> Result<MicrosoftAuthStart, String> {
    let code_verifier = generate_code_verifier();
    let authorize_url = build_authorize_url(&code_verifier)?;

    Ok(MicrosoftAuthStart {
        authorize_url,
        code_verifier,
        redirect_uri: MICROSOFT_REDIRECT_URI.to_string(),
    })
}

#[tauri::command]
pub fn complete_microsoft_auth(
    code: String,
    code_verifier: String,
) -> Result<MicrosoftAuthResult, String> {
    if code.trim().is_empty() {
        return Err("El código de autorización de Microsoft está vacío.".to_string());
    }

    let microsoft_tokens = exchange_authorization_code(&code, &code_verifier)?;
    let xbox = authenticate_with_xbox_live(&microsoft_tokens.access_token)?;
    let xsts = authorize_xsts(&xbox.token)?;
    let minecraft = login_minecraft_with_xbox(&xsts.uhs, &xsts.token)?;
    let profile = read_minecraft_profile(&minecraft.access_token)?;

    Ok(MicrosoftAuthResult {
        microsoft_access_token: microsoft_tokens.access_token,
        microsoft_refresh_token: microsoft_tokens.refresh_token,
        xbox_token: xbox.token,
        xsts_token: xsts.token,
        uhs: xsts.uhs,
        minecraft_access_token: minecraft.access_token,
        profile,
    })
}

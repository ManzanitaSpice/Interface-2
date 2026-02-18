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

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOption {
    pub id: String,
    pub name: String,
}

async fn finalize_microsoft_tokens(
    client: &reqwest::Client,
    microsoft_tokens: crate::domain::auth::tokens::MicrosoftTokenResponse,
) -> Result<MicrosoftAuthResult, String> {
    let xbox = authenticate_with_xbox_live(client, &microsoft_tokens.access_token).await?;
    let xsts = authorize_xsts(client, &xbox.token).await?;
    let minecraft = login_minecraft_with_xbox(client, &xsts.uhs, &xsts.token).await?;
    let profile = read_minecraft_profile(client, &minecraft.access_token).await?;

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

fn browser_candidates() -> Vec<(&'static str, &'static str)> {
    vec![("default", "Navegador predeterminado")]
}

fn browser_is_available(_browser_id: &str) -> bool {
    true
}

#[tauri::command]
pub fn list_available_browsers() -> Vec<BrowserOption> {
    browser_candidates()
        .into_iter()
        .filter(|(id, _)| browser_is_available(id))
        .map(|(id, name)| BrowserOption {
            id: id.to_string(),
            name: name.to_string(),
        })
        .collect()
}

fn open_with_browser_command(url: &str) -> Result<(), String> {
    webbrowser::open(url)
        .map(|_| ())
        .map_err(|err| format!("No se pudo abrir el navegador del sistema: {err}"))
}

#[tauri::command]
pub fn open_url_in_browser(url: String, browser_id: String) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("La URL para abrir en el navegador está vacía.".to_string());
    }

    let normalized_url = url.trim().to_string();
    if !normalized_url.starts_with("http://") && !normalized_url.starts_with("https://") {
        return Err("La URL OAuth debe comenzar con http:// o https://.".to_string());
    }

    let _ = browser_id;
    println!("Microsoft OAuth authorize URL: {normalized_url}");
    open_with_browser_command(&normalized_url)
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
pub async fn complete_microsoft_auth(
    code: String,
    code_verifier: String,
) -> Result<MicrosoftAuthResult, String> {
    if code.trim().is_empty() {
        return Err("El código de autorización de Microsoft está vacío.".to_string());
    }

    let client = reqwest::Client::new();
    let microsoft_tokens = exchange_authorization_code(&client, &code, &code_verifier).await?;
    finalize_microsoft_tokens(&client, microsoft_tokens).await
}

#[tauri::command]
pub fn start_microsoft_device_auth() -> Result<MicrosoftAuthStart, String> {
    start_microsoft_auth()
}

#[tauri::command]
pub async fn complete_microsoft_device_auth(
    code: String,
    code_verifier: String,
) -> Result<MicrosoftAuthResult, String> {
    complete_microsoft_auth(code, code_verifier).await
}

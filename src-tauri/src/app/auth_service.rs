use std::process::Command;

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
    microsoft_tokens: crate::domain::auth::tokens::MicrosoftTokenResponse,
) -> Result<MicrosoftAuthResult, String> {
    let xbox = authenticate_with_xbox_live(&microsoft_tokens.access_token).await?;
    let xsts = authorize_xsts(&xbox.token).await?;
    let minecraft = login_minecraft_with_xbox(&xsts.uhs, &xsts.token).await?;
    let profile = read_minecraft_profile(&minecraft.access_token).await?;

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
    vec![
        ("default", "Navegador predeterminado"),
        ("chrome", "Google Chrome"),
        ("chromium", "Chromium"),
        ("firefox", "Mozilla Firefox"),
        ("edge", "Microsoft Edge"),
        ("brave", "Brave"),
        ("opera", "Opera"),
        ("vivaldi", "Vivaldi"),
    ]
}

#[cfg(target_os = "linux")]
fn browser_is_available(browser_id: &str) -> bool {
    if browser_id == "default" {
        return true;
    }

    let commands: &[&str] = match browser_id {
        "chrome" => &["google-chrome", "google-chrome-stable"],
        "chromium" => &["chromium", "chromium-browser"],
        "firefox" => &["firefox"],
        "edge" => &["microsoft-edge", "microsoft-edge-stable"],
        "brave" => &["brave-browser", "brave"],
        "opera" => &["opera"],
        "vivaldi" => &["vivaldi"],
        _ => &[],
    };

    commands.iter().any(|command| {
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {command}"))
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

#[cfg(target_os = "windows")]
fn browser_is_available(_browser_id: &str) -> bool {
    true
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "linux")]
fn open_with_browser_command(browser_id: &str, url: &str) -> Result<(), String> {
    let spawn_result = match browser_id {
        "default" => Command::new("xdg-open").arg(url).spawn(),
        "chrome" => Command::new("google-chrome")
            .arg(url)
            .spawn()
            .or_else(|_| Command::new("google-chrome-stable").arg(url).spawn()),
        "chromium" => Command::new("chromium")
            .arg(url)
            .spawn()
            .or_else(|_| Command::new("chromium-browser").arg(url).spawn()),
        "firefox" => Command::new("firefox").arg(url).spawn(),
        "edge" => Command::new("microsoft-edge")
            .arg(url)
            .spawn()
            .or_else(|_| Command::new("microsoft-edge-stable").arg(url).spawn()),
        "brave" => Command::new("brave-browser")
            .arg(url)
            .spawn()
            .or_else(|_| Command::new("brave").arg(url).spawn()),
        "opera" => Command::new("opera").arg(url).spawn(),
        "vivaldi" => Command::new("vivaldi").arg(url).spawn(),
        _ => return Err(format!("Navegador no soportado: {browser_id}")),
    };

    spawn_result
        .map(|_| ())
        .map_err(|err| format!("No se pudo abrir navegador '{browser_id}': {err}"))
}

#[cfg(target_os = "windows")]
fn open_with_browser_command(browser_id: &str, url: &str) -> Result<(), String> {
    let status = match browser_id {
        "default" => Command::new("cmd").args(["/C", "start", "", url]).status(),
        "chrome" => Command::new("cmd")
            .args(["/C", "start", "", "chrome", url])
            .status(),
        "firefox" => Command::new("cmd")
            .args(["/C", "start", "", "firefox", url])
            .status(),
        "edge" => Command::new("cmd")
            .args(["/C", "start", "", "msedge", url])
            .status(),
        "brave" => Command::new("cmd")
            .args(["/C", "start", "", "brave", url])
            .status(),
        _ => Command::new("cmd").args(["/C", "start", "", url]).status(),
    };

    status
        .map_err(|err| format!("No se pudo abrir navegador '{browser_id}': {err}"))
        .and_then(|exit| {
            if exit.success() {
                Ok(())
            } else {
                Err(format!(
                    "No se pudo abrir navegador '{browser_id}', código de salida {:?}",
                    exit.code()
                ))
            }
        })
}

#[cfg(target_os = "macos")]
fn open_with_browser_command(browser_id: &str, url: &str) -> Result<(), String> {
    let status = match browser_id {
        "default" => Command::new("open").arg(url).status(),
        "chrome" => Command::new("open")
            .args(["-a", "Google Chrome", url])
            .status(),
        "chromium" => Command::new("open").args(["-a", "Chromium", url]).status(),
        "firefox" => Command::new("open").args(["-a", "Firefox", url]).status(),
        "edge" => Command::new("open")
            .args(["-a", "Microsoft Edge", url])
            .status(),
        "brave" => Command::new("open")
            .args(["-a", "Brave Browser", url])
            .status(),
        "opera" => Command::new("open").args(["-a", "Opera", url]).status(),
        "vivaldi" => Command::new("open").args(["-a", "Vivaldi", url]).status(),
        _ => Command::new("open").arg(url).status(),
    };

    status
        .map_err(|err| format!("No se pudo abrir navegador '{browser_id}': {err}"))
        .and_then(|exit| {
            if exit.success() {
                Ok(())
            } else {
                Err(format!(
                    "No se pudo abrir navegador '{browser_id}', código de salida {:?}",
                    exit.code()
                ))
            }
        })
}

#[tauri::command]
pub fn open_url_in_browser(url: String, browser_id: String) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("La URL para abrir en el navegador está vacía.".to_string());
    }
    open_with_browser_command(&browser_id, &url)
}

#[tauri::command]
pub fn start_microsoft_auth() -> Result<MicrosoftAuthStart, String> {
    let code_verifier = generate_code_verifier();
    let authorize_url = build_authorize_url(&code_verifier, MICROSOFT_REDIRECT_URI)?;

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

    let microsoft_tokens =
        exchange_authorization_code(&code, &code_verifier, MICROSOFT_REDIRECT_URI).await?;
    finalize_microsoft_tokens(microsoft_tokens).await
}

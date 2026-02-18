use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

use crate::domain::auth::{
    microsoft::{
        build_authorize_url, exchange_authorization_code, generate_code_verifier,
        MICROSOFT_REDIRECT_URI,
    },
    profile::MinecraftProfile,
    xbox::{
        authenticate_with_xbox_live, authorize_xsts, has_minecraft_license,
        login_minecraft_with_xbox, read_minecraft_profile,
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

const MICROSOFT_AUTH_WINDOW_LABEL_PREFIX: &str = "microsoft-auth";
const MICROSOFT_AUTH_WINDOW_EVENT: &str = "microsoft-auth-window-closed";
const MICROSOFT_AUTH_TIMEOUT_SECS: u64 = 300;

async fn finalize_microsoft_tokens(
    client: &reqwest::Client,
    microsoft_tokens: crate::domain::auth::tokens::MicrosoftTokenResponse,
) -> Result<MicrosoftAuthResult, String> {
    let xbox = authenticate_with_xbox_live(client, &microsoft_tokens.access_token).await?;
    let xsts = authorize_xsts(client, &xbox.token).await?;
    let minecraft = login_minecraft_with_xbox(client, &xsts.uhs, &xsts.token).await?;
    let has_license = has_minecraft_license(client, &minecraft.access_token).await?;
    if !has_license {
        return Err("La cuenta no tiene licencia oficial de Minecraft (entitlements/mcstore vacío). No se permite modo Demo.".to_string());
    }
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

fn extract_code_from_redirect(redirect_url: &str) -> Result<String, String> {
    let parsed = redirect_url
        .parse::<tauri::Url>()
        .map_err(|err| format!("La URL de redirección de Microsoft es inválida: {err}"))?;

    let mut oauth_error: Option<String> = None;
    let mut oauth_error_description: Option<String> = None;

    for (key, value) in parsed.query_pairs() {
        if key == "code" {
            let code = value.trim().to_string();
            if code.is_empty() {
                return Err("Microsoft devolvió un parámetro code vacío.".to_string());
            }
            return Ok(code);
        }

        if key == "error" {
            oauth_error = Some(value.into_owned());
            continue;
        }

        if key == "error_description" {
            oauth_error_description = Some(value.into_owned());
        }
    }

    if let Some(error) = oauth_error {
        let detail = oauth_error_description.unwrap_or_else(|| "Sin detalle adicional".to_string());
        return Err(format!(
            "Microsoft devolvió un error durante el login: {error} ({detail})"
        ));
    }

    Err("No se encontró el parámetro code en la redirección de Microsoft.".to_string())
}

#[tauri::command]
pub async fn authorize_microsoft_in_launcher(
    app: AppHandle,
    authorize_url: String,
) -> Result<String, String> {
    let trimmed_url = authorize_url.trim();
    if trimmed_url.is_empty() {
        return Err("La URL de autorización de Microsoft está vacía.".to_string());
    }

    let parsed_authorize_url = trimmed_url
        .parse::<tauri::Url>()
        .map_err(|err| format!("La URL de autorización de Microsoft es inválida: {err}"))?;

    let label = format!(
        "{MICROSOFT_AUTH_WINDOW_LABEL_PREFIX}-{}",
        uuid::Uuid::new_v4()
    );
    let (tx, rx) = oneshot::channel::<Result<String, String>>();
    let tx_holder = Arc::new(Mutex::new(Some(tx)));
    let tx_for_navigation = Arc::clone(&tx_holder);
    let app_for_navigation = app.clone();
    let label_for_navigation = label.clone();

    let window =
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed_authorize_url))
            .title("Iniciar sesión con Microsoft")
            .inner_size(520.0, 720.0)
            .center()
            .resizable(true)
            .on_navigation(move |navigation_url| {
                let navigation = navigation_url.to_string();
                if !navigation.starts_with(MICROSOFT_REDIRECT_URI) {
                    return true;
                }

                let auth_result = extract_code_from_redirect(&navigation);
                if let Ok(mut tx_guard) = tx_for_navigation.lock() {
                    if let Some(sender) = tx_guard.take() {
                        let _ = sender.send(auth_result);
                    }
                }

                if let Some(window) = app_for_navigation.get_webview_window(&label_for_navigation) {
                    let _ = window.close();
                }

                false
            })
            .build()
            .map_err(|err| format!("No se pudo abrir la ventana de login de Microsoft: {err}"))?;

    window.on_window_event({
        let tx_for_close = Arc::clone(&tx_holder);
        let app_for_close = app.clone();
        move |event| {
            if !matches!(event, tauri::WindowEvent::Destroyed) {
                return;
            }

            if let Ok(mut tx_guard) = tx_for_close.lock() {
                if let Some(sender) = tx_guard.take() {
                    let _ = sender.send(Err(
                        "El inicio de sesión fue cancelado porque se cerró la ventana de Microsoft."
                            .to_string(),
                    ));
                }
            }

            let _ = app_for_close.emit(MICROSOFT_AUTH_WINDOW_EVENT, window.label().to_string());
        }
    });

    match tokio::time::timeout(Duration::from_secs(MICROSOFT_AUTH_TIMEOUT_SECS), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("No se pudo completar el login de Microsoft en el launcher.".to_string()),
        Err(_) => {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.close();
            }
            Err("Tiempo de espera agotado durante el login de Microsoft.".to_string())
        }
    }
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

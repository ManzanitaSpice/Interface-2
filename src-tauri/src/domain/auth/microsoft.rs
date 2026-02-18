use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::auth::tokens::MicrosoftTokenResponse;

pub const MICROSOFT_CLIENT_ID: &str = "7ce1b3e8-48d7-4a9d-9329-7e11f988df39";
pub const MICROSOFT_TENANT: &str = "consumers";
pub const MICROSOFT_SCOPES: &str = "XboxLive.signin XboxLive.offline_access offline_access";

const AUTHORIZE_BASE_URL: &str = "https://login.microsoftonline.com";
const TOKEN_BASE_URL: &str = "https://login.microsoftonline.com";

pub fn generate_code_verifier() -> String {
    let random = uuid::Uuid::new_v4().as_simple().to_string();
    let entropy = uuid::Uuid::new_v4().as_simple().to_string();
    format!("{random}{entropy}")
}

fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[derive(Debug, Deserialize)]
pub struct MicrosoftDeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftAuthErrorResponse {
    error: String,
    error_description: Option<String>,
}

pub fn build_authorize_url(code_verifier: &str, redirect_uri: &str) -> Result<String, String> {
    if code_verifier.trim().is_empty() {
        return Err("El code_verifier para PKCE está vacío.".to_string());
    }

    let authorize_endpoint = format!(
        "{}/{}/oauth2/v2.0/authorize",
        AUTHORIZE_BASE_URL, MICROSOFT_TENANT
    );

    #[derive(Serialize)]
    struct Query<'a> {
        client_id: &'a str,
        response_type: &'a str,
        redirect_uri: &'a str,
        response_mode: &'a str,
        scope: &'a str,
        code_challenge: String,
        code_challenge_method: &'a str,
    }

    let query = Query {
        client_id: MICROSOFT_CLIENT_ID,
        response_type: "code",
        redirect_uri,
        response_mode: "query",
        scope: MICROSOFT_SCOPES,
        code_challenge: code_challenge(code_verifier),
        code_challenge_method: "S256",
    };

    let encoded_query = serde_urlencoded::to_string(query)
        .map_err(|err| format!("No se pudo construir la URL OAuth de Microsoft: {err}"))?;

    Ok(format!("{authorize_endpoint}?{encoded_query}"))
}

pub fn exchange_authorization_code(
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<MicrosoftTokenResponse, String> {
    if code_verifier.trim().is_empty() {
        return Err("code_verifier vacío en intercambio de token Microsoft.".to_string());
    }

    let token_endpoint = format!("{}/{}/oauth2/v2.0/token", TOKEN_BASE_URL, MICROSOFT_TENANT);

    let params = [
        ("client_id", MICROSOFT_CLIENT_ID.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("scope", MICROSOFT_SCOPES.to_string()),
        ("code_verifier", code_verifier.to_string()),
    ];

    let client = reqwest::blocking::Client::new();
    client
        .post(token_endpoint)
        .form(&params)
        .send()
        .map_err(|err| format!("No se pudo llamar a token endpoint de Microsoft: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Token endpoint de Microsoft devolvió error HTTP: {err}"))?
        .json::<MicrosoftTokenResponse>()
        .map_err(|err| format!("No se pudo deserializar token de Microsoft: {err}"))
}

pub fn start_device_code_flow() -> Result<MicrosoftDeviceCodeResponse, String> {
    let endpoint = format!("{}/{}/oauth2/v2.0/devicecode", AUTHORIZE_BASE_URL, MICROSOFT_TENANT);

    let params = [
        ("client_id", MICROSOFT_CLIENT_ID.to_string()),
        ("scope", MICROSOFT_SCOPES.to_string()),
    ];

    let client = reqwest::blocking::Client::new();
    client
        .post(endpoint)
        .form(&params)
        .send()
        .map_err(|err| format!("No se pudo iniciar device code con Microsoft: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Microsoft rechazó inicio de device code: {err}"))?
        .json::<MicrosoftDeviceCodeResponse>()
        .map_err(|err| format!("No se pudo leer respuesta de device code de Microsoft: {err}"))
}

pub fn exchange_device_code(device_code: &str) -> Result<MicrosoftTokenResponse, String> {
    let token_endpoint = format!("{}/{}/oauth2/v2.0/token", TOKEN_BASE_URL, MICROSOFT_TENANT);
    let params = [
        ("client_id", MICROSOFT_CLIENT_ID.to_string()),
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:device_code".to_string(),
        ),
        ("device_code", device_code.to_string()),
    ];

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .map_err(|err| format!("No se pudo consultar estado del device code: {err}"))?;

    if response.status().is_success() {
        return response
            .json::<MicrosoftTokenResponse>()
            .map_err(|err| format!("No se pudo deserializar token de Microsoft: {err}"));
    }

    let status = response.status();
    let parsed = response
        .json::<MicrosoftAuthErrorResponse>()
        .map_err(|err| format!("No se pudo deserializar error de Microsoft ({status}): {err}"))?;

    let description = parsed
        .error_description
        .unwrap_or_else(|| "Sin detalle adicional".to_string());
    Err(format!("{}: {}", parsed.error, description))
}

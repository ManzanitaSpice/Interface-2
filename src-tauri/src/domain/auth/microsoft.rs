use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::domain::auth::tokens::MicrosoftTokenResponse;

pub const MICROSOFT_CLIENT_ID: &str = "7ce1b3e8-48d7-4a9d-9329-7e11f988df39";
pub const MICROSOFT_TENANT: &str = "consumers";
pub const MICROSOFT_REDIRECT_URI: &str = "http://localhost";
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

pub fn build_authorize_url(code_verifier: &str) -> Result<String, String> {
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
        redirect_uri: MICROSOFT_REDIRECT_URI,
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
) -> Result<MicrosoftTokenResponse, String> {
    if code_verifier.trim().is_empty() {
        return Err("code_verifier vacío en intercambio de token Microsoft.".to_string());
    }

    let token_endpoint = format!("{}/{}/oauth2/v2.0/token", TOKEN_BASE_URL, MICROSOFT_TENANT);

    let params = [
        ("client_id", MICROSOFT_CLIENT_ID.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", MICROSOFT_REDIRECT_URI.to_string()),
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

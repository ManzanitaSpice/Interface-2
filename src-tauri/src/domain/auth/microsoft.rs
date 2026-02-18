use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::auth::tokens::MicrosoftTokenResponse;

pub const MICROSOFT_CLIENT_ID: &str = "7ce1b3e8-48d7-4a9d-9329-7e11f988df39";
pub const MICROSOFT_SCOPES: &str = "XboxLive.signin offline_access";
pub const MICROSOFT_REDIRECT_URI: &str =
    "https://login.microsoftonline.com/common/oauth2/nativeclient";

const AUTHORIZE_ENDPOINT: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

pub fn generate_code_verifier() -> String {
    let random = format!(
        "{}{}{}{}",
        uuid::Uuid::new_v4().as_simple(),
        uuid::Uuid::new_v4().as_simple(),
        uuid::Uuid::new_v4().as_simple(),
        uuid::Uuid::new_v4().as_simple()
    );
    random.chars().take(128).collect()
}

fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[derive(Debug, Deserialize)]
struct MicrosoftAuthErrorResponse {
    error: String,
    error_description: Option<String>,
}

pub fn build_authorize_url(code_verifier: &str, redirect_uri: &str) -> Result<String, String> {
    let verifier_len = code_verifier.len();
    if !(43..=128).contains(&verifier_len) {
        return Err(format!(
            "El code_verifier para PKCE debe tener entre 43 y 128 caracteres (actual: {verifier_len})."
        ));
    }

    #[derive(Serialize)]
    struct Query<'a> {
        response_type: &'a str,
        client_id: &'a str,
        redirect_uri: &'a str,
        scope: &'a str,
        code_challenge: String,
        code_challenge_method: &'a str,
    }

    let query = Query {
        response_type: "code",
        client_id: MICROSOFT_CLIENT_ID,
        redirect_uri,
        scope: MICROSOFT_SCOPES,
        code_challenge: code_challenge(code_verifier),
        code_challenge_method: "S256",
    };

    let encoded_query = serde_urlencoded::to_string(query)
        .map_err(|err| format!("No se pudo construir la URL OAuth de Microsoft: {err}"))?;

    Ok(format!("{AUTHORIZE_ENDPOINT}?{encoded_query}"))
}

pub async fn exchange_authorization_code(
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<MicrosoftTokenResponse, String> {
    let verifier_len = code_verifier.len();
    if !(43..=128).contains(&verifier_len) {
        return Err(format!(
            "code_verifier inválido en intercambio de token Microsoft: longitud {verifier_len}, esperado 43-128."
        ));
    }

    let params = [
        ("grant_type", "authorization_code".to_string()),
        ("client_id", MICROSOFT_CLIENT_ID.to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", code_verifier.to_string()),
    ];

    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|err| format!("No se pudo llamar a token endpoint de Microsoft: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let parsed = serde_json::from_str::<MicrosoftAuthErrorResponse>(&body).ok();
        let detail = parsed
            .map(|p| {
                let desc = p
                    .error_description
                    .unwrap_or_else(|| "Sin detalle adicional".to_string());
                format!("{}: {}", p.error, desc)
            })
            .unwrap_or(body);

        return Err(format!(
            "Token endpoint de Microsoft devolvió error HTTP: {status}. Detalle: {detail}"
        ));
    }

    response
        .json::<MicrosoftTokenResponse>()
        .await
        .map_err(|err| format!("No se pudo deserializar token de Microsoft: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{build_authorize_url, generate_code_verifier, MICROSOFT_REDIRECT_URI};

    #[test]
    fn pkce_code_verifier_has_valid_length() {
        let verifier = generate_code_verifier();
        assert!((43..=128).contains(&verifier.len()));
    }

    #[test]
    fn authorize_url_contains_required_oauth_parameters() {
        let verifier = "A".repeat(64);
        let url = build_authorize_url(&verifier, MICROSOFT_REDIRECT_URI).expect("url should build");

        assert!(url.starts_with("https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=7ce1b3e8-48d7-4a9d-9329-7e11f988df39"));
        assert!(url.contains(
            "redirect_uri=https%3A%2F%2Flogin.microsoftonline.com%2Fcommon%2Foauth2%2Fnativeclient"
        ));
        assert!(url.contains("scope=XboxLive.signin+offline_access"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains("openid"));
        assert!(!url.contains("profile"));
        assert!(!url.contains("email"));
    }
}

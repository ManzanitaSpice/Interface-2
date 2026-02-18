use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::auth::tokens::MicrosoftTokenResponse;

pub const MICROSOFT_CLIENT_ID: &str = "7ce1b3e8-48d7-4a9d-9329-7e11f988df39";
pub const MICROSOFT_SCOPES: &str = "XboxLive.signin offline_access";
pub const MICROSOFT_REDIRECT_URI: &str =
    "https://login.microsoftonline.com/common/oauth2/nativeclient";

const AUTHORIZE_ENDPOINT: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";

const TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

/* =========================================================
   PKCE
========================================================= */

pub fn generate_code_verifier() -> String {
    let raw = format!(
        "{}{}{}{}",
        Uuid::new_v4().as_simple(),
        Uuid::new_v4().as_simple(),
        Uuid::new_v4().as_simple(),
        Uuid::new_v4().as_simple()
    );

    raw.chars().take(128).collect()
}

fn generate_code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/* =========================================================
   AUTHORIZE URL
========================================================= */

pub fn build_authorize_url(code_verifier: &str) -> Result<String, String> {
    validate_verifier(code_verifier)?;

    Ok(format!(
        "{AUTHORIZE_ENDPOINT}?client_id={}&response_type=code&redirect_uri={}&response_mode=query&scope=XboxLive.signin%20offline_access&code_challenge={}&code_challenge_method=S256&prompt=select_account",
        MICROSOFT_CLIENT_ID,
        urlencoding::encode(MICROSOFT_REDIRECT_URI),
        generate_code_challenge(code_verifier)
    ))
}

/* =========================================================
   TOKEN EXCHANGE
========================================================= */

fn validate_verifier(verifier: &str) -> Result<(), String> {
    let len = verifier.len();
    if !(43..=128).contains(&len) {
        return Err(format!(
            "code_verifier inválido: longitud {len}, esperado 43-128"
        ));
    }
    Ok(())
}

fn build_token_params(code: &str, verifier: &str) -> Result<[(&'static str, String); 6], String> {
    validate_verifier(verifier)?;

    Ok([
        ("grant_type", "authorization_code".to_string()),
        ("client_id", MICROSOFT_CLIENT_ID.to_string()),
        ("code", code.to_string()),
        ("redirect_uri", MICROSOFT_REDIRECT_URI.to_string()),
        ("code_verifier", verifier.to_string()),
        ("scope", MICROSOFT_SCOPES.to_string()),
    ])
}

#[derive(Debug, Deserialize)]
struct MicrosoftAuthError {
    error: String,
    error_description: Option<String>,
}

pub async fn exchange_authorization_code(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
) -> Result<MicrosoftTokenResponse, String> {
    let params = build_token_params(code, verifier)?;

    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Error llamando token endpoint: {e}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        if let Ok(parsed) = serde_json::from_str::<MicrosoftAuthError>(&body) {
            let detail = parsed
                .error_description
                .unwrap_or_else(|| "Sin detalle adicional".to_string());
            return Err(format!(
                "Microsoft OAuth error {}: {}",
                parsed.error, detail
            ));
        }

        return Err(format!(
            "Microsoft token endpoint HTTP {}: {}",
            status, body
        ));
    }

    serde_json::from_str::<MicrosoftTokenResponse>(&body)
        .map_err(|e| format!("Error deserializando MicrosoftTokenResponse: {e}"))
}

/* =========================================================
   TESTS
========================================================= */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_has_valid_length() {
        let v = generate_code_verifier();
        assert!((43..=128).contains(&v.len()));
    }

    #[test]
    fn authorize_url_is_valid() {
        let verifier = "A".repeat(64);
        let url = build_authorize_url(&verifier).unwrap();

        assert!(url.starts_with(AUTHORIZE_ENDPOINT));
        assert!(!url.contains('\n'));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("response_mode=query"));
        assert!(url.contains("client_id=7ce1b3e8-48d7-4a9d-9329-7e11f988df39"));
        assert!(url.contains("scope=XboxLive.signin%20offline_access"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("prompt=select_account"));
    }

    #[test]
    fn token_params_are_correct() {
        let params = build_token_params("abc", "A".repeat(64).as_str()).unwrap();

        assert!(params.iter().any(|(k, _)| *k == "client_id"));
        assert!(params
            .iter()
            .any(|(k, v)| *k == "grant_type" && v == "authorization_code"));
        assert!(params.iter().any(|(k, _)| *k == "code_verifier"));
    }
}

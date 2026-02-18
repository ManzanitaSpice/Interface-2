use sha2::{Digest, Sha256};

use crate::shared::result::AppResult;

pub fn parse_checksum(raw: &str) -> AppResult<String> {
    let maybe_hex = raw
        .split_whitespace()
        .find(|token| token.len() == 64 && token.chars().all(|ch| ch.is_ascii_hexdigit()));

    maybe_hex
        .map(|value| value.to_string())
        .ok_or_else(|| "Checksum remoto inválido: no contiene un SHA-256 legible.".to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

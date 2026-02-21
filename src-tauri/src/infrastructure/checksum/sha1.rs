use std::{fs::File, io::Read, path::Path};

use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha256;

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

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn compute_file_sha1(path: &Path) -> AppResult<String> {
    let mut file = File::open(path).map_err(|err| {
        format!(
            "No se pudo abrir archivo para SHA1 {}: {err}",
            path.display()
        )
    })?;

    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 65_536];
    loop {
        let bytes_read = file.read(&mut buffer).map_err(|err| {
            format!(
                "No se pudo leer archivo para SHA1 {}: {err}",
                path.display()
            )
        })?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub async fn verify_sha1_async(path: &Path, expected: &str) -> AppResult<bool> {
    let path_buf = path.to_path_buf();
    let expected = expected.to_string();

    tokio::task::spawn_blocking(move || {
        let actual = compute_file_sha1(&path_buf)?;
        Ok(actual.eq_ignore_ascii_case(&expected))
    })
    .await
    .map_err(|err| {
        format!(
            "Error en verificación SHA1 async para {}: {err}",
            path.display()
        )
    })?
}

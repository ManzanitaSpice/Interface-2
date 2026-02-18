use crate::shared::result::AppResult;

pub fn validate_checksum(
    expected_checksum: &str,
    actual_checksum: &str,
    java_major: u8,
) -> AppResult<()> {
    if actual_checksum.eq_ignore_ascii_case(expected_checksum) {
        Ok(())
    } else {
        Err(format!(
            "Checksum inválido para Java {}. Esperado: {}, obtenido: {}",
            java_major, expected_checksum, actual_checksum
        ))
    }
}

use crate::shared::result::AppResult;

pub fn detect_architecture() -> AppResult<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("aarch64"),
        other => Err(format!(
            "Arquitectura no soportada para Java embebido: {other}"
        )),
    }
}

use image::ImageFormat;

pub fn validate_skin_png(bytes: &[u8]) -> Result<(u32, u32), String> {
    let format = image::guess_format(bytes).map_err(|err| format!("No se pudo detectar formato: {err}"))?;
    if format != ImageFormat::Png {
        return Err("El archivo debe ser PNG".into());
    }

    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|err| format!("No se pudo leer PNG: {err}"))?;
    let (width, height) = (image.width(), image.height());
    let valid = (width == 64 && height == 64) || (width == 64 && height == 32);
    if !valid {
        return Err(format!("Dimensiones inválidas {width}x{height}. Usa 64x64 o 64x32"));
    }
    Ok((width, height))
}

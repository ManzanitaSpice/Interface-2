use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};

#[tauri::command]
pub fn optimize_skin_png(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    crate::commands::validator::validate_skin_png(&bytes)?;

    let image = image::load_from_memory(&bytes).map_err(|err| format!("No se pudo leer imagen: {err}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut output = Vec::<u8>::new();
    let encoder = PngEncoder::new(&mut output);
    encoder
        .write_image(&rgba, width, height, ColorType::Rgba8.into())
        .map_err(|err| format!("No se pudo optimizar PNG: {err}"))?;

    Ok(output)
}

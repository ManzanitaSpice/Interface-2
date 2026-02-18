use reqwest::blocking::Client;

use crate::{
    domain::models::java::JavaRuntime,
    platform::{linux::current_os, windows::detect_architecture},
    shared::result::AppResult,
};

#[derive(Debug, serde::Deserialize)]
struct AdoptiumBinaryPackage {
    link: String,
    checksum: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    checksum_link: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumBinaryPackage,
}

#[derive(Debug, serde::Deserialize)]
struct AdoptiumRelease {
    #[serde(default)]
    binary: Option<AdoptiumBinary>,
    #[serde(default)]
    binaries: Vec<AdoptiumBinary>,
}

pub fn build_http_client() -> AppResult<Client> {
    Client::builder()
        .user_agent("InterfaceLauncher/0.1")
        .build()
        .map_err(|err| format!("No se pudo crear cliente HTTP: {err}"))
}

pub fn resolve_temurin_asset(
    client: &Client,
    runtime: JavaRuntime,
) -> AppResult<(String, String, String)> {
    let arch = detect_architecture()?;
    let os = current_os();

    let mut last_error = String::new();
    for image_type in ["jre", "jdk"] {
        let api = format!(
            "https://api.adoptium.net/v3/assets/latest/{}/hotspot?architecture={}&image_type={}&os={}",
            runtime.major(), arch, image_type, os
        );

        let releases = client
            .get(&api)
            .send()
            .and_then(|resp| resp.error_for_status())
            .map_err(|err| format!("No se pudo consultar catálogo de Temurin: {err}"))?
            .json::<Vec<AdoptiumRelease>>()
            .map_err(|err| format!("Respuesta inválida del catálogo de Temurin: {err}"))?;

        if let Some(package) = releases
            .into_iter()
            .find_map(|release| {
                release
                    .binary
                    .or_else(|| release.binaries.into_iter().next())
            })
            .map(|binary| binary.package)
        {
            return build_asset_tuple(client, package);
        }

        last_error = format!(
            "Sin releases para Java {} con image_type={image_type} ({api}).",
            runtime.major()
        );
    }

    Err(format!(
        "No se encontró release de Temurin para el runtime solicitado. {last_error}"
    ))
}

fn build_asset_tuple(
    client: &Client,
    package: AdoptiumBinaryPackage,
) -> AppResult<(String, String, String)> {
    let download_link = package.link;
    let file_name = if package.name.trim().is_empty() {
        download_link
            .rsplit('/')
            .next()
            .unwrap_or("runtime-archive")
            .to_string()
    } else {
        package.name
    };

    let checksum = if package.checksum.trim().is_empty() {
        let checksum_link = package
            .checksum_link
            .ok_or_else(|| "Release sin checksum disponible.".to_string())?;

        let checksum_body = client
            .get(&checksum_link)
            .send()
            .and_then(|resp| resp.error_for_status())
            .map_err(|err| format!("No se pudo leer checksum remoto: {err}"))?
            .text()
            .map_err(|err| format!("No se pudo parsear checksum remoto: {err}"))?;

        crate::infrastructure::checksum::sha1::parse_checksum(&checksum_body)?
    } else {
        package.checksum
    };

    Ok((download_link, checksum, file_name))
}

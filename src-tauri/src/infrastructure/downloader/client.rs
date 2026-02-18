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
    binary: AdoptiumBinary,
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

    let image_type = "jdk";
    let jvm_impl = "hotspot";
    let release_type = "ga";
    let vendor = "eclipse";

    let api = format!(
        "https://api.adoptium.net/v3/assets/feature_releases/{}/ga?architecture={}&heap_size=normal&image_type={}&jvm_impl={}&os={}&page=0&page_size=1&project=jdk&release_type={}&sort_method=DEFAULT&sort_order=DESC&vendor={}",
        runtime.major(), arch, image_type, jvm_impl, os, release_type, vendor
    );

    let releases = client
        .get(&api)
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|err| format!("No se pudo consultar catálogo de Temurin: {err}"))?
        .json::<Vec<AdoptiumRelease>>()
        .map_err(|err| format!("Respuesta inválida del catálogo de Temurin: {err}"))?;

    let release = releases.into_iter().next().ok_or_else(|| {
        "No se encontró release de Temurin para el runtime solicitado.".to_string()
    })?;

    let download_link = release.binary.package.link;
    let file_name = if release.binary.package.name.trim().is_empty() {
        download_link
            .rsplit('/')
            .next()
            .unwrap_or("runtime-archive")
            .to_string()
    } else {
        release.binary.package.name
    };

    let checksum = if release.binary.package.checksum.trim().is_empty() {
        let checksum_link = release
            .binary
            .package
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
        release.binary.package.checksum
    };

    Ok((download_link, checksum, file_name))
}

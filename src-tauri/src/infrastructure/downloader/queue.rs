use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use reqwest::blocking::Client;

use crate::{infrastructure::checksum::sha1::compute_file_sha1, shared::result::AppResult};

const OFFICIAL_BINARY_HOSTS: [&str; 24] = [
    // Mojang / Microsoft
    "launchermeta.mojang.com",
    "launcher.mojang.com",
    "resources.download.minecraft.net",
    "libraries.minecraft.net",
    "piston-data.mojang.com",
    "piston-meta.mojang.com",
    "assets.minecraft.net",
    "authserver.mojang.com",
    "sessionserver.mojang.com",
    "api.mojang.com",
    "api.minecraftservices.com",
    // Forge / NeoForge
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
    // Fabric / Quilt
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    // Official upstream Maven/CDN used by loaders
    "repo1.maven.org",
    "repo.maven.apache.org",
    "jcenter.bintray.com",
    "dl.google.com",
    "oss.sonatype.org",
    "s3.amazonaws.com",
];

fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn is_official_binary_host(host: &str) -> bool {
    let normalized_host = normalize_host(host);
    OFFICIAL_BINARY_HOSTS.iter().any(|allowed| {
        normalized_host == *allowed || normalized_host.ends_with(&format!(".{allowed}"))
    })
}

#[derive(Clone, Debug)]
pub struct DownloadJob {
    pub url: String,
    pub target_path: PathBuf,
    pub expected_sha1: String,
    pub label: String,
}

pub fn official_timeout() -> Duration {
    let configured = std::env::var("MINECRAFT_DOWNLOAD_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_secs(configured.max(30))
}

pub fn official_retries() -> usize {
    std::env::var("MINECRAFT_DOWNLOAD_RETRIES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(3)
        .max(3)
}

pub fn build_official_client() -> AppResult<Client> {
    Client::builder()
        .timeout(official_timeout())
        .connect_timeout(Duration::from_secs(30))
        .tcp_keepalive(Duration::from_secs(60))
        .user_agent("InterfaceLauncher/0.1")
        .build()
        .map_err(|err| format!("No se pudo construir cliente HTTP oficial de Minecraft: {err}"))
}

pub fn ensure_official_binary_url(url: &str) -> AppResult<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| format!("URL de descarga inválida: {url}. Error: {err}"))?;
    let host = parsed.host_str().unwrap_or_default();

    if !is_official_binary_host(host) {
        return Err(format!(
            "Host de descarga bloqueado por política oficial: {host}. URL: {url}"
        ));
    }

    Ok(())
}

pub fn download_with_retry(
    client: &Client,
    url: &str,
    target_path: &Path,
    expected_sha1: &str,
    force: bool,
) -> AppResult<bool> {
    ensure_official_binary_url(url)?;

    if target_path.exists() && !force {
        if expected_sha1.is_empty() {
            return Ok(false);
        }

        let current_sha1 = compute_file_sha1(target_path)?;
        if current_sha1.eq_ignore_ascii_case(expected_sha1) {
            return Ok(false);
        }

        let _ = fs::remove_file(target_path);
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo crear directorio para descarga {}: {err}",
                parent.display()
            )
        })?;
    }

    let mut last_error = String::new();
    let max_attempts = official_retries();
    for attempt in 1..=max_attempts {
        match perform_download(client, url, target_path, expected_sha1) {
            Ok(()) => return Ok(true),
            Err(err) => {
                last_error = err;
                let temp = temp_path_for(target_path);
                let _ = fs::remove_file(temp);

                if attempt < max_attempts {
                    let wait_secs = 2u64.pow(attempt as u32);
                    thread::sleep(Duration::from_secs(wait_secs));
                }
            }
        }
    }

    Err(format!(
        "Fallo al descargar recurso oficial tras {} intentos: {}",
        max_attempts, last_error
    ))
}

fn temp_path_for(target_path: &Path) -> PathBuf {
    target_path.with_extension("tmp")
}

fn perform_download(
    client: &Client,
    url: &str,
    target_path: &Path,
    expected_sha1: &str,
) -> AppResult<()> {
    let response = client
        .get(url)
        .send()
        .map_err(|err| explain_network_error(url, &err))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {} al descargar {}", status.as_u16(), url));
    }

    let temp_path = temp_path_for(target_path);
    let mut response = response;
    let mut temp_file = fs::File::create(&temp_path).map_err(|err| {
        format!(
            "No se pudo crear archivo temporal {}: {err}",
            temp_path.display()
        )
    })?;

    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    let mut buffer = vec![0u8; 65_536];
    loop {
        let bytes_read = response
            .read(&mut buffer)
            .map_err(|err| format!("No se pudo leer respuesta HTTP de {url}: {err}"))?;
        if bytes_read == 0 {
            break;
        }
        temp_file.write_all(&buffer[..bytes_read]).map_err(|err| {
            format!(
                "No se pudo escribir archivo temporal {}: {err}",
                temp_path.display()
            )
        })?;
        if !expected_sha1.is_empty() {
            hasher.update(&buffer[..bytes_read]);
        }
    }

    temp_file.flush().map_err(|err| {
        format!(
            "No se pudo hacer flush del archivo temporal {}: {err}",
            temp_path.display()
        )
    })?;

    let downloaded_sha1 = if expected_sha1.is_empty() {
        String::new()
    } else {
        format!("{:x}", hasher.finalize())
    };

    if !expected_sha1.is_empty() && !downloaded_sha1.eq_ignore_ascii_case(expected_sha1) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "SHA1 inválido para {}. Esperado {}, obtenido {}",
            url, expected_sha1, downloaded_sha1
        ));
    }

    fs::rename(&temp_path, target_path).map_err(|err| {
        format!(
            "No se pudo mover {} a {}: {err}",
            temp_path.display(),
            target_path.display()
        )
    })?;

    if !expected_sha1.is_empty() {
        let disk_sha1 = compute_file_sha1(target_path)?;
        if !disk_sha1.eq_ignore_ascii_case(expected_sha1) {
            let _ = fs::remove_file(target_path);
            return Err(format!(
                "SHA1 inválido tras escritura para {}. Esperado {}, obtenido {}",
                target_path.display(),
                expected_sha1,
                disk_sha1
            ));
        }
    }

    Ok(())
}

pub fn explain_network_error(url: &str, err: &reqwest::Error) -> String {
    if err.is_timeout() {
        return format!("Timeout al descargar {url}. Verifica latencia/red/firewall.");
    }

    if err.is_connect() {
        let raw = err.to_string().to_lowercase();
        if raw.contains("dns") || raw.contains("name") {
            return format!("Error DNS al resolver {url}: {err}");
        }
        if raw.contains("tls") || raw.contains("certificate") || raw.contains("ssl") {
            return format!("Error TLS/SSL al conectar con {url}: {err}");
        }
        return format!("Error de conexión/firewall hacia {url}: {err}");
    }

    format!("Error HTTP/IO al descargar {url}: {err}")
}

pub fn download_jobs_parallel(client: &Client, jobs: Vec<DownloadJob>) -> AppResult<Vec<String>> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = std::env::var("MINECRAFT_DOWNLOAD_PARALLELISM")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(6)
        .clamp(2, 12)
        .min(jobs.len());

    let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));
    let results: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let errors = Arc::clone(&errors);
            let local_client = client.clone();
            scope.spawn(move || loop {
                let next = {
                    let mut queue = queue.lock().expect("queue lock");
                    queue.pop_front()
                };

                let Some(job) = next else { break };

                match download_with_retry(
                    &local_client,
                    &job.url,
                    &job.target_path,
                    &job.expected_sha1,
                    false,
                ) {
                    Ok(_) => results.lock().expect("results lock").push(job.label),
                    Err(err) => errors
                        .lock()
                        .expect("errors lock")
                        .push(format!("{} => {}", job.url, err)),
                }
            });
        }
    });

    let errors = errors.lock().expect("errors lock");
    if !errors.is_empty() {
        return Err(errors.join(" | "));
    }

    let completed = results.lock().expect("results lock").clone();
    Ok(completed)
}

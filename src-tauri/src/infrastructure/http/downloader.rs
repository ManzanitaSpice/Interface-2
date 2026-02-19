use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::{header, Client};
use sha1::{Digest, Sha1};
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
    sync::{Mutex, Semaphore},
    time::sleep,
};

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub label: String,
}

#[derive(Debug, Default, Clone)]
pub struct BatchResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum DownloadError {
    Network(String),
    Io(String),
    Sha1Mismatch {
        expected: String,
        got: String,
        path: String,
    },
    SizeMismatch {
        expected: u64,
        got: u64,
    },
    MaxRetriesExceeded(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "Network error: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::Sha1Mismatch {
                expected,
                got,
                path,
            } => write!(
                f,
                "SHA1 mismatch for {path}. Expected {expected}, got {got}"
            ),
            Self::SizeMismatch { expected, got } => {
                write!(f, "Size mismatch. Expected {expected} bytes, got {got}")
            }
            Self::MaxRetriesExceeded(msg) => write!(f, "Max retries exceeded: {msg}"),
        }
    }
}

impl std::error::Error for DownloadError {}

pub fn build_download_client() -> Result<Arc<Client>, DownloadError> {
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::ACCEPT_ENCODING,
        header::HeaderValue::from_static("identity"),
    );

    Client::builder()
        .gzip(false)
        .brotli(false)
        .deflate(false)
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .default_headers(headers)
        .user_agent("MinecraftLauncher/1.0")
        .build()
        .map(Arc::new)
        .map_err(|err| DownloadError::Network(format!("failed to build HTTP client: {err}")))
}

pub async fn download_file(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    on_progress: impl Fn(u64, u64) + Send,
) -> Result<(), DownloadError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            DownloadError::Io(format!(
                "failed creating parent directory {} for {url}: {err}",
                parent.display()
            ))
        })?;
    }

    let tmp_path = dest.with_extension("tmp");
    let response = client.get(url).send().await.map_err(|err| {
        DownloadError::Network(format!(
            "request failed for {url} -> {}: {err}",
            dest.display()
        ))
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Network(format!(
            "HTTP {} while downloading {url} -> {}",
            status,
            dest.display()
        )));
    }

    let total = expected_size.or(response.content_length()).unwrap_or(0);
    let file = fs::File::create(&tmp_path).await.map_err(|err| {
        DownloadError::Io(format!(
            "failed creating temporary file {} for {url}: {err}",
            tmp_path.display()
        ))
    })?;

    let mut writer = BufWriter::new(file);
    let mut stream = response.bytes_stream();
    let mut downloaded = 0_u64;
    let mut hasher = Sha1::new();

    while let Some(next_chunk) = stream.next().await {
        let chunk = next_chunk.map_err(|err| {
            DownloadError::Network(format!(
                "stream read error while downloading {url} -> {}: {err}",
                tmp_path.display()
            ))
        })?;

        writer.write_all(&chunk).await.map_err(|err| {
            DownloadError::Io(format!(
                "failed writing chunk to {} from {url}: {err}",
                tmp_path.display()
            ))
        })?;

        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }

    writer.flush().await.map_err(|err| {
        DownloadError::Io(format!(
            "failed flushing temporary file {} for {url}: {err}",
            tmp_path.display()
        ))
    })?;

    if let Some(expected) = expected_size {
        if downloaded != expected {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(DownloadError::SizeMismatch {
                expected,
                got: downloaded,
            });
        }
    }

    if let Some(expected) = expected_sha1 {
        let got = format!("{:x}", hasher.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(DownloadError::Sha1Mismatch {
                expected: expected.to_owned(),
                got,
                path: dest.display().to_string(),
            });
        }
    }

    fs::rename(&tmp_path, dest).await.map_err(|err| {
        DownloadError::Io(format!(
            "failed moving temporary file {} to {} for {url}: {err}",
            tmp_path.display(),
            dest.display()
        ))
    })?;

    Ok(())
}

pub async fn download_with_retry(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    max_attempts: u8,
    on_progress: impl Fn(u64, u64) + Send + Clone,
) -> Result<(), DownloadError> {
    let total_attempts = max_attempts.max(3);
    let mut last_error = String::new();

    for attempt in 1..=total_attempts {
        let result = download_file(
            client,
            url,
            dest,
            expected_sha1,
            expected_size,
            on_progress.clone(),
        )
        .await;

        match result {
            Ok(()) => return Ok(()),
            Err(err) => {
                log::warn!(
                    "download attempt {attempt}/{total_attempts} failed for {} ({}): {}",
                    url,
                    dest.display(),
                    err
                );

                if !is_retryable(&err) {
                    return Err(err);
                }

                last_error = err.to_string();
                if attempt < total_attempts {
                    let backoff_secs = 1_u64 << (attempt - 1);
                    sleep(Duration::from_secs(backoff_secs)).await;
                }
            }
        }
    }

    Err(DownloadError::MaxRetriesExceeded(format!(
        "{url} -> {} ({last_error})",
        dest.display()
    )))
}

pub async fn download_batch(
    client: &Client,
    tasks: Vec<DownloadTask>,
    max_concurrent: usize,
    on_progress: impl Fn(u64, u64) + Send + Clone + 'static,
) -> Result<BatchResult, DownloadError> {
    let total_tasks = tasks.len() as u64;
    if total_tasks == 0 {
        return Ok(BatchResult::default());
    }

    let semaphore = Arc::new(Semaphore::new(max_concurrent.max(1)));
    let completed = Arc::new(Mutex::new(0_u64));
    let mut workers = FuturesUnordered::new();

    for task in tasks {
        let local_client = client.clone();
        let permit_pool = Arc::clone(&semaphore);
        let progress_cb = on_progress.clone();
        let completed_ref = Arc::clone(&completed);

        workers.push(tokio::spawn(async move {
            let permit = permit_pool
                .acquire_owned()
                .await
                .map_err(|err| DownloadError::Network(format!("semaphore closed: {err}")))?;

            let label = task.label.clone();
            let result = download_with_retry(
                &local_client,
                &task.url,
                &task.dest,
                task.sha1.as_deref(),
                task.size,
                3,
                |_downloaded, _total| {},
            )
            .await;

            drop(permit);

            let mut done = completed_ref.lock().await;
            *done += 1;
            progress_cb(*done, total_tasks);

            Ok::<(String, Result<(), DownloadError>), DownloadError>((label, result))
        }));
    }

    let mut batch_result = BatchResult::default();
    while let Some(joined) = workers.next().await {
        match joined {
            Ok(Ok((label, Ok(())))) => batch_result.succeeded.push(label),
            Ok(Ok((label, Err(err)))) => batch_result.failed.push((label, err.to_string())),
            Ok(Err(err)) => batch_result
                .failed
                .push(("batch-internal".to_string(), err.to_string())),
            Err(err) => batch_result
                .failed
                .push(("batch-join".to_string(), format!("join error: {err}"))),
        }
    }

    Ok(batch_result)
}

pub fn needs_download(path: &Path, expected_sha1: Option<&str>) -> bool {
    if !path.exists() {
        return true;
    }

    let Some(expected) = expected_sha1 else {
        return false;
    };

    match compute_sha1_sync(path) {
        Ok(got) => !got.eq_ignore_ascii_case(expected),
        Err(_) => true,
    }
}

pub async fn needs_download_async(path: &Path, expected_sha1: Option<&str>) -> bool {
    if !path.exists() {
        return true;
    }

    let Some(expected) = expected_sha1.map(ToOwned::to_owned) else {
        return false;
    };

    let path_buf = path.to_path_buf();
    match tokio::task::spawn_blocking(move || compute_sha1_sync(&path_buf)).await {
        Ok(Ok(got)) => !got.eq_ignore_ascii_case(&expected),
        Ok(Err(_)) | Err(_) => true,
    }
}

fn compute_sha1_sync(path: &Path) -> Result<String, DownloadError> {
    let mut file = std::fs::File::open(path).map_err(|err| {
        DownloadError::Io(format!(
            "failed opening file for SHA1 {}: {err}",
            path.display()
        ))
    })?;

    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = std::io::Read::read(&mut file, &mut buffer).map_err(|err| {
            DownloadError::Io(format!(
                "failed reading file for SHA1 {}: {err}",
                path.display()
            ))
        })?;

        if read == 0 {
            break;
        }

        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn is_retryable(error: &DownloadError) -> bool {
    matches!(error, DownloadError::Network(_))
}

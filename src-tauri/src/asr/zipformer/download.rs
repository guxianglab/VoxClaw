//! Zipformer model download.
//!
//! Downloads the bilingual zh-en streaming Zipformer model as a `.tar.bz2`
//! from the sherpa-onnx GitHub release and extracts it into the model
//! directory. Emits `asr_model_download` progress events (same UI as
//! SenseVoice/VAD downloads).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use bzip2::read::BzDecoder;
use futures_util::StreamExt;
use tauri::{AppHandle, Emitter, Runtime};
use tar::Archive;

use crate::asr::sensevoice::download::{DownloadEvent, emit};
use crate::http_client;
use crate::storage::ProxyConfig;

/// The model archive URL (bilingual zh-en streaming Zipformer, ~300 MB).
const MODEL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20.tar.bz2";

/// The directory name inside the archive (it extracts to this subfolder).
const ARCHIVE_DIR_NAME: &str = "sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20";

/// Download and extract the streaming Zipformer model into `parent_dir`.
/// The model files end up in `parent_dir/<ARCHIVE_DIR_NAME>/`.
/// Emits progress via the `asr_model_download` event.
pub async fn download_zipformer_model<R: Runtime>(
    app: &AppHandle<R>,
    parent_dir: PathBuf,
    proxy: ProxyConfig,
) -> Result<PathBuf> {
    let target_dir = parent_dir.join(ARCHIVE_DIR_NAME);

    // Skip if already present.
    if crate::asr::zipformer::model::is_present(&target_dir) {
        emit(app, DownloadEvent::Finished {
            dir: target_dir.display().to_string(),
        });
        return Ok(target_dir);
    }

    fs::create_dir_all(&parent_dir)
        .map_err(|e| anyhow!("create model dir failed: {e}"))?;

    let client = http_client::build_client(&proxy, 600)
        .map_err(|e| anyhow!("build http client failed: {e}"))?;

    emit(app, DownloadEvent::Started { total_files: 1 });

    // Stream to a temp .tar.bz2, then extract.
    let archive_path = parent_dir.join("zipformer-download.tar.bz2");
    if let Err(err) = stream_download(app, &client, &archive_path).await {
        emit(app, DownloadEvent::Failed { message: err.to_string() });
        let _ = fs::remove_file(&archive_path);
        return Err(err);
    }

    // Extract the tar.bz2.
    emit(app, DownloadEvent::File {
        name: "解压中…".to_string(),
        index: 1,
        total: 1,
        downloaded: 0,
        size: None,
    });

    if let Err(err) = extract(&archive_path, &parent_dir) {
        emit(app, DownloadEvent::Failed { message: err.to_string() });
        let _ = fs::remove_file(&archive_path);
        return Err(err);
    }

    // Clean up the archive.
    let _ = fs::remove_file(&archive_path);

    if !crate::asr::zipformer::model::is_present(&target_dir) {
        let missing = crate::asr::zipformer::model::missing_files(&target_dir).join(", ");
        let msg = format!("解压后模型文件仍不完整，缺少: {missing}");
        emit(app, DownloadEvent::Failed { message: msg.clone() });
        return Err(anyhow!(msg));
    }

    emit(app, DownloadEvent::Finished {
        dir: target_dir.display().to_string(),
    });
    Ok(target_dir)
}

async fn stream_download<R: Runtime>(
    app: &AppHandle<R>,
    client: &reqwest::Client,
    dest: &Path,
) -> Result<()> {
    let response = client
        .get(MODEL_URL)
        .send()
        .await
        .map_err(|e| anyhow!("request zipformer model failed: {e}"))?;
    if !response.status().is_success() {
        return Err(anyhow!("download zipformer failed: HTTP {}", response.status()));
    }
    let total_size = response.content_length();
    let mut file = fs::File::create(dest)
        .map_err(|e| anyhow!("create {} failed: {e}", dest.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow!("stream zipformer error: {e}"))?;
        file.write_all(&chunk)
            .map_err(|e| anyhow!("write zipformer failed: {e}"))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(300) {
            emit(app, DownloadEvent::File {
                name: "zipformer-model.tar.bz2".to_string(),
                index: 1,
                total: 1,
                downloaded,
                size: total_size,
            });
            last_emit = std::time::Instant::now();
        }
    }
    file.flush().ok();
    Ok(())
}

fn extract(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let tar_bz2 = fs::File::open(archive_path)
        .map_err(|e| anyhow!("open archive failed: {e}"))?;
    let bz = BzDecoder::new(tar_bz2);
    let mut tar = Archive::new(bz);
    tar.unpack(dest_dir)
        .map_err(|e| anyhow!("extract archive failed: {e}"))?;
    Ok(())
}

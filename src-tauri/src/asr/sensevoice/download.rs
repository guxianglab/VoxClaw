//! ModelScope downloader for SenseVoiceSmall-onnx.
//!
//! Streams each required file to `<target_dir>/<name>.part`, then atomically
//! renames it on completion. Emits Tauri `asr_model_download` events with
//! per-file progress so the frontend can render a progress bar without us
//! having to thread state back manually.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use super::model::REQUIRED_FILES;
use crate::storage::ProxyConfig;

const MODEL_REPO: &str = "iic/SenseVoiceSmall-onnx";
const REVISION: &str = "master";

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum DownloadEvent {
    Started { total_files: usize },
    File {
        name: String,
        index: usize,
        total: usize,
        downloaded: u64,
        size: Option<u64>,
    },
    Finished { dir: String },
    Failed { message: String },
}

fn emit<R: Runtime>(app: &AppHandle<R>, event: DownloadEvent) {
    let _ = app.emit("asr_model_download", &event);
}

/// Download every required SenseVoice file into `target_dir`. Idempotent: if a
/// file already exists with the expected size, it is skipped.
pub async fn download_model<R: Runtime>(
    app: &AppHandle<R>,
    target_dir: PathBuf,
    proxy: ProxyConfig,
) -> Result<PathBuf> {
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir)
            .map_err(|e| anyhow!("create model dir failed: {e}"))?;
    }

    let client = crate::http_client::build_client(&proxy, 600)
        .map_err(|e| anyhow!("build http client failed: {e}"))?;

    emit(app, DownloadEvent::Started {
        total_files: REQUIRED_FILES.len(),
    });

    for (idx, (name, expected_size)) in REQUIRED_FILES.iter().enumerate() {
        let dest = target_dir.join(name);
        if let Ok(meta) = fs::metadata(&dest) {
            if meta.is_file() && (*expected_size == 0 || meta.len() == *expected_size) {
                emit(app, DownloadEvent::File {
                    name: (*name).to_string(),
                    index: idx + 1,
                    total: REQUIRED_FILES.len(),
                    downloaded: meta.len(),
                    size: Some(meta.len()),
                });
                continue;
            }
        }

        if let Err(err) = download_one(app, &client, name, idx, &dest).await {
            emit(app, DownloadEvent::Failed {
                message: err.to_string(),
            });
            return Err(err);
        }
    }

    emit(app, DownloadEvent::Finished {
        dir: target_dir.display().to_string(),
    });
    Ok(target_dir)
}

async fn download_one<R: Runtime>(
    app: &AppHandle<R>,
    client: &reqwest::Client,
    name: &str,
    idx: usize,
    dest: &Path,
) -> Result<()> {
    // ModelScope's git-lfs resolve endpoint — stable download URL for all files.
    // (The old /api/v1/models/.../repo?Revision=…&FilePath=… endpoint now returns 403/404.)
    let url = format!(
        "https://www.modelscope.cn/models/{MODEL_REPO}/resolve/{REVISION}/{}",
        urlencoding::encode(name)
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("request {} failed: {e}", name))?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "download {} failed: HTTP {}",
            name,
            response.status()
        ));
    }

    let total_size = response.content_length();
    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|s| s.to_str()).unwrap_or("dl")
    ));

    let mut file = fs::File::create(&part_path)
        .map_err(|e| anyhow!("create {} failed: {e}", part_path.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_emit_at = std::time::Instant::now();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow!("stream {} error: {e}", name))?;
        file.write_all(&chunk)
            .map_err(|e| anyhow!("write {} failed: {e}", name))?;
        downloaded += chunk.len() as u64;

        if last_emit_at.elapsed() >= std::time::Duration::from_millis(150) {
            emit(app, DownloadEvent::File {
                name: name.to_string(),
                index: idx + 1,
                total: REQUIRED_FILES.len(),
                downloaded,
                size: total_size,
            });
            last_emit_at = std::time::Instant::now();
        }
    }

    file.flush().ok();
    drop(file);

    fs::rename(&part_path, dest)
        .map_err(|e| anyhow!("finalize {} failed: {e}", dest.display()))?;

    emit(app, DownloadEvent::File {
        name: name.to_string(),
        index: idx + 1,
        total: REQUIRED_FILES.len(),
        downloaded,
        size: total_size,
    });

    Ok(())
}

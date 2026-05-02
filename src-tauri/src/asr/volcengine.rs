//! Volcengine bigmodel SAUC streaming ASR provider.
//!
//! This file is a near-verbatim move of the original `src/asr.rs` (cloud
//! WebSocket implementation), adapted to the [`super::AsrProvider`] trait.
//! Behavioural changes vs. the original are intentionally minimal: only the
//! public surface (struct names, constructor, `finish_and_wait`) was reshaped.

use anyhow::{anyhow, Result};
use base64::Engine;
use flate2::write::GzEncoder;
use flate2::Compression;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::io::Write;
use std::thread::{self, JoinHandle};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_socks::tcp::{Socks4Stream, Socks5Stream};
use tokio_tungstenite::client_async_tls_with_config;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::storage::{OnlineAsrConfig, ProxyConfig};

use super::{AsrCapabilities, AsrProvider, AsrSession, AsrStreamParams};

const ASYNC_URL: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
const ASR_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const ASR_FINAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct VolcengineProvider {
    config: OnlineAsrConfig,
    proxy: ProxyConfig,
}

impl VolcengineProvider {
    pub fn new(config: OnlineAsrConfig, proxy: ProxyConfig) -> Self {
        Self { config, proxy }
    }
}

impl AsrProvider for VolcengineProvider {
    fn name(&self) -> &'static str {
        "volcengine"
    }

    fn capabilities(&self) -> AsrCapabilities {
        AsrCapabilities {
            streaming: true,
            offline: false,
            languages: vec!["zh-CN".to_string()],
            supports_diarization: false,
        }
    }

    fn start_streaming(&self, params: AsrStreamParams) -> Result<Box<dyn AsrSession>> {
        let AsrStreamParams {
            audio_rx,
            sample_rate,
            on_update,
        } = params;
        let config = self.config.clone();
        let proxy = self.proxy.clone();

        let (tx, mut async_rx) = mpsc::channel::<Vec<f32>>(100);

        // Bridge sync mpsc → async mpsc.
        let tx_clone = tx.clone();
        thread::spawn(move || {
            while let Ok(data) = audio_rx.recv() {
                if tx_clone.blocking_send(data).is_err() {
                    break;
                }
            }
        });

        // Main streaming thread: owns its own current-thread Tokio runtime.
        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                let mut request = ASYNC_URL.into_client_request().unwrap();
                {
                    let headers = request.headers_mut();
                    headers.insert(
                        "X-Api-App-Key",
                        HeaderValue::from_str(&config.app_key).unwrap(),
                    );
                    headers.insert(
                        "X-Api-Access-Key",
                        HeaderValue::from_str(&config.access_key).unwrap(),
                    );
                    headers.insert(
                        "X-Api-Resource-Id",
                        HeaderValue::from_str(&config.resource_id).unwrap(),
                    );
                    headers.insert(
                        "X-Api-Connect-Id",
                        HeaderValue::from_str(&uuid::Uuid::new_v4().to_string()).unwrap(),
                    );
                }

                let (ws_stream, response) =
                    match timeout(ASR_CONNECT_TIMEOUT, connect_ws(request, &proxy)).await {
                        Ok(Ok(res)) => res,
                        Ok(Err(e)) => {
                            eprintln!("[ASR] WebSocket connection failed: {}", e);
                            return Err(e);
                        }
                        Err(_) => {
                            let err = anyhow!(
                                "ASR websocket connection timed out after {}s",
                                ASR_CONNECT_TIMEOUT.as_secs()
                            );
                            eprintln!("[ASR] {}", err);
                            return Err(err);
                        }
                    };

                if let Some(logid) = response
                    .headers()
                    .get("X-Tt-Logid")
                    .and_then(|v| v.to_str().ok())
                {
                    println!("[ASR] Connected, X-Tt-Logid={}", logid);
                }

                let (mut write, mut read) = ws_stream.split();

                // Send full client request
                let req_payload = json!({
                    "user": {
                        "uid": "voxclaw-user"
                    },
                    "audio": {
                        "format": "pcm",
                        "codec": "raw",
                        "rate": sample_rate,
                        "bits": 16,
                        "channel": 1,
                        "language": "zh-CN"
                    },
                    "request": {
                        "model_name": "bigmodel",
                        "enable_itn": true,
                        "enable_punc": true,
                        "show_utterances": true,
                        "result_type": "full",
                        "enable_nonstream": true
                    }
                })
                .to_string();

                let compressed = gzip_compress(req_payload.as_bytes());
                let mut msg = Vec::new();
                let _header: u32 = 0x11101100;
                msg.extend_from_slice(&[0x11, 0x10, 0x11, 0x00]);
                let payload_size = compressed.len() as u32;
                msg.extend_from_slice(&payload_size.to_be_bytes());
                msg.extend_from_slice(&compressed);

                if let Err(e) = write.send(Message::Binary(msg.into())).await {
                    eprintln!("[ASR] Failed to send full client request: {}", e);
                    return Err(anyhow!("Failed to send ASR init request: {}", e));
                }

                let mut seq: i32 = 2;
                let read_task = tokio::spawn(async move {
                    let mut final_text = String::new();
                    let mut latest_text = String::new();
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Binary(data)) => {
                                if data.len() < 4 {
                                    continue;
                                }
                                let header_len = ((data[0] & 0x0F) * 4) as usize;
                                if data.len() < header_len {
                                    continue;
                                }
                                let msg_type = (data[1] >> 4) & 0x0F;
                                let flags = data[1] & 0x0F;
                                let is_compressed = (data[2] & 0x0F) == 0b0001;

                                let mut offset = header_len;
                                if msg_type == 0b1111 {
                                    offset += 4;
                                } else if flags == 0b0001 || flags == 0b0011 {
                                    offset += 4;
                                }

                                if data.len() < offset + 4 {
                                    continue;
                                }
                                let payload_size = u32::from_be_bytes(
                                    data[offset..offset + 4].try_into().unwrap(),
                                ) as usize;
                                let payload_offset = offset + 4;

                                if data.len() < payload_offset + payload_size {
                                    continue;
                                }
                                let payload = &data[payload_offset..payload_offset + payload_size];

                                let decompressed = if is_compressed {
                                    gzip_decompress(payload)
                                } else {
                                    payload.to_vec()
                                };

                                if msg_type == 0b1001 || msg_type == 0b1011 {
                                    if let Ok(json_str) = String::from_utf8(decompressed) {
                                        if let Ok(json_val) =
                                            serde_json::from_str::<serde_json::Value>(&json_str)
                                        {
                                            if let Some(result) = json_val.get("result") {
                                                if let Some(text) =
                                                    result.get("text").and_then(|t| t.as_str())
                                                {
                                                    latest_text = text.to_string();
                                                    if msg_type == 0b1001 {
                                                        final_text = latest_text.clone();
                                                    }
                                                    on_update(latest_text.clone());
                                                }
                                            }
                                        }
                                    }
                                } else if msg_type == 0b1111 {
                                    let msg_str = String::from_utf8_lossy(&decompressed);
                                    eprintln!("[ASR] Server error: {}", msg_str);
                                }
                            }
                            Ok(Message::Text(text)) => {
                                eprintln!("[ASR] Received Text msg: {}", text);
                            }
                            Ok(Message::Close(c)) => {
                                eprintln!("[ASR] Received Close: {:?}", c);
                            }
                            Err(e) => {
                                eprintln!("[ASR] Read error: {}", e);
                            }
                            _ => {}
                        }
                    }
                    if !final_text.is_empty()
                        && latest_text.starts_with(&final_text)
                        && latest_text.len() > final_text.len()
                    {
                        latest_text
                    } else if !final_text.is_empty() {
                        final_text
                    } else {
                        latest_text
                    }
                });

                let mut pending_chunk: Option<Vec<f32>> = None;
                while let Some(audio_chunk) = async_rx.recv().await {
                    if let Some(previous_chunk) = pending_chunk.replace(audio_chunk) {
                        let msg = build_audio_message(&previous_chunk, seq, false);
                        if write.send(Message::Binary(msg.into())).await.is_err() {
                            break;
                        }
                        seq += 1;
                    }
                }

                let final_msg = match pending_chunk {
                    Some(last_chunk) => build_audio_message(&last_chunk, seq, true),
                    None => build_empty_last_audio_message(seq),
                };
                let _ = write.send(Message::Binary(final_msg.into())).await;
                let _ = write.close().await;

                match timeout(ASR_FINAL_RESPONSE_TIMEOUT, read_task).await {
                    Ok(join_result) => Ok(join_result.unwrap_or_default()),
                    Err(_) => Err(anyhow!(
                        "ASR final response timed out after {}s",
                        ASR_FINAL_RESPONSE_TIMEOUT.as_secs()
                    )),
                }
            })
        });

        Ok(Box::new(VolcengineSession {
            handle: Some(handle),
            tx: Some(tx),
        }))
    }
}

pub struct VolcengineSession {
    handle: Option<JoinHandle<Result<String>>>,
    tx: Option<mpsc::Sender<Vec<f32>>>,
}

impl AsrSession for VolcengineSession {
    fn finish_and_wait(mut self: Box<Self>) -> Result<String> {
        // Drop the sender so the streaming task drains and closes the WS.
        self.tx.take();
        match self.handle.take() {
            Some(handle) => match handle.join() {
                Ok(result) => result,
                Err(_) => Err(anyhow!("Transcription thread panicked")),
            },
            None => Err(anyhow!("Session already finished")),
        }
    }
}

trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> AsyncIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

type BoxedStream = Box<dyn AsyncIo>;

async fn connect_ws(
    request: tungstenite::http::Request<()>,
    proxy: &ProxyConfig,
) -> Result<(
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<BoxedStream>>,
    tungstenite::handshake::client::Response,
)> {
    let stream = connect_stream(ASYNC_URL, proxy).await?;
    let response = client_async_tls_with_config(request, stream, None, None).await?;
    Ok(response)
}

async fn connect_stream(url: &str, proxy: &ProxyConfig) -> Result<BoxedStream> {
    let target = reqwest::Url::parse(url)?;
    let host = target
        .host_str()
        .ok_or_else(|| anyhow!("ASR URL missing host"))?
        .to_string();
    let port = target
        .port_or_known_default()
        .ok_or_else(|| anyhow!("ASR URL missing port"))?;
    let target_addr = format!("{host}:{port}");

    if !proxy.enabled || proxy.url.trim().is_empty() {
        return Ok(Box::new(TcpStream::connect(&target_addr).await?));
    }

    let proxy_url = reqwest::Url::parse(proxy.url.trim())?;
    match proxy_url.scheme() {
        "http" => connect_http_proxy(&proxy_url, &host, port).await,
        "socks5" | "socks5h" => connect_socks5_proxy(&proxy_url, &host, port).await,
        "socks4" | "socks4a" => connect_socks4_proxy(&proxy_url, &host, port).await,
        scheme => Err(anyhow!(
            "Unsupported proxy scheme for ASR websocket: {scheme}"
        )),
    }
}

async fn connect_http_proxy(
    proxy_url: &reqwest::Url,
    host: &str,
    port: u16,
) -> Result<BoxedStream> {
    let proxy_addr = format!(
        "{}:{}",
        proxy_url
            .host_str()
            .ok_or_else(|| anyhow!("Proxy URL missing host"))?,
        proxy_url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("Proxy URL missing port"))?
    );
    let mut stream = TcpStream::connect(proxy_addr).await?;

    let mut connect_req = format!(
        "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Connection: Keep-Alive\r\n"
    );

    if !proxy_url.username().is_empty() || proxy_url.password().is_some() {
        let credentials = format!(
            "{}:{}",
            proxy_url.username(),
            proxy_url.password().unwrap_or("")
        );
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        connect_req.push_str(&format!("Proxy-Authorization: Basic {encoded}\r\n"));
    }

    connect_req.push_str("\r\n");
    stream.write_all(connect_req.as_bytes()).await?;
    stream.flush().await?;

    let mut response = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(anyhow!("Proxy closed connection before CONNECT completed"));
        }
        response.extend_from_slice(&chunk[..read]);
        if response.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if response.len() > 16 * 1024 {
            return Err(anyhow!("Proxy CONNECT response too large"));
        }
    }

    let head = String::from_utf8_lossy(&response);
    let status_line = head.lines().next().unwrap_or_default();
    if !status_line.contains(" 200 ") {
        return Err(anyhow!("HTTP proxy CONNECT failed: {status_line}"));
    }

    Ok(Box::new(stream))
}

async fn connect_socks5_proxy(
    proxy_url: &reqwest::Url,
    host: &str,
    port: u16,
) -> Result<BoxedStream> {
    let proxy_addr = format!(
        "{}:{}",
        proxy_url
            .host_str()
            .ok_or_else(|| anyhow!("Proxy URL missing host"))?,
        proxy_url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("Proxy URL missing port"))?
    );
    let target_addr = format!("{host}:{port}");

    let stream = if !proxy_url.username().is_empty() || proxy_url.password().is_some() {
        Socks5Stream::connect_with_password(
            proxy_addr.as_str(),
            target_addr.as_str(),
            proxy_url.username(),
            proxy_url.password().unwrap_or(""),
        )
        .await?
        .into_inner()
    } else {
        Socks5Stream::connect(proxy_addr.as_str(), target_addr.as_str())
            .await?
            .into_inner()
    };

    Ok(Box::new(stream))
}

async fn connect_socks4_proxy(
    proxy_url: &reqwest::Url,
    host: &str,
    port: u16,
) -> Result<BoxedStream> {
    let proxy_addr = format!(
        "{}:{}",
        proxy_url
            .host_str()
            .ok_or_else(|| anyhow!("Proxy URL missing host"))?,
        proxy_url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("Proxy URL missing port"))?
    );
    let target_addr = format!("{host}:{port}");

    let stream = if !proxy_url.username().is_empty() {
        Socks4Stream::connect_with_userid(
            proxy_addr.as_str(),
            target_addr.as_str(),
            proxy_url.username(),
        )
        .await?
        .into_inner()
    } else {
        Socks4Stream::connect(proxy_addr.as_str(), target_addr.as_str())
            .await?
            .into_inner()
    };

    Ok(Box::new(stream))
}

fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn gzip_decompress(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut result = Vec::new();
    let _ = decoder.read_to_end(&mut result);
    result
}

fn build_audio_message(samples: &[f32], seq: i32, is_last: bool) -> Vec<u8> {
    let pcm = float_to_pcm16(samples);
    let mut pcm_bytes = Vec::with_capacity(pcm.len() * 2);
    for sample in pcm {
        pcm_bytes.extend_from_slice(&sample.to_le_bytes());
    }
    build_audio_message_from_pcm_bytes(&pcm_bytes, seq, is_last)
}

fn build_empty_last_audio_message(seq: i32) -> Vec<u8> {
    build_audio_message_from_pcm_bytes(&[], seq, true)
}

fn build_audio_message_from_pcm_bytes(pcm_bytes: &[u8], seq: i32, is_last: bool) -> Vec<u8> {
    let compressed = gzip_compress(pcm_bytes);
    let mut msg = Vec::new();
    msg.extend_from_slice(if is_last {
        &[0x11, 0x23, 0x01, 0x00]
    } else {
        &[0x11, 0x21, 0x01, 0x00]
    });
    msg.extend_from_slice(&(if is_last { -seq } else { seq }).to_be_bytes());
    let payload_size = compressed.len() as u32;
    msg.extend_from_slice(&payload_size.to_be_bytes());
    msg.extend_from_slice(&compressed);
    msg
}

fn float_to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * i16::MAX as f32) as i16
        })
        .collect()
}

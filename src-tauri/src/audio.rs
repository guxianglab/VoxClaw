use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

// ─────────────────────────────────────────────────────────────
//  常量
// ─────────────────────────────────────────────────────────────

/// 音频电平事件节流间隔 (毫秒)
const LEVEL_THROTTLE_MS: u64 = 16;
const STOP_DRAIN_MS: u64 = 120;

// ─────────────────────────────────────────────────────────────
//  公共数据类型
// ─────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

// ─────────────────────────────────────────────────────────────
//  共享状态 — 全部通过 Arc 在音频回调与外部之间共享
// ─────────────────────────────────────────────────────────────

/// 跨线程共享的音频状态，使用无锁原子 + 小粒度锁
struct SharedState {
    /// 是否「激活」采集 (测试/录音 都要把它设置为 true)
    active: AtomicBool,
    /// 当前采样率
    sample_rate: AtomicU32,
    /// 当前设备名
    device_name: Mutex<String>,
    /// 离线缓冲区 (录音结束后取走)
    buffer: Mutex<Vec<f32>>,
    /// 流式发送器 (实时传给 ASR)
    stream_tx: Mutex<Option<Sender<Vec<f32>>>>,
    /// 上次发送电平事件的时间戳 (ms)，用于节流
    last_emit_ms: AtomicU64,
}

impl SharedState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(false),
            sample_rate: AtomicU32::new(16000),
            device_name: Mutex::new(String::new()),
            buffer: Mutex::new(Vec::new()),
            stream_tx: Mutex::new(None),
            last_emit_ms: AtomicU64::new(0),
        })
    }
}

// ─────────────────────────────────────────────────────────────
//  AudioService
// ─────────────────────────────────────────────────────────────

/// 持有 cpal::Stream 的 wrapper，实现 Send + Sync
///
/// cpal::Stream 本身不是 Send/Sync (在 WASAPI 后端里包含
/// COM 指针)，所以我们用 unsafe impl。安全前提是：
/// - stream 仅在 AudioService::drop 时被 drop
/// - 我们**不**跨线程移动 stream，只在 init 所在线程创建 + drop
#[allow(dead_code)]
struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct AudioService {
    /// 当前流 — 创建后一直 .play()，不 pause
    stream: Option<SendStream>,
    /// 共享状态
    state: Arc<SharedState>,
}

// AudioService 所有方法通过 Mutex<AudioService> 调用，
// 外部已由 tauri::State 管理线程安全。
impl AudioService {
    pub fn new() -> Self {
        Self {
            stream: None,
            state: SharedState::new(),
        }
    }

    // ─────── 设备枚举 ───────

    pub fn get_input_devices() -> Vec<AudioDevice> {
        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|d| d.name().ok())
            .unwrap_or_default();

        let mut devices = Vec::new();
        let mut name_counts = std::collections::HashMap::new();

        if let Ok(iter) = host.input_devices() {
            for dev in iter {
                if let Ok(name) = dev.name() {
                    // 跳过不支持任何输入配置的设备
                    if dev.default_input_config().is_err() {
                        println!("[AUDIO] skip device (no config): {}", name);
                        continue;
                    }

                    let count = name_counts.entry(name.clone()).or_insert(0);
                    *count += 1;

                    let id = if *count == 1 {
                        name.clone()
                    } else {
                        format!("{} ({})", name, count)
                    };

                    devices.push(AudioDevice {
                        id,
                        name: name.clone(),
                        is_default: name == default_name && *count == 1,
                    });
                }
            }
        }
        println!(
            "[AUDIO] enumerated {} devices, default='{}'",
            devices.len(),
            default_name
        );
        devices
    }

    // ─────── 初始化 / 切换设备 ───────

    pub fn init_with_device<R: tauri::Runtime>(
        &mut self,
        device_id: &str,
        app_handle: tauri::AppHandle<R>,
    ) -> Result<()> {
        // 1. 先丢弃旧流 — 自动 stop + release COM 资源
        self.stream = None;
        self.state.active.store(false, Ordering::SeqCst);

        // 2. 找设备
        let host = cpal::default_host();
        let mut resolved_id = device_id.to_string();

        let device = if device_id.is_empty() {
            let def = host
                .default_input_device()
                .ok_or(anyhow::anyhow!("No default input device found"))?;
            resolved_id = def.name().unwrap_or_default();
            def
        } else {
            let mut name_counts = std::collections::HashMap::new();
            let mut target_device = None;
            if let Ok(iter) = host.input_devices() {
                for dev in iter {
                    if let Ok(name) = dev.name() {
                        if dev.default_input_config().is_err() {
                            continue;
                        }
                        let count = name_counts.entry(name.clone()).or_insert(0);
                        *count += 1;
                        let id = if *count == 1 {
                            name.clone()
                        } else {
                            format!("{} ({})", name, count)
                        };
                        if id == device_id {
                            target_device = Some(dev);
                            break;
                        }
                    }
                }
            }
            target_device.ok_or(anyhow::anyhow!("Device not found: {}", device_id))?
        };

        let actual_name = device.name()?;
        let supported = device.default_input_config()?;
        println!(
            "[AUDIO] init device_id='{}' actual_name='{}' supported={:?}",
            resolved_id, actual_name, supported
        );

        // 3. 准备配置 — 使用设备默认值
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let sample_format = supported.sample_format();

        let stream_config = cpal::StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        // 4. 更新共享状态
        self.state.sample_rate.store(sample_rate, Ordering::Relaxed);
        *self.state.device_name.lock().unwrap() = resolved_id;

        // 5. 构建流 — 通过通用闭包处理所有样本格式
        let state = self.state.clone();
        let cpal_stream = build_input_stream(
            &device,
            &stream_config,
            sample_format,
            channels,
            state,
            app_handle,
        )?;

        // 6. 立即 play — **永不 pause**
        //    这样可以完全回避 Windows WASAPI pause/play 恢复失败的问题。
        //    数据采集由 `state.active` 原子变量控制。
        cpal_stream.play()?;
        println!(
            "[AUDIO] stream playing | sr={} ch={} fmt={:?}",
            sample_rate, channels, sample_format
        );

        self.stream = Some(SendStream(cpal_stream));
        Ok(())
    }

    // ─────── 查询 ───────

    pub fn get_sample_rate(&self) -> u32 {
        self.state.sample_rate.load(Ordering::Relaxed)
    }

    pub fn get_current_device_name(&self) -> String {
        self.state.device_name.lock().unwrap().clone()
    }

    // ─────── 录音 ───────

    /// 开始录音 + 流式传输 — 返回 Receiver 接收实时音频块
    pub fn start_recording_with_streaming(&self) -> Result<std::sync::mpsc::Receiver<Vec<f32>>> {
        self.ensure_stream()?;
        self.state.buffer.lock().unwrap().clear();

        let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
        *self.state.stream_tx.lock().unwrap() = Some(tx);

        self.state.active.store(true, Ordering::SeqCst);
        println!("[AUDIO] start_recording_with_streaming");
        Ok(rx)
    }

    pub fn stop_recording(&self) -> Result<Vec<f32>> {
        thread::sleep(std::time::Duration::from_millis(STOP_DRAIN_MS));
        self.state.active.store(false, Ordering::SeqCst);
        // 清除流式发送器
        *self.state.stream_tx.lock().unwrap() = None;
        let mut buf = self.state.buffer.lock().unwrap();
        let data = std::mem::take(&mut *buf);
        println!("[AUDIO] stop_recording, {} samples collected", data.len());
        Ok(data)
    }

    // ─────── 测试 ───────

    pub fn start_test(&self) -> Result<()> {
        self.ensure_stream()?;
        self.state.active.store(true, Ordering::SeqCst);
        println!("[AUDIO] start_test");
        Ok(())
    }

    pub fn stop_test(&self) -> Result<()> {
        self.state.active.store(false, Ordering::SeqCst);
        self.state.buffer.lock().unwrap().clear();
        println!("[AUDIO] stop_test");
        Ok(())
    }

    // ─────── 内部 ───────

    fn ensure_stream(&self) -> Result<()> {
        if self.stream.is_none() {
            return Err(anyhow::anyhow!(
                "Audio stream not initialized. Please select a device in Settings."
            ));
        }
        Ok(())
    }
}

impl Drop for AudioService {
    fn drop(&mut self) {
        self.state.active.store(false, Ordering::SeqCst);
        // Drop stream — cpal 会自动停止 WASAPI 线程
        self.stream = None;
        println!("[AUDIO] AudioService dropped");
    }
}

// ─────────────────────────────────────────────────────────────
//  统一的输入流构建
// ─────────────────────────────────────────────────────────────

fn build_input_stream<R: tauri::Runtime>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    state: Arc<SharedState>,
    app_handle: tauri::AppHandle<R>,
) -> Result<cpal::Stream> {
    let err_fn = {
        let app_handle = app_handle.clone();
        move |err: cpal::StreamError| {
            eprintln!("[AUDIO] cpal stream error: {}", err);
            match err {
                cpal::StreamError::DeviceNotAvailable => {
                    eprintln!("[AUDIO] Device disconnected, initiating auto-recovery in background...");
                    let app_handle = app_handle.clone();
                    
                    // Trigger a background worker to try auto-reconnecting
                    tauri::async_runtime::spawn(async move {
                        use tauri::Manager;
                        use tauri::Emitter;
                        
                        // Notify frontend it dropped
                        let _ = app_handle.emit("audio_device_disconnected", "");

                        // We will try up to 3 times to get the device back with exponential backoff
                        let mut backoff_secs = 2;
                        
                        for attempt in 1..=3 {
                            eprintln!("[AUDIO] Auto-reconnect attempt {} in {} seconds...", attempt, backoff_secs);
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            
                            // 1. Fetch current target device from storage config
                            let device_id = if let Some(storage) = app_handle.try_state::<crate::state::StorageState>() {
                                storage.load_config().input_device
                            } else {
                                String::new()
                            };
                            
                            // 2. Perform the reset mapping
                            let mut success = false;
                            if let Some(audio_state) = app_handle.try_state::<crate::state::AudioState>() {
                                if let Ok(mut audio) = audio_state.lock() {
                                    if let Err(e) = audio.init_with_device(&device_id, app_handle.clone()) {
                                        eprintln!("[AUDIO] Auto-recovery attempt {} failed: {}", attempt, e);
                                    } else {
                                        success = true;
                                    }
                                }
                            }
                            
                            if success {
                                eprintln!("[AUDIO] Auto-recovery successful!");
                                // Notify frontend that UI can sync state if necessary
                                let _ = app_handle.emit("audio_reconnected", "");
                                break;
                            } else {
                                backoff_secs *= 2; // e.g. 2s -> 4s -> 8s
                            }
                        }
                    });
                }
                _ => {}
            }
        }
    };

    // 使用统一的字节级回调，避免泛型地狱和 trait bound 问题
    let stream = match sample_format {
        SampleFormat::F32 => {
            let state = state.clone();
            let app_handle = app_handle.clone();
            device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !state.active.load(Ordering::Relaxed) {
                        return;
                    }
                    // F32 已经是归一化的 [-1.0, 1.0]
                    let mono = to_mono(data, channels);
                    process_audio_data(&state, &app_handle, mono);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::I16 => {
            let state = state.clone();
            let app_handle = app_handle.clone();
            device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !state.active.load(Ordering::Relaxed) {
                        return;
                    }
                    // 归一化 i16 -> f32 [-1.0, 1.0]
                    let float_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono = to_mono(&float_data, channels);
                    process_audio_data(&state, &app_handle, mono);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::U16 => {
            let state = state.clone();
            let app_handle = app_handle.clone();
            device.build_input_stream(
                config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if !state.active.load(Ordering::Relaxed) {
                        return;
                    }
                    let float_data: Vec<f32> =
                        data.iter().map(|&s| (s as f32 / 32768.0) - 1.0).collect();
                    let mono = to_mono(&float_data, channels);
                    process_audio_data(&state, &app_handle, mono);
                },
                err_fn,
                None,
            )?
        }
        fmt => {
            return Err(anyhow::anyhow!("Unsupported sample format: {:?}", fmt));
        }
    };

    Ok(stream)
}

/// 多声道 -> 单声道 (已经是 f32)
#[inline]
fn to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|ch| ch.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// 处理已归一化的单声道 f32 数据：流式发送 + 缓冲 + 电平事件
#[inline]
fn process_audio_data<R: tauri::Runtime>(
    state: &SharedState,
    app_handle: &tauri::AppHandle<R>,
    mono: Vec<f32>,
) {
    if mono.is_empty() {
        return;
    }

    // ── 1. 流式发送给 ASR ──
    if let Ok(guard) = state.stream_tx.try_lock() {
        if let Some(ref tx) = *guard {
            let _ = tx.send(mono.clone());
        }
    }

    // ── 2. 缓冲 ──
    if let Ok(mut buf) = state.buffer.try_lock() {
        buf.extend_from_slice(&mono);
    }

    // ── 3. 电平事件 (节流) ──
    let rms = {
        let sum_sq: f32 = mono.iter().map(|s| s * s).sum();
        (sum_sq / mono.len() as f32).sqrt()
    };

    let now = current_time_ms();
    let last = state.last_emit_ms.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= LEVEL_THROTTLE_MS {
        state.last_emit_ms.store(now, Ordering::Relaxed);
        use tauri::Emitter;
        let _ = app_handle.emit("audio_level", rms);
    }
}

// ─────────────────────────────────────────────────────────────
//  工具
// ─────────────────────────────────────────────────────────────

#[inline]
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

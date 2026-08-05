//! Meeting audio capture.
//!
//! Owns its own cpal input stream (independent from the dictation
//! [`crate::audio::AudioService`]) so dictation hotkey input keeps working
//! while a meeting is being recorded.
//!
//! ## Design
//! - Opens the configured input device (or default mic) at the device's
//!   native sample rate (mono after downmix).
//! - When `include_system_audio` is enabled, also opens a WASAPI loopback
//!   stream on the default output device. A mixer thread resamples the
//!   loopback stream to the mic's sample rate and adds both sources together,
//!   so the downstream ASR pipeline always sees a properly mixed mono stream.
//! - Pushes f32 chunks into a `crossbeam`-style mpsc channel that the ASR
//!   provider consumes.
//! - Emits `meeting_audio_level` events at most every 16 ms.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::{fs::File, io::BufWriter, path::{Path, PathBuf}};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

const LEVEL_THROTTLE_MS: u64 = 16;
type DraftWriter = hound::WavWriter<BufWriter<File>>;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct MeetingAudioConfig {
    /// Capture system audio (WASAPI loopback) alongside the microphone.
    /// When enabled, both mic and loopback streams are mixed into a single
    /// mono channel before being sent to the ASR provider.
    #[serde(default)]
    pub include_system_audio: bool,
}

/// Wrapper to make cpal::Stream Send (mirrors the dictation audio module).
#[allow(dead_code)]
struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct MeetingAudioCapture {
    _stream: SendStream,
    _loopback: Option<super::loopback::LoopbackCapture>,
    sample_rate: u32,
    started: Arc<AtomicBool>,
    pub audio_rx: Option<Receiver<Vec<f32>>>,
    draft_audio_path: Option<PathBuf>,
    draft_writer: Option<Arc<Mutex<Option<DraftWriter>>>>,
}

impl MeetingAudioCapture {
    /// Open the configured input device and begin streaming f32 mono chunks
    /// to the returned `audio_rx`. When `opts.include_system_audio` is true,
    /// also opens a WASAPI loopback stream and mixes both sources.
    pub fn start<R: Runtime>(
        app: AppHandle<R>,
        device_id: &str,
        draft_audio_path: Option<PathBuf>,
        opts: MeetingAudioConfig,
    ) -> Result<Self> {
        let host = cpal::default_host();
        let device = if device_id.is_empty() {
            host.default_input_device()
                .ok_or_else(|| anyhow!("no default input device"))?
        } else {
            let mut found = None;
            let mut name_counts = std::collections::HashMap::new();
            if let Ok(iter) = host.input_devices() {
                for dev in iter {
                    let Ok(name) = dev.name() else { continue };
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
                        found = Some(dev);
                        break;
                    }
                }
            }
            found.ok_or_else(|| anyhow!("input device not found: {}", device_id))?
        };

        let supported = device.default_input_config()?;
        let sample_format = supported.sample_format();
        let channels = supported.channels() as usize;
        let stream_config = cpal::StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        let sample_rate = supported.sample_rate().0;

        // Downstream channel handed to the ASR pipeline. Mic audio (and, when
        // enabled, the loopback mix) flows through a forwarding/mixer thread
        // so the draft WAV always matches what the ASR actually hears.
        let (tx, rx) = channel::<Vec<f32>>();
        let (mic_tx, mic_rx) = channel::<Vec<f32>>();
        let started = Arc::new(AtomicBool::new(true));
        let last_emit = Arc::new(AtomicU64::new(0));
        let draft_writer = draft_audio_path
            .as_ref()
            .map(|path| create_draft_writer(path, sample_rate))
            .transpose()?
            .map(|writer| Arc::new(Mutex::new(Some(writer))));

        // If system audio is requested, start the WASAPI loopback capture on
        // its own channel; the mixer thread below combines it with the mic.
        let mut loopback: Option<super::loopback::LoopbackCapture> = None;
        let mut loopback_rx: Option<Receiver<Vec<f32>>> = None;
        if opts.include_system_audio {
            let (lb_tx, lb_rx) = channel::<Vec<f32>>();
            match super::loopback::LoopbackCapture::start(lb_tx) {
                Ok(lb) => {
                    println!(
                        "[MEETING] loopback capture started: sr={}",
                        lb.sample_rate()
                    );
                    loopback_rx = Some(lb_rx);
                    loopback = Some(lb);
                }
                Err(e) => {
                    eprintln!("[MEETING] loopback capture failed to start: {e}");
                    eprintln!("[MEETING] continuing with mic-only capture");
                }
            }
        }

        let cpal_stream = build_input_stream(
            &device,
            &stream_config,
            sample_format,
            channels,
            mic_tx,
            started.clone(),
            last_emit,
            app,
        )?;
        cpal_stream.play()?;

        match loopback_rx {
            Some(lb_rx) => spawn_mixer(
                tx,
                mic_rx,
                lb_rx,
                sample_rate,
                loopback.as_ref().expect("loopback present").sample_rate(),
                draft_writer.clone(),
            ),
            None => spawn_passthrough(tx, mic_rx, draft_writer.clone()),
        }

        println!(
            "[MEETING] capture started sr={} ch={} fmt={:?} loopback={}",
            sample_rate, channels, sample_format, loopback.is_some()
        );

        Ok(Self {
            _stream: SendStream(cpal_stream),
            _loopback: loopback,
            sample_rate,
            started,
            audio_rx: Some(rx),
            draft_audio_path,
            draft_writer,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn take_audio_rx(&mut self) -> Option<Receiver<Vec<f32>>> {
        self.audio_rx.take()
    }

    pub fn draft_audio_path(&self) -> Option<&Path> {
        self.draft_audio_path.as_deref()
    }

    pub fn stop(self) {
        let MeetingAudioCapture {
            _stream,
            _loopback,
            started,
            draft_writer,
            ..
        } = self;
        started.store(false, Ordering::SeqCst);
        if let Some(lb) = _loopback {
            lb.stop();
        }
        drop(_stream);
        if let Some(writer) = draft_writer {
            if let Ok(mut guard) = writer.lock() {
                if let Some(writer) = guard.take() {
                    let _ = writer.finalize();
                }
            }
        }
        println!("[MEETING] capture stopped");
    }
}

#[allow(clippy::too_many_arguments)]
fn build_input_stream<R: Runtime>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    channels: usize,
    tx: Sender<Vec<f32>>,
    active: Arc<AtomicBool>,
    last_emit: Arc<AtomicU64>,
    app: AppHandle<R>,
) -> Result<cpal::Stream> {
    let err_fn = |err: cpal::StreamError| {
        eprintln!("[MEETING] cpal stream error: {err}");
    };

    let stream = match sample_format {
        SampleFormat::F32 => {
            let tx = Mutex::new(tx);
            let active = active.clone();
            let last_emit = last_emit.clone();
            let app = app.clone();
            device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !active.load(Ordering::Relaxed) {
                        return;
                    }
                    let mono = to_mono(data, channels);
                    push_chunk(&tx, mono, &last_emit, &app);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::I16 => {
            let tx = Mutex::new(tx);
            let active = active.clone();
            let last_emit = last_emit.clone();
            let app = app.clone();
            device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !active.load(Ordering::Relaxed) {
                        return;
                    }
                    let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                    let mono = to_mono(&f, channels);
                    push_chunk(&tx, mono, &last_emit, &app);
                },
                err_fn,
                None,
            )?
        }
        SampleFormat::U16 => {
            let tx = Mutex::new(tx);
            let active = active.clone();
            let last_emit = last_emit.clone();
            let app = app.clone();
            device.build_input_stream(
                config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if !active.load(Ordering::Relaxed) {
                        return;
                    }
                    let f: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 / 32768.0) - 1.0)
                        .collect();
                    let mono = to_mono(&f, channels);
                    push_chunk(&tx, mono, &last_emit, &app);
                },
                err_fn,
                None,
            )?
        }
        fmt => return Err(anyhow!("unsupported sample format: {fmt:?}")),
    };
    Ok(stream)
}

fn push_chunk<R: Runtime>(
    tx: &Mutex<Sender<Vec<f32>>>,
    mono: Vec<f32>,
    last_emit: &AtomicU64,
    app: &AppHandle<R>,
) {
    if mono.is_empty() {
        return;
    }
    // Level event (throttled).
    let rms: f32 = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if now.saturating_sub(last_emit.load(Ordering::Relaxed)) >= LEVEL_THROTTLE_MS {
        last_emit.store(now, Ordering::Relaxed);
        let _ = app.emit("meeting_audio_level", rms);
    }
    if let Ok(guard) = tx.try_lock() {
        // Receiver gone → meeting stopped before we noticed; ignore.
        let _ = guard.send(mono);
    }
}

/// Mic-only path: forward mic chunks downstream, writing the draft WAV from
/// the same samples the ASR consumes.
fn spawn_passthrough(
    downstream: Sender<Vec<f32>>,
    mic_rx: Receiver<Vec<f32>>,
    draft_writer: Option<Arc<Mutex<Option<DraftWriter>>>>,
) {
    thread::Builder::new()
        .name("meeting-forward".into())
        .spawn(move || {
            while let Ok(chunk) = mic_rx.recv() {
                if let Some(writer) = &draft_writer {
                    write_draft_samples(writer, &chunk);
                }
                if downstream.send(chunk).is_err() {
                    break;
                }
            }
        })
        .ok();
}

/// Loopback path: resample the loopback stream to the mic sample rate and
/// sum both sources, so the ASR pipeline (and the draft WAV) sees a single
/// properly mixed mono stream. Exits when the mic channel closes (the cpal
/// stream is dropped on stop), which drops the downstream sender and ends
/// the pipeline with EOF.
fn spawn_mixer(
    downstream: Sender<Vec<f32>>,
    mic_rx: Receiver<Vec<f32>>,
    lb_rx: Receiver<Vec<f32>>,
    mic_sr: u32,
    lb_sr: u32,
    draft_writer: Option<Arc<Mutex<Option<DraftWriter>>>>,
) {
    thread::Builder::new()
        .name("meeting-mixer".into())
        .spawn(move || {
            let needs_resample = (mic_sr as f32 - lb_sr as f32).abs() >= 1.0;
            let mut resampler = if needs_resample {
                FastFixedIn::<f32>::new(
                    mic_sr as f64 / lb_sr as f64,
                    1.0,
                    PolynomialDegree::Septic,
                    1024,
                    1,
                )
                .ok()
            } else {
                None
            };
            // Resampled loopback samples ready to be mixed into mic chunks.
            let mut lb_pending: Vec<f32> = Vec::new();

            while let Ok(chunk) = mic_rx.recv() {
                while let Ok(lb_chunk) = lb_rx.try_recv() {
                    let resampled = match resampler.as_mut() {
                        Some(r) => resample_loopback(r, &lb_chunk),
                        None => lb_chunk,
                    };
                    lb_pending.extend_from_slice(&resampled);
                }

                // Pad with silence if the loopback hasn't produced enough
                // samples yet (slight start-up skew between the two devices).
                if lb_pending.len() < chunk.len() {
                    lb_pending.resize(chunk.len(), 0.0);
                }

                let mixed: Vec<f32> = chunk
                    .iter()
                    .zip(lb_pending.drain(..chunk.len()))
                    .map(|(mic, lb)| (mic + lb).clamp(-1.0, 1.0))
                    .collect();

                if let Some(writer) = &draft_writer {
                    write_draft_samples(writer, &mixed);
                }
                if downstream.send(mixed).is_err() {
                    break;
                }
            }
        })
        .ok();
}

/// Resample a loopback chunk into the mic sample-rate domain (1024-sample
/// blocks, tail padded).
fn resample_loopback(resampler: &mut FastFixedIn<f32>, samples: &[f32]) -> Vec<f32> {
    const CHUNK: usize = 1024;
    let mut out = Vec::new();
    let mut input_buf: [Vec<f32>; 1] = [Vec::with_capacity(CHUNK)];
    let mut idx = 0;
    while idx + CHUNK <= samples.len() {
        input_buf[0].clear();
        input_buf[0].extend_from_slice(&samples[idx..idx + CHUNK]);
        if let Ok(processed) = resampler.process(&input_buf, None) {
            out.extend_from_slice(&processed[0]);
        }
        idx += CHUNK;
    }
    if idx < samples.len() {
        input_buf[0].clear();
        input_buf[0].extend_from_slice(&samples[idx..]);
        input_buf[0].resize(CHUNK, 0.0);
        if let Ok(processed) = resampler.process(&input_buf, None) {
            out.extend_from_slice(&processed[0]);
        }
    }
    out
}

#[inline]
fn to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|ch| ch.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn create_draft_writer(path: &Path, sample_rate: u32) -> Result<DraftWriter> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    Ok(hound::WavWriter::create(path, spec)?)
}

fn write_draft_samples(writer: &Arc<Mutex<Option<DraftWriter>>>, mono: &[f32]) {
    let Ok(mut guard) = writer.lock() else {
        return;
    };
    let Some(writer) = guard.as_mut() else {
        return;
    };
    for sample in mono {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        let _ = writer.write_sample(value);
    }
}

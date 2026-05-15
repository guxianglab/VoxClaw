//! WASAPI loopback capture for recording system audio on Windows.
//!
//! Opens the default audio render (output) device in loopback mode so that
//! any audio playing through the speakers/headphones is captured. This is
//! used by meeting mode to record the other participants' audio.
//!
//! Requires the `Win32_Media_Audio` and `Win32_System_Com` features of the
//! `windows` crate.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use windows::Win32::Media::Audio::{
    eConsole, eRender, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::core::GUID;

// CLSID_MMDeviceEnumerator {BCDE0395-E52F-467C-8E3D-C4579291692E}
const CLSID_MM_DEVICE_ENUMERATOR: GUID = GUID::from_values(
    0xBCDE0395, 0xE52F, 0x467C, [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
);

/// A handle to a running WASAPI loopback capture thread.
pub struct LoopbackCapture {
    active: Arc<AtomicBool>,
    sample_rate: u32,
    channels: u32,
}

impl LoopbackCapture {
    /// Start capturing system audio in loopback mode.
    ///
    /// Captured f32 mono samples are sent to `tx`. The caller should drop `tx`
    /// to signal stop, or call [`LoopbackCapture::stop`].
    pub fn start(tx: Sender<Vec<f32>>) -> Result<Self> {
        let active = Arc::new(AtomicBool::new(true));
        let active_clone = active.clone();

        // We'll get sample_rate and channels from the WASAPI format inside the thread.
        let sr = Arc::new(Mutex::new(0u32));
        let ch = Arc::new(Mutex::new(0u32));
        let sr_clone = sr.clone();
        let ch_clone = ch.clone();

        thread::Builder::new()
            .name("meeting-loopback".into())
            .spawn(move || {
                if let Err(e) = loopback_capture_thread(tx, &active_clone, &sr_clone, &ch_clone) {
                    eprintln!("[LOOPBACK] Capture thread error: {e}");
                }
            })
            .map_err(|e| anyhow!("failed to spawn loopback thread: {e}"))?;

        // Give the thread a moment to initialize and report the format.
        thread::sleep(std::time::Duration::from_millis(200));
        let sample_rate = *sr.lock().unwrap_or_else(|e| e.into_inner());
        let channels = *ch.lock().unwrap_or_else(|e| e.into_inner());

        if sample_rate == 0 {
            return Err(anyhow!(
                "WASAPI loopback: failed to get audio format from render device"
            ));
        }

        println!(
            "[LOOPBACK] Started: sr={} ch={}",
            sample_rate, channels
        );

        Ok(Self {
            active,
            sample_rate,
            channels,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        println!("[LOOPBACK] Stopped");
    }
}

fn loopback_capture_thread(
    tx: Sender<Vec<f32>>,
    active: &AtomicBool,
    out_sr: &Arc<Mutex<u32>>,
    out_ch: &Arc<Mutex<u32>>,
) -> Result<()> {
    unsafe {
        // Initialize COM for this thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        // Get the default audio render device (speakers/headphones).
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&CLSID_MM_DEVICE_ENUMERATOR, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;

        // Activate IAudioClient on the render device.
        let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        // Get the mix format.
        let format_ptr = audio_client.GetMixFormat()?;
        let format = &*format_ptr;

        let sample_rate = format.nSamplesPerSec;
        let channels = format.nChannels as u32;

        // Report format back to caller.
        {
            let mut sr = out_sr.lock().unwrap_or_else(|e| e.into_inner());
            *sr = sample_rate;
        }
        {
            let mut ch = out_ch.lock().unwrap_or_else(|e| e.into_inner());
            *ch = channels;
        }

        let frame_size = (channels as usize) * (format.wBitsPerSample as usize / 8);

        // Initialize the audio client in loopback mode.
        // We use 10ms buffer for low latency.
        let buffer_duration = 10_000_000i64; // 10ms in 100ns units
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            buffer_duration,
            0,
            format_ptr,
            None,
        )?;

        let capture_client: IAudioCaptureClient = audio_client.GetService()?;
        audio_client.Start()?;

        // Capture loop.
        while active.load(Ordering::Relaxed) {
            // Sleep briefly to avoid busy-looping.
            thread::sleep(std::time::Duration::from_millis(5));

            loop {
                // GetNextPacketSize returns Result<u32> in windows 0.58
                let packet_length = match capture_client.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("[LOOPBACK] GetNextPacketSize error: {e}");
                        break;
                    }
                };

                if packet_length == 0 {
                    break;
                }

                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut num_frames = 0u32;
                let mut flags = 0u32;

                match capture_client.GetBuffer(
                    &mut data_ptr,
                    &mut num_frames,
                    &mut flags,
                    None,
                    None,
                ) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("[LOOPBACK] GetBuffer error: {e}");
                        break;
                    }
                }

                if data_ptr.is_null() || num_frames == 0 {
                    let _ = capture_client.ReleaseBuffer(num_frames);
                    continue;
                }

                // Convert to f32 mono.
                let samples = unsafe {
                    let byte_len = (num_frames as usize) * frame_size;
                    let bytes = std::slice::from_raw_parts(data_ptr, byte_len);

                    match format.wBitsPerSample {
                        16 => pcm16_to_mono_f32(bytes, channels as usize),
                        32 => pcm32f_to_mono_f32(bytes, channels as usize),
                        _ => {
                            // Unsupported format — skip this packet.
                            Vec::new()
                        }
                    }
                };

                let _ = capture_client.ReleaseBuffer(num_frames);

                if !samples.is_empty() && tx.send(samples).is_err() {
                    // Receiver dropped — meeting stopped.
                    break;
                }
            }
        }

        let _ = audio_client.Stop();
        Ok(())
    }
}

/// Convert 16-bit PCM interleaved bytes to f32 mono.
fn pcm16_to_mono_f32(data: &[u8], channels: usize) -> Vec<f32> {
    let num_samples = data.len() / 2;
    let num_frames = num_samples / channels;
    let mut out = Vec::with_capacity(num_frames);

    for frame_idx in 0..num_frames {
        let mut sum = 0.0f32;
        for ch in 0..channels {
            let offset = (frame_idx * channels + ch) * 2;
            if offset + 1 < data.len() {
                let sample =
                    i16::from_le_bytes([data[offset], data[offset + 1]]) as f32 / 32768.0;
                sum += sample;
            }
        }
        out.push(sum / channels as f32);
    }
    out
}

/// Convert 32-bit float PCM interleaved bytes to f32 mono.
fn pcm32f_to_mono_f32(data: &[u8], channels: usize) -> Vec<f32> {
    let num_samples = data.len() / 4;
    let num_frames = num_samples / channels;
    let mut out = Vec::with_capacity(num_frames);

    for frame_idx in 0..num_frames {
        let mut sum = 0.0f32;
        for ch in 0..channels {
            let offset = (frame_idx * channels + ch) * 4;
            if offset + 3 < data.len() {
                let sample = f32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                sum += sample;
            }
        }
        out.push(sum / channels as f32);
    }
    out
}

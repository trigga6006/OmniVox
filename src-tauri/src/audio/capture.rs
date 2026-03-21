use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::types::{AudioConfig, AudioDevice};
use crate::error::{AppError, AppResult};

const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Real-time audio capture engine backed by cpal.
///
/// Captures microphone input, converts to 16 kHz mono f32 (what Whisper expects),
/// and exposes an RMS audio level for the frontend VU meter.
pub struct AudioCapture {
    config: AudioConfig,
    /// Accumulated 16 kHz mono f32 samples — written by the cpal thread, read on stop.
    buffer: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
    /// Current RMS audio level stored as f32 bits for lock-free reads.
    rms_level: Arc<AtomicU32>,
    /// Active cpal stream handle. Dropping it stops capture.
    stream: Option<cpal::Stream>,
}

// SAFETY: AudioCapture is always accessed behind a Mutex in AppState, ensuring
// exclusive access. cpal::Stream is !Send as a conservative blanket across all
// platforms, but on Windows (WASAPI) the underlying handles are thread-safe.
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

impl AudioCapture {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            // Pre-allocate for ~60 s of audio to avoid early reallocations
            buffer: Arc::new(Mutex::new(Vec::with_capacity(
                TARGET_SAMPLE_RATE as usize * 60,
            ))),
            is_recording: Arc::new(AtomicBool::new(false)),
            rms_level: Arc::new(AtomicU32::new(0)),
            stream: None,
        }
    }

    /// List all available audio input devices.
    pub fn enumerate_devices() -> AppResult<Vec<AudioDevice>> {
        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|d| d.name().ok());

        let input_devices = host
            .input_devices()
            .map_err(|e| AppError::Audio(format!("Failed to enumerate devices: {e}")))?;

        let mut devices = Vec::new();
        for device in input_devices {
            let name = device.name().unwrap_or_else(|_| "Unknown".into());
            let is_default = default_name.as_ref() == Some(&name);

            if let Ok(config) = device.default_input_config() {
                devices.push(AudioDevice {
                    id: name.clone(),
                    name,
                    is_default,
                    sample_rate: config.sample_rate().0,
                    channels: config.channels(),
                });
            }
        }

        Ok(devices)
    }

    /// Open the configured input device and begin capturing audio.
    ///
    /// Audio is continuously resampled to 16 kHz mono and accumulated in an
    /// internal buffer until [`stop`] or [`cancel`] is called.
    pub fn start(&mut self) -> AppResult<()> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Err(AppError::Audio("Already recording".into()));
        }

        self.buffer.lock().unwrap().clear();

        let host = cpal::default_host();
        let device = self.resolve_device(&host)?;

        let supported = device
            .default_input_config()
            .map_err(|e| AppError::Audio(format!("No input config: {e}")))?;

        let device_rate = supported.sample_rate().0;
        let device_channels = supported.channels();
        let stream_config: cpal::StreamConfig = supported.into();

        let buffer = Arc::clone(&self.buffer);
        let is_recording = Arc::clone(&self.is_recording);
        let rms_level = Arc::clone(&self.rms_level);

        // Linear-interpolation resampler state.
        // resample_ratio < 1 means we are downsampling (e.g. 48 kHz → 16 kHz).
        let resample_ratio = TARGET_SAMPLE_RATE as f64 / device_rate as f64;
        let mut resample_pos: f64 = 0.0;

        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !is_recording.load(Ordering::Relaxed) {
                        return;
                    }

                    // --- Stereo → Mono ---
                    let mono: Vec<f32> = if device_channels == 1 {
                        data.to_vec()
                    } else {
                        // cpal always delivers complete frames, so chunks_exact is safe
                        data.chunks_exact(device_channels as usize)
                            .map(|frame| {
                                frame.iter().sum::<f32>() / device_channels as f32
                            })
                            .collect()
                    };

                    if mono.is_empty() {
                        return;
                    }

                    // --- RMS for VU meter ---
                    let rms =
                        (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
                    // Scale up aggressively for UI sensitivity — desktop mic
                    // speech RMS is typically 0.005–0.05 depending on gain.
                    let level = (rms * 35.0).min(1.0);
                    rms_level.store(level.to_bits(), Ordering::Relaxed);

                    // --- Resample to 16 kHz ---
                    let resampled = if device_rate == TARGET_SAMPLE_RATE {
                        mono
                    } else {
                        let capacity =
                            (mono.len() as f64 * resample_ratio) as usize + 1;
                        let mut out = Vec::with_capacity(capacity);

                        while resample_pos < mono.len() as f64 {
                            let idx = resample_pos as usize;
                            let frac = (resample_pos - idx as f64) as f32;
                            let sample = if idx + 1 < mono.len() {
                                mono[idx] * (1.0 - frac) + mono[idx + 1] * frac
                            } else {
                                mono[idx]
                            };
                            out.push(sample);
                            resample_pos += 1.0 / resample_ratio;
                        }
                        // Carry fractional position into the next callback
                        resample_pos -= mono.len() as f64;
                        out
                    };

                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend_from_slice(&resampled);
                    }
                },
                |err| eprintln!("Audio stream error: {err}"),
                None,
            )
            .map_err(|e| AppError::Audio(format!("Failed to build stream: {e}")))?;

        stream
            .play()
            .map_err(|e| AppError::Audio(format!("Failed to start stream: {e}")))?;

        self.is_recording.store(true, Ordering::SeqCst);
        self.stream = Some(stream);

        Ok(())
    }

    /// Stop recording and return all accumulated 16 kHz mono f32 samples.
    pub fn stop(&mut self) -> AppResult<Vec<f32>> {
        self.is_recording.store(false, Ordering::SeqCst);
        // Dropping the stream handle closes the device
        self.stream.take();
        self.rms_level.store(0, Ordering::Relaxed);

        let samples = std::mem::take(&mut *self.buffer.lock().unwrap());
        Ok(samples)
    }

    /// Cancel recording and discard all captured audio.
    pub fn cancel(&mut self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream.take();
        self.buffer.lock().unwrap().clear();
        self.rms_level.store(0, Ordering::Relaxed);
    }

    /// Current RMS audio level in the 0.0–1.0 range (for the frontend VU meter).
    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.rms_level.load(Ordering::Relaxed))
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    /// Arc handle to the is_recording flag — used by the audio level emitter.
    pub fn is_recording_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_recording)
    }

    /// Arc handle to the RMS level — used by the audio level emitter.
    pub fn rms_level_ref(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.rms_level)
    }

    /// Duration of the currently buffered audio in seconds.
    pub fn duration_secs(&self) -> f32 {
        let len = self.buffer.lock().unwrap().len();
        len as f32 / TARGET_SAMPLE_RATE as f32
    }

    /// Resolve the device to use based on `AudioConfig.device_id`.
    fn resolve_device(&self, host: &cpal::Host) -> AppResult<cpal::Device> {
        if let Some(ref id) = self.config.device_id {
            host.input_devices()
                .map_err(|e| AppError::Audio(format!("Failed to enumerate: {e}")))?
                .find(|d| d.name().ok().as_ref() == Some(id))
                .ok_or_else(|| AppError::Audio(format!("Device '{id}' not found")))
        } else {
            host.default_input_device()
                .ok_or_else(|| AppError::Audio("No default input device".into()))
        }
    }
}

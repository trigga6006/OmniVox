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
    ///
    /// Runs on a dedicated thread with a 3-second timeout to avoid hanging
    /// when USB devices disconnect mid-enumeration.
    pub fn enumerate_devices() -> AppResult<Vec<AudioDevice>> {
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let result = Self::enumerate_devices_inner();
            let _ = tx.send(result);
        });

        rx.recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| AppError::Audio("Device enumeration timed out — try unplugging and re-plugging your audio device".into()))?
    }

    fn enumerate_devices_inner() -> AppResult<Vec<AudioDevice>> {
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

                    let ch = device_channels as usize;
                    let n_frames = if ch == 1 { data.len() } else { data.len() / ch };
                    if n_frames == 0 {
                        return;
                    }

                    // Inline mono sample accessor — avoids allocating a Vec.
                    // For stereo+, averages channels on the fly. Each sample is
                    // accessed at most twice (interpolation), so the redundant
                    // arithmetic is negligible vs. a heap allocation per callback.
                    let mono = |i: usize| -> f32 {
                        if ch == 1 {
                            data[i]
                        } else {
                            let off = i * ch;
                            let mut sum = 0.0f32;
                            for c in 0..ch {
                                sum += data[off + c];
                            }
                            sum / ch as f32
                        }
                    };

                    // --- RMS for VU meter (no allocation) ---
                    let mut sum_sq = 0.0f32;
                    for i in 0..n_frames {
                        let s = mono(i);
                        sum_sq += s * s;
                    }
                    let rms = (sum_sq / n_frames as f32).sqrt();
                    // Scale up aggressively for UI sensitivity — desktop mic
                    // speech RMS is typically 0.005–0.05 depending on gain.
                    let level = (rms * 35.0).min(1.0);
                    rms_level.store(level.to_bits(), Ordering::Relaxed);

                    // --- Resample to 16 kHz directly into the shared buffer ---
                    if let Ok(mut buf) = buffer.lock() {
                        if device_rate == TARGET_SAMPLE_RATE {
                            if ch == 1 {
                                buf.extend_from_slice(data);
                            } else {
                                buf.reserve(n_frames);
                                for i in 0..n_frames {
                                    buf.push(mono(i));
                                }
                            }
                        } else {
                            let estimated =
                                (n_frames as f64 * resample_ratio) as usize + 1;
                            buf.reserve(estimated);

                            while resample_pos < n_frames as f64 {
                                let idx = resample_pos as usize;
                                let frac = (resample_pos - idx as f64) as f32;
                                let sample = if idx + 1 < n_frames {
                                    mono(idx) * (1.0 - frac) + mono(idx + 1) * frac
                                } else {
                                    mono(idx)
                                };
                                buf.push(sample);
                                resample_pos += 1.0 / resample_ratio;
                            }
                            // Carry fractional position into the next callback
                            resample_pos -= n_frames as f64;
                        }
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
    ///
    /// Swaps in a fresh pre-allocated buffer so the next recording doesn't
    /// need to grow from zero capacity.
    pub fn stop(&mut self) -> AppResult<Vec<f32>> {
        self.is_recording.store(false, Ordering::SeqCst);
        // Dropping the stream handle closes the device
        self.stream.take();
        self.rms_level.store(0, Ordering::Relaxed);

        let mut buf = self.buffer.lock().unwrap();
        let samples = std::mem::replace(
            &mut *buf,
            Vec::with_capacity(TARGET_SAMPLE_RATE as usize * 60),
        );
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

    /// Clone the last `max_samples` from the buffer without stopping capture.
    ///
    /// Used by the live preview feature to grab a rolling window of recent
    /// audio for interim transcription while recording continues.  The lock
    /// is held only for the clone (~microseconds).
    pub fn snapshot_tail(&self, max_samples: usize) -> Vec<f32> {
        let buf = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        let start = buf.len().saturating_sub(max_samples);
        buf[start..].to_vec()
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

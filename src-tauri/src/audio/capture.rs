use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::types::{AudioConfig, AudioDevice};
use crate::error::{AppError, AppResult};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const BUFFER_CAPACITY: usize = TARGET_SAMPLE_RATE as usize * 60;

/// Real-time audio capture engine backed by cpal.
///
/// Captures microphone input, converts its native format to 16 kHz mono f32,
/// and exposes an RMS audio level for the frontend VU meter.
pub struct AudioCapture {
    config: AudioConfig,
    source: CaptureSource,
    buffer: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
    rms_level: Arc<AtomicU32>,
    /// Fatal asynchronous backend error from the active stream, if any.
    stream_error: Arc<Mutex<Option<String>>>,
    stream: Option<cpal::Stream>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSource {
    Microphone,
    SystemLoopback,
}

// SAFETY: AudioCapture is always accessed behind a Mutex in AppState, ensuring
// exclusive access. cpal::Stream is !Send as a conservative blanket across all
// platforms, but the underlying handles (WASAPI, CoreAudio, ALSA) are thread-safe.
unsafe impl Send for AudioCapture {}
unsafe impl Sync for AudioCapture {}

struct CaptureProcessor {
    buffer: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<AtomicBool>,
    rms_level: Arc<AtomicU32>,
    device_rate: u32,
    channels: usize,
    downsampler: Option<crate::audio::resample::StreamDownsampler>,
    mono_buf: Vec<f32>,
    resampled: Vec<f32>,
    upsample_ratio: f64,
    upsample_position: f64,
}

impl CaptureProcessor {
    fn new(
        buffer: Arc<Mutex<Vec<f32>>>,
        is_recording: Arc<AtomicBool>,
        rms_level: Arc<AtomicU32>,
        device_rate: u32,
        channels: u16,
    ) -> Self {
        Self {
            buffer,
            is_recording,
            rms_level,
            device_rate,
            channels: channels as usize,
            downsampler: (device_rate > TARGET_SAMPLE_RATE)
                .then(|| crate::audio::resample::StreamDownsampler::new(device_rate)),
            mono_buf: Vec::new(),
            resampled: Vec::new(),
            upsample_ratio: TARGET_SAMPLE_RATE as f64 / device_rate as f64,
            upsample_position: 0.0,
        }
    }

    fn process(&mut self, data: &[f32]) {
        if !self.is_recording.load(Ordering::Relaxed) {
            return;
        }

        let channels = self.channels;
        let n_frames = if channels == 1 {
            data.len()
        } else {
            data.len() / channels
        };
        if n_frames == 0 {
            return;
        }

        let mono = |index: usize| -> f32 {
            if channels == 1 {
                data[index]
            } else {
                let offset = index * channels;
                data[offset..offset + channels].iter().sum::<f32>() / channels as f32
            }
        };

        let sum_sq = (0..n_frames)
            .map(|index| {
                let sample = mono(index);
                sample * sample
            })
            .sum::<f32>();
        let rms = (sum_sq / n_frames as f32).sqrt();
        self.rms_level
            .store((rms * 35.0).min(1.0).to_bits(), Ordering::Relaxed);

        if self.device_rate == TARGET_SAMPLE_RATE {
            if let Ok(mut buffer) = self.buffer.lock() {
                if channels == 1 {
                    buffer.extend_from_slice(data);
                } else {
                    buffer.reserve(n_frames);
                    for index in 0..n_frames {
                        buffer.push(mono(index));
                    }
                }
            }
        } else if let Some(downsampler) = self.downsampler.as_mut() {
            let mono_slice = if channels == 1 {
                data
            } else {
                self.mono_buf.clear();
                self.mono_buf.reserve(n_frames);
                for index in 0..n_frames {
                    self.mono_buf.push(mono(index));
                }
                &self.mono_buf
            };
            self.resampled.clear();
            downsampler.process(mono_slice, &mut self.resampled);
            if let Ok(mut buffer) = self.buffer.lock() {
                buffer.extend_from_slice(&self.resampled);
            }
        } else if let Ok(mut buffer) = self.buffer.lock() {
            let estimated = (n_frames as f64 * self.upsample_ratio) as usize + 1;
            buffer.reserve(estimated);
            while self.upsample_position < n_frames as f64 {
                let index = self.upsample_position as usize;
                let fraction = (self.upsample_position - index as f64) as f32;
                let sample = if index + 1 < n_frames {
                    mono(index) * (1.0 - fraction) + mono(index + 1) * fraction
                } else {
                    mono(index)
                };
                buffer.push(sample);
                self.upsample_position += 1.0 / self.upsample_ratio;
            }
            self.upsample_position -= n_frames as f64;
        }
    }
}

fn stream_error_callback(
    is_recording: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
) -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |error| {
        let message = error.to_string();
        // CPAL 0.17 reports transient input overruns through the same callback
        // as fatal disconnects/invalidations. The stream remains usable after
        // an overrun, so retain the captured audio and keep recording.
        if matches!(error, cpal::StreamError::BufferUnderrun) {
            eprintln!("Audio input buffer overrun: {message}");
            return;
        }
        if let Ok(mut slot) = stream_error.lock() {
            *slot = Some(message.clone());
        }
        // A fatal backend error means callbacks can no longer be trusted. This
        // also stops the level task; stop() later returns the recorded error.
        is_recording.store(false, Ordering::Release);
        eprintln!("Audio stream error: {message}");
    }
}

fn build_f32_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut processor: CaptureProcessor,
    error_state: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let error_recording = Arc::clone(&processor.is_recording);
    device.build_input_stream(
        config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| processor.process(data),
        stream_error_callback(error_recording, error_state),
        None,
    )
}

fn build_i16_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut processor: CaptureProcessor,
    error_state: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let error_recording = Arc::clone(&processor.is_recording);
    let mut converted = Vec::new();
    device.build_input_stream(
        config,
        move |data: &[i16], _: &cpal::InputCallbackInfo| {
            converted.clear();
            converted.reserve(data.len());
            converted.extend(data.iter().copied().map(i16_to_f32));
            processor.process(&converted);
        },
        stream_error_callback(error_recording, error_state),
        None,
    )
}

fn build_u16_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut processor: CaptureProcessor,
    error_state: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let error_recording = Arc::clone(&processor.is_recording);
    let mut converted = Vec::new();
    device.build_input_stream(
        config,
        move |data: &[u16], _: &cpal::InputCallbackInfo| {
            converted.clear();
            converted.reserve(data.len());
            converted.extend(data.iter().copied().map(u16_to_f32));
            processor.process(&converted);
        },
        stream_error_callback(error_recording, error_state),
        None,
    )
}

#[inline]
fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32_768.0
}

#[inline]
fn u16_to_f32(sample: u16) -> f32 {
    (sample as f32 - 32_768.0) / 32_768.0
}

fn device_name(device: &cpal::Device) -> String {
    device.description().map_or_else(
        |_| "Unknown microphone".into(),
        |description| device_display_name(description.name(), description.extended()),
    )
}

fn device_display_name(primary: &str, extended: &[String]) -> String {
    let primary = primary.trim();

    // CPAL's WASAPI backend currently prefers DEVPKEY_Device_DeviceDesc for
    // `name()`. For many USB microphones that value is just "Microphone";
    // Windows' useful endpoint label (for example "Microphone (HyperX
    // QuadCast)") is exposed as the first extended line. Use that friendly
    // endpoint label on Windows while retaining CPAL's stable DeviceId for
    // selection and persistence.
    #[cfg(target_os = "windows")]
    if let Some(friendly) = extended
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.eq_ignore_ascii_case(primary))
    {
        return friendly.to_string();
    }

    if primary.is_empty() {
        "Unknown microphone".into()
    } else {
        primary.to_string()
    }
}

fn device_id_or_name(device: &cpal::Device, name: &str) -> String {
    device
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| name.to_string())
}

fn description_matches_saved_name(primary: &str, extended: &[String], saved: &str) -> bool {
    primary.trim().eq_ignore_ascii_case(saved.trim())
        || extended
            .iter()
            .any(|line| line.trim().eq_ignore_ascii_case(saved.trim()))
        || device_display_name(primary, extended).eq_ignore_ascii_case(saved.trim())
}

fn device_matches_saved_name(device: &cpal::Device, saved: &str) -> bool {
    device.description().is_ok_and(|description| {
        description_matches_saved_name(description.name(), description.extended(), saved)
    })
}

impl AudioCapture {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            source: CaptureSource::Microphone,
            buffer: Arc::new(Mutex::new(Vec::with_capacity(BUFFER_CAPACITY))),
            is_recording: Arc::new(AtomicBool::new(false)),
            rms_level: Arc::new(AtomicU32::new(0)),
            stream_error: Arc::new(Mutex::new(None)),
            stream: None,
        }
    }

    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    /// Create a capture stream for the default render endpoint. On Windows,
    /// CPAL's WASAPI backend transparently enables loopback when an output
    /// device is opened as an input stream. Meeting Mode uses this to record
    /// the remote side without installing a virtual audio driver.
    pub fn new_system_loopback() -> Self {
        let mut capture = Self::new(AudioConfig::default());
        capture.source = CaptureSource::SystemLoopback;
        capture
    }

    pub fn enumerate_devices() -> AppResult<Vec<AudioDevice>> {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(Self::enumerate_devices_inner());
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| {
                AppError::Audio(
                "Device enumeration timed out — try unplugging and re-plugging your audio device"
                    .into(),
            )
            })?
    }

    fn enumerate_devices_inner() -> AppResult<Vec<AudioDevice>> {
        let host = cpal::default_host();
        let default_id = host.default_input_device().map(|device| {
            let name = device_name(&device);
            device_id_or_name(&device, &name)
        });
        let input_devices = host
            .input_devices()
            .map_err(|error| AppError::Audio(format!("Failed to enumerate devices: {error}")))?;
        let mut devices = Vec::new();
        for device in input_devices {
            let name = device_name(&device);
            let id = device_id_or_name(&device, &name);
            let is_default = default_id.as_ref() == Some(&id);
            if let Ok(config) = device.default_input_config() {
                devices.push(AudioDevice {
                    id,
                    name,
                    is_default,
                    sample_rate: config.sample_rate(),
                    channels: config.channels(),
                });
            }
        }
        Ok(devices)
    }

    pub fn start(&mut self) -> AppResult<()> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Err(AppError::Audio("Already recording".into()));
        }
        self.buffer.lock().unwrap().clear();
        if let Ok(mut error) = self.stream_error.lock() {
            *error = None;
        }

        let host = cpal::default_host();
        let device = self.resolve_device(&host)?;
        let supported = match self.source {
            CaptureSource::Microphone => device.default_input_config(),
            CaptureSource::SystemLoopback => device.default_output_config(),
        }
        .map_err(|error| AppError::Audio(format!("No capture config: {error}")))?;
        let sample_format = supported.sample_format();
        let device_rate = supported.sample_rate();
        let device_channels = supported.channels();
        let stream_config: cpal::StreamConfig = supported.into();
        let processor = CaptureProcessor::new(
            Arc::clone(&self.buffer),
            Arc::clone(&self.is_recording),
            Arc::clone(&self.rms_level),
            device_rate,
            device_channels,
        );

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_f32_stream(
                &device,
                &stream_config,
                processor,
                Arc::clone(&self.stream_error),
            ),
            cpal::SampleFormat::I16 => build_i16_stream(
                &device,
                &stream_config,
                processor,
                Arc::clone(&self.stream_error),
            ),
            cpal::SampleFormat::U16 => build_u16_stream(
                &device,
                &stream_config,
                processor,
                Arc::clone(&self.stream_error),
            ),
            format => {
                return Err(AppError::Audio(format!(
                    "Unsupported input sample format: {format}"
                )))
            }
        }
        .map_err(|error| AppError::Audio(format!("Failed to build stream: {error}")))?;

        // Arm capture before play: some backends invoke the first callback from
        // inside play(), and arming afterward clipped that first buffer.
        self.is_recording.store(true, Ordering::SeqCst);
        if let Err(error) = stream.play() {
            self.is_recording.store(false, Ordering::SeqCst);
            return Err(AppError::Audio(format!("Failed to start stream: {error}")));
        }
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> AppResult<Vec<f32>> {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream.take();
        self.rms_level.store(0, Ordering::Relaxed);
        let samples = {
            let mut buffer = self.buffer.lock().unwrap();
            std::mem::replace(&mut *buffer, Vec::with_capacity(BUFFER_CAPACITY))
        };
        if let Some(error) = self.take_stream_error() {
            return Err(AppError::Audio(format!("Input stream failed: {error}")));
        }
        Ok(samples)
    }

    pub fn cancel(&mut self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.stream.take();
        self.buffer.lock().unwrap().clear();
        self.rms_level.store(0, Ordering::Relaxed);
        let _ = self.take_stream_error();
    }

    /// Return and clear the last asynchronous backend failure.
    pub fn take_stream_error(&self) -> Option<String> {
        self.stream_error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
    }

    /// Inspect an asynchronous backend failure without consuming it. Meeting
    /// Mode uses this to surface a broken input stream while the call is live;
    /// `stop` still consumes and returns the same failure to its caller.
    pub fn stream_error(&self) -> Option<String> {
        self.stream_error
            .lock()
            .ok()
            .and_then(|error| error.clone())
    }

    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.rms_level.load(Ordering::Relaxed))
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn is_recording_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_recording)
    }

    pub fn rms_level_ref(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.rms_level)
    }

    pub fn duration_secs(&self) -> f32 {
        self.buffer.lock().unwrap().len() as f32 / TARGET_SAMPLE_RATE as f32
    }

    pub fn snapshot_tail(&self, max_samples: usize) -> Vec<f32> {
        let buffer = self
            .buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let start = buffer.len().saturating_sub(max_samples);
        buffer[start..].to_vec()
    }

    /// Atomically take all samples accumulated since the previous drain.
    /// Long-running Meeting Mode calls this frequently so capture memory stays
    /// bounded while the returned block is durably spooled and transcribed.
    pub fn drain_samples(&self) -> Vec<f32> {
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *buffer, Vec::with_capacity(BUFFER_CAPACITY))
    }

    fn resolve_device(&self, host: &cpal::Host) -> AppResult<cpal::Device> {
        if self.source == CaptureSource::SystemLoopback {
            return host
                .default_output_device()
                .ok_or_else(|| AppError::Audio("No default output device".into()));
        }
        if let Some(ref id) = self.config.device_id {
            // CPAL 0.17 exposes stable device identifiers. Older OmniVox
            // settings stored the display name, so retain a name fallback for
            // seamless migration of existing preferences.
            if let Ok(device_id) = id.parse::<cpal::DeviceId>() {
                if let Some(device) = host.device_by_id(&device_id) {
                    return Ok(device);
                }
            }
            host.input_devices()
                .map_err(|error| AppError::Audio(format!("Failed to enumerate: {error}")))?
                .find(|device| device_matches_saved_name(device, id))
                .ok_or_else(|| AppError::Audio(format!("Device '{id}' not found")))
        } else {
            host.default_input_device()
                .ok_or_else(|| AppError::Audio("No default input device".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_display_name_uses_windows_endpoint_friendly_name() {
        let extended = vec!["Microphone (HyperX QuadCast)".to_string()];
        #[cfg(target_os = "windows")]
        assert_eq!(
            device_display_name("Microphone", &extended),
            "Microphone (HyperX QuadCast)"
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(device_display_name("Microphone", &extended), "Microphone");
    }

    #[test]
    fn device_display_name_ignores_blank_or_duplicate_metadata() {
        let extended = vec!["  ".to_string(), "microphone".to_string()];
        assert_eq!(device_display_name("Microphone", &extended), "Microphone");
        assert_eq!(device_display_name(" ", &[]), "Unknown microphone");
    }

    #[test]
    fn saved_microphone_names_match_both_legacy_and_friendly_labels() {
        let extended = vec!["Microphone (HyperX QuadCast)".to_string()];
        assert!(description_matches_saved_name(
            "Microphone",
            &extended,
            "Microphone"
        ));
        assert!(description_matches_saved_name(
            "Microphone",
            &extended,
            "microphone (hyperx quadcast)"
        ));
        assert!(!description_matches_saved_name(
            "Microphone",
            &extended,
            "Laptop Array"
        ));
    }

    #[test]
    fn signed_integer_samples_map_to_normalized_float() {
        assert_eq!(i16_to_f32(i16::MIN), -1.0);
        assert!((i16_to_f32(i16::MAX) - 1.0).abs() < 0.0001);
        assert_eq!(i16_to_f32(0), 0.0);
    }

    #[test]
    fn unsigned_integer_samples_are_centered_at_zero() {
        assert_eq!(u16_to_f32(u16::MIN), -1.0);
        assert!((u16_to_f32(u16::MAX) - 1.0).abs() < 0.0001);
        assert_eq!(u16_to_f32(32_768), 0.0);
    }

    #[test]
    fn capture_exposes_the_selected_device_config_for_meeting_reuse() {
        let capture = AudioCapture::new(AudioConfig {
            device_id: Some("preferred-microphone".into()),
            ..AudioConfig::default()
        });
        assert_eq!(
            capture.config().device_id.as_deref(),
            Some("preferred-microphone")
        );
    }

    #[test]
    fn shared_processor_downmixes_stereo_before_buffering() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::new(AtomicBool::new(true));
        let level = Arc::new(AtomicU32::new(0));
        let mut processor =
            CaptureProcessor::new(Arc::clone(&buffer), recording, level, TARGET_SAMPLE_RATE, 2);
        processor.process(&[1.0, -1.0, 0.5, 0.5]);
        assert_eq!(*buffer.lock().unwrap(), vec![0.0, 0.5]);
    }
}

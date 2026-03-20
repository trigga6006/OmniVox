/// Energy-based Voice Activity Detector.
///
/// Classifies audio frames as speech or silence using RMS energy with
/// hysteresis and minimum-duration constraints to avoid false triggers.
///
/// Designed for dictation: detects when the user stops speaking so the
/// pipeline can auto-stop recording and begin transcription.
pub struct VoiceActivityDetector {
    config: VadConfig,
    /// Current state: true = speech detected, false = silence
    is_speech_active: bool,
    /// Number of consecutive silent frames observed while in speech state
    silent_frame_count: u32,
    /// Number of consecutive speech frames observed while in silence state
    speech_frame_count: u32,
}

#[derive(Debug, Clone)]
pub struct VadConfig {
    /// RMS threshold to consider a frame as speech (0.0–1.0).
    /// Lower = more sensitive, higher = less sensitive.
    pub speech_threshold: f32,
    /// RMS threshold to consider a frame as silence.
    /// Set below speech_threshold to create hysteresis and avoid rapid toggling.
    pub silence_threshold: f32,
    /// Minimum consecutive speech frames before transitioning to "speech" state.
    /// Prevents brief noise spikes from triggering speech.
    pub min_speech_frames: u32,
    /// Consecutive silent frames required to transition from speech → silence.
    /// This is the "trailing silence" that signals the user has stopped talking.
    pub trailing_silence_frames: u32,
    /// Expected frame rate (frames per second) — used to interpret frame counts
    /// as durations. At 16 kHz with 512-sample frames, this is ~31 fps.
    pub frame_rate: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            speech_threshold: 0.015,
            silence_threshold: 0.010,
            min_speech_frames: 3,          // ~100 ms at 31 fps
            trailing_silence_frames: 50,   // ~1.6 s at 31 fps
            frame_rate: 31.25,             // 16000 / 512
        }
    }
}

impl VadConfig {
    /// Configure trailing silence in seconds (more intuitive than frame counts).
    pub fn with_trailing_silence_secs(mut self, secs: f32) -> Self {
        self.trailing_silence_frames = (secs * self.frame_rate) as u32;
        self
    }

    /// Configure minimum speech duration in seconds.
    pub fn with_min_speech_secs(mut self, secs: f32) -> Self {
        self.min_speech_frames = (secs * self.frame_rate) as u32;
        self
    }
}

/// Result of processing an audio frame through the VAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    /// Silence — no speech detected
    Silence,
    /// Speech is ongoing
    Speech,
    /// Transition: silence → speech started
    SpeechStarted,
    /// Transition: speech → silence (trailing silence exceeded).
    /// This is the signal to stop recording and transcribe.
    SpeechEnded,
}

impl VoiceActivityDetector {
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            is_speech_active: false,
            silent_frame_count: 0,
            speech_frame_count: 0,
        }
    }

    /// Process a frame of 16 kHz mono f32 audio and return a VAD event.
    pub fn process_frame(&mut self, samples: &[f32]) -> VadEvent {
        if samples.is_empty() {
            return if self.is_speech_active {
                VadEvent::Speech
            } else {
                VadEvent::Silence
            };
        }

        let rms = Self::compute_rms(samples);

        if self.is_speech_active {
            // Currently in speech state
            if rms < self.config.silence_threshold {
                self.silent_frame_count += 1;
                self.speech_frame_count = 0;

                if self.silent_frame_count >= self.config.trailing_silence_frames {
                    // Enough silence — speech has ended
                    self.is_speech_active = false;
                    self.silent_frame_count = 0;
                    return VadEvent::SpeechEnded;
                }
            } else {
                // Still speaking — reset silence counter
                self.silent_frame_count = 0;
            }
            VadEvent::Speech
        } else {
            // Currently in silence state
            if rms >= self.config.speech_threshold {
                self.speech_frame_count += 1;
                self.silent_frame_count = 0;

                if self.speech_frame_count >= self.config.min_speech_frames {
                    // Enough consecutive speech — transition
                    self.is_speech_active = true;
                    self.speech_frame_count = 0;
                    return VadEvent::SpeechStarted;
                }
            } else {
                self.speech_frame_count = 0;
            }
            VadEvent::Silence
        }
    }

    /// Reset the detector state (e.g., between recording sessions).
    pub fn reset(&mut self) {
        self.is_speech_active = false;
        self.silent_frame_count = 0;
        self.speech_frame_count = 0;
    }

    /// Returns true if the detector currently considers speech to be active.
    pub fn is_speech(&self) -> bool {
        self.is_speech_active
    }

    fn compute_rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }
}

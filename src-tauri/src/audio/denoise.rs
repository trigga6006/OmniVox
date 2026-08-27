use nnnoiseless::DenoiseState;

/// RNNoise frame size: 480 samples at 48 kHz = 10 ms per frame.
const FRAME_SIZE: usize = DenoiseState::FRAME_SIZE;

/// Number of 16 kHz input samples that map to one RNNoise frame.
/// 480 / 3 = 160 samples at 16 kHz = 10 ms.
const INPUT_CHUNK: usize = FRAME_SIZE / 3;

/// Scale factor: nnnoiseless expects f32 in i16 range [-32768, 32767],
/// not the [-1.0, 1.0] range that cpal/our pipeline uses.
const SCALE_UP: f32 = 32767.0;
const SCALE_DOWN: f32 = 1.0 / 32767.0;

/// Conservative thresholds used only to reject captures that both detectors
/// agree are silence. Borderline input deliberately remains transcribable.
const SILENCE_MAX_WINDOW_RMS: f32 = 0.002;
const SILENCE_MAX_PEAK: f32 = 0.01;
const SILENCE_MAX_VAD_PROBABILITY: f32 = 0.35;
const SPEECH_VAD_PROBABILITY: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechDecision {
    Speech,
    Silence,
    /// A detector was unavailable, input was too short, or the detectors
    /// disagreed. Callers must transcribe this fail-open outcome.
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechAnalysis {
    pub decision: SpeechDecision,
    pub peak: f32,
    pub max_window_rms: f32,
    pub max_vad_probability: Option<f32>,
    pub speech_frames: usize,
    pub total_frames: usize,
}

impl SpeechAnalysis {
    /// Safe gate for the ASR pipeline: only a high-confidence silence decision
    /// is rejected. Unknown and detector-disagreement cases fail open.
    pub const fn should_transcribe(self) -> bool {
        !matches!(self.decision, SpeechDecision::Silence)
    }
}

/// Denoise 16 kHz mono audio in-place using RNNoise.
///
/// Processes frame-by-frame: each 160-sample chunk of 16 kHz input is
/// upsampled into a 480-sample stack buffer, denoised, and downsampled
/// back to 160 output samples. Only a single output Vec is heap-allocated;
/// all intermediate buffers live on the stack.
///
/// For 10 s of 16 kHz audio (~160k samples → 1000 frames),
/// processing takes ~5–15 ms on a modern desktop CPU.
pub fn denoise(samples: &mut Vec<f32>) {
    let _ = denoise_with_speech_analysis(samples);
}

/// Denoise audio and return a conservative speech/silence decision using both
/// raw amplitude and RNNoise's frame probabilities in the same pass.
pub fn denoise_with_speech_analysis(samples: &mut Vec<f32>) -> SpeechAnalysis {
    // RNNoise cannot make a meaningful decision without one complete 10 ms
    // input frame. Preserve these captures and fail open just like the
    // analysis-only path instead of classifying zero-padded fragments.
    if samples.len() < INPUT_CHUNK {
        return amplitude_only_analysis(samples);
    }

    let input = &*samples as &[f32];
    let (peak, max_window_rms) = amplitude_observations(input);
    if !peak.is_finite() || !max_window_rms.is_finite() {
        return classify(peak, max_window_rms, None, 0, 0);
    }
    let mut state = DenoiseState::new();
    let mut output = Vec::with_capacity(input.len());
    let mut in_frame = [0.0f32; FRAME_SIZE];
    let mut out_frame = [0.0f32; FRAME_SIZE];

    let n_chunks = input.len() / INPUT_CHUNK;
    let remainder = input.len() % INPUT_CHUNK;
    let mut max_vad_probability = 0.0f32;
    let mut speech_frames = 0usize;

    for chunk_idx in 0..n_chunks {
        let start = chunk_idx * INPUT_CHUNK;

        // Upsample 160 input samples → 480-sample frame with scaling,
        // using only the stack-allocated in_frame buffer.
        for i in 0..INPUT_CHUNK {
            let a = input[start + i];
            let b = if start + i + 1 < input.len() {
                input[start + i + 1]
            } else {
                a
            };
            let o = i * 3;
            in_frame[o] = a * SCALE_UP;
            in_frame[o + 1] = (a + (b - a) / 3.0) * SCALE_UP;
            in_frame[o + 2] = (a + 2.0 * (b - a) / 3.0) * SCALE_UP;
        }

        let vad = state.process_frame(&mut out_frame, &in_frame);
        max_vad_probability = max_vad_probability.max(vad);
        if vad >= SPEECH_VAD_PROBABILITY {
            speech_frames += 1;
        }

        if chunk_idx == 0 {
            // First frame has fade-in artifacts — pass through original audio
            output.extend_from_slice(&input[start..start + INPUT_CHUNK]);
        } else {
            // Downsample: take every 3rd denoised sample
            for j in (0..FRAME_SIZE).step_by(3) {
                output.push(out_frame[j] * SCALE_DOWN);
            }
        }
    }

    // Handle remaining samples (partial frame, zero-padded)
    if remainder > 0 {
        let start = n_chunks * INPUT_CHUNK;
        in_frame = [0.0f32; FRAME_SIZE];

        for i in 0..remainder {
            let a = input[start + i];
            let b = if start + i + 1 < input.len() {
                input[start + i + 1]
            } else {
                a
            };
            let o = i * 3;
            in_frame[o] = a * SCALE_UP;
            in_frame[o + 1] = (a + (b - a) / 3.0) * SCALE_UP;
            in_frame[o + 2] = (a + 2.0 * (b - a) / 3.0) * SCALE_UP;
        }

        let vad = state.process_frame(&mut out_frame, &in_frame);
        max_vad_probability = max_vad_probability.max(vad);
        if vad >= SPEECH_VAD_PROBABILITY {
            speech_frames += 1;
        }

        if n_chunks == 0 {
            // Very short audio (< 160 samples) — pass through
            output.extend_from_slice(&input[start..start + remainder]);
        } else {
            for j in (0..remainder * 3).step_by(3) {
                output.push(out_frame[j] * SCALE_DOWN);
            }
        }
    }

    *samples = output;
    classify(
        peak,
        max_window_rms,
        Some(max_vad_probability),
        speech_frames,
        n_chunks + usize::from(remainder > 0),
    )
}

/// Analyze without changing the captured samples. Clearly audible input exits
/// after the amplitude pass; quiet candidates use RNNoise to confirm silence.
pub fn analyze_speech(samples: &[f32]) -> SpeechAnalysis {
    if samples.len() < INPUT_CHUNK {
        return amplitude_only_analysis(samples);
    }

    let (peak, max_window_rms) = amplitude_observations(samples);
    if !peak.is_finite() || !max_window_rms.is_finite() {
        return classify(peak, max_window_rms, None, 0, 0);
    }
    // The final classifier always accepts an above-threshold amplitude as
    // speech. Avoid a redundant RNNoise pass for ordinary audible utterances;
    // RNNoise is only needed to confirm that quiet input is truly silence.
    if max_window_rms >= SILENCE_MAX_WINDOW_RMS {
        return SpeechAnalysis {
            decision: SpeechDecision::Speech,
            peak,
            max_window_rms,
            max_vad_probability: None,
            speech_frames: 0,
            total_frames: 0,
        };
    }
    let mut state = DenoiseState::new();
    let mut in_frame = [0.0f32; FRAME_SIZE];
    let mut out_frame = [0.0f32; FRAME_SIZE];
    let mut max_vad_probability = 0.0f32;
    let mut speech_frames = 0usize;
    let mut total_frames = 0usize;

    for chunk in samples.chunks(INPUT_CHUNK) {
        in_frame.fill(0.0);
        fill_upsampled_frame(chunk, &mut in_frame);
        let vad = state.process_frame(&mut out_frame, &in_frame);
        max_vad_probability = max_vad_probability.max(vad);
        if vad >= SPEECH_VAD_PROBABILITY {
            speech_frames += 1;
        }
        total_frames += 1;
    }

    classify(
        peak,
        max_window_rms,
        Some(max_vad_probability),
        speech_frames,
        total_frames,
    )
}

fn fill_upsampled_frame(input: &[f32], frame: &mut [f32; FRAME_SIZE]) {
    for (i, &a) in input.iter().enumerate() {
        let b = input.get(i + 1).copied().unwrap_or(a);
        let output = i * 3;
        frame[output] = a * SCALE_UP;
        frame[output + 1] = (a + (b - a) / 3.0) * SCALE_UP;
        frame[output + 2] = (a + 2.0 * (b - a) / 3.0) * SCALE_UP;
    }
}

fn amplitude_observations(samples: &[f32]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut max_window_rms = 0.0f32;
    for window in samples.chunks(INPUT_CHUNK) {
        let mut sum_sq = 0.0f32;
        for &sample in window {
            let magnitude = sample.abs();
            if !magnitude.is_finite() {
                return (f32::NAN, f32::NAN);
            }
            peak = peak.max(magnitude);
            sum_sq += sample * sample;
        }
        max_window_rms = max_window_rms.max((sum_sq / window.len() as f32).sqrt());
    }
    (peak, max_window_rms)
}

fn amplitude_only_analysis(samples: &[f32]) -> SpeechAnalysis {
    let (peak, max_window_rms) = amplitude_observations(samples);
    classify(peak, max_window_rms, None, 0, 0)
}

fn classify(
    peak: f32,
    max_window_rms: f32,
    max_vad_probability: Option<f32>,
    speech_frames: usize,
    total_frames: usize,
) -> SpeechAnalysis {
    let finite = peak.is_finite()
        && max_window_rms.is_finite()
        && max_vad_probability.map(f32::is_finite).unwrap_or(true);
    let decision = if !finite || total_frames == 0 {
        SpeechDecision::Uncertain
    } else if speech_frames > 0 || max_window_rms >= SILENCE_MAX_WINDOW_RMS {
        SpeechDecision::Speech
    } else if peak < SILENCE_MAX_PEAK
        && max_vad_probability
            .map(|probability| probability < SILENCE_MAX_VAD_PROBABILITY)
            .unwrap_or(false)
    {
        SpeechDecision::Silence
    } else {
        SpeechDecision::Uncertain
    };

    SpeechAnalysis {
        decision,
        peak,
        max_window_rms,
        max_vad_probability,
        speech_frames,
        total_frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denoise_empty_is_noop() {
        let mut samples: Vec<f32> = vec![];
        denoise(&mut samples);
        assert!(samples.is_empty());
    }

    #[test]
    fn denoise_single_sample_is_noop() {
        let mut samples = vec![0.5f32];
        denoise(&mut samples);
        assert_eq!(samples.len(), 1);
    }

    #[test]
    fn denoise_preserves_approximate_length() {
        // 1 second of 16 kHz silence
        let mut samples = vec![0.0f32; 16_000];
        let original_len = samples.len();
        denoise(&mut samples);
        // Length should be approximately the same (within a few samples
        // due to integer rounding in up/downsample)
        assert!((samples.len() as i64 - original_len as i64).abs() <= 3);
    }

    #[test]
    fn silence_is_rejected_only_when_amplitude_and_vad_agree() {
        let analysis = analyze_speech(&[0.0; 16_000]);
        assert_eq!(analysis.decision, SpeechDecision::Silence);
        assert!(!analysis.should_transcribe());
    }

    #[test]
    fn audible_input_is_kept_even_without_a_vad_vote() {
        let samples: Vec<f32> = (0..16_000)
            .map(|i| ((i as f32 * 0.05).sin()) * 0.02)
            .collect();
        let analysis = analyze_speech(&samples);
        assert_eq!(analysis.decision, SpeechDecision::Speech);
        assert!(analysis.should_transcribe());
    }

    #[test]
    fn invalid_or_too_short_input_fails_open() {
        let invalid = analyze_speech(&[f32::NAN; INPUT_CHUNK]);
        assert_eq!(invalid.decision, SpeechDecision::Uncertain);
        assert!(invalid.should_transcribe());

        let short = analyze_speech(&[0.0; INPUT_CHUNK - 1]);
        assert_eq!(short.decision, SpeechDecision::Uncertain);
        assert!(short.should_transcribe());

        let mut short_with_denoise = vec![0.0; INPUT_CHUNK - 1];
        let denoised = denoise_with_speech_analysis(&mut short_with_denoise);
        assert_eq!(denoised.decision, SpeechDecision::Uncertain);
        assert!(denoised.should_transcribe());
        assert_eq!(short_with_denoise.len(), INPUT_CHUNK - 1);
    }

    #[test]
    fn upsample_downsample_roundtrip() {
        // Upsample by 3x (linear interp) then downsample (every 3rd) should
        // recover the original samples.
        let input = [0.0, 0.5, 1.0, 0.5, 0.0];
        let mut up = Vec::with_capacity(input.len() * 3);
        for i in 0..input.len() {
            let a = input[i];
            let b = if i + 1 < input.len() { input[i + 1] } else { a };
            up.push(a);
            up.push(a + (b - a) / 3.0);
            up.push(a + 2.0 * (b - a) / 3.0);
        }
        assert_eq!(up.len(), input.len() * 3);
        let down: Vec<f32> = up.iter().step_by(3).copied().collect();
        for (a, b) in input.iter().zip(down.iter()) {
            assert!((a - b).abs() < 0.01, "mismatch: {a} vs {b}");
        }
    }
}

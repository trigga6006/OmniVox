use nnnoiseless::DenoiseState;

/// RNNoise frame size: 480 samples at 48 kHz = 10 ms per frame.
const FRAME_SIZE: usize = DenoiseState::FRAME_SIZE;

/// Scale factor: nnnoiseless expects f32 in i16 range [-32768, 32767],
/// not the [-1.0, 1.0] range that cpal/our pipeline uses.
const SCALE_UP: f32 = 32767.0;
const SCALE_DOWN: f32 = 1.0 / 32767.0;

/// Denoise 16 kHz mono audio in-place using RNNoise.
///
/// Pipeline: upsample 16 kHz → 48 kHz (3x linear interp) → RNNoise
/// frame-by-frame → downsample 48 kHz → 16 kHz → replace buffer.
///
/// This is a batch operation on the full recorded buffer. For 10 s of
/// 16 kHz audio (~160k samples → 480k at 48 kHz → 1000 frames),
/// processing takes ~5–15 ms on a modern desktop CPU.
pub fn denoise(samples: &mut Vec<f32>) {
    if samples.len() < 2 {
        return;
    }

    let upsampled = upsample_3x(samples);
    let denoised = denoise_48k(&upsampled);
    let downsampled = downsample_3x(&denoised);

    samples.clear();
    samples.extend_from_slice(&downsampled);
}

/// Upsample by factor of 3 using linear interpolation.
fn upsample_3x(input: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(input.len() * 3);
    for i in 0..input.len() - 1 {
        let a = input[i];
        let b = input[i + 1];
        out.push(a);
        out.push(a + (b - a) / 3.0);
        out.push(a + 2.0 * (b - a) / 3.0);
    }
    // Last sample
    let last = *input.last().unwrap();
    out.push(last);
    out.push(last);
    out.push(last);
    out
}

/// Downsample by factor of 3 (take every 3rd sample).
fn downsample_3x(input: &[f32]) -> Vec<f32> {
    input.iter().step_by(3).copied().collect()
}

/// Apply RNNoise to 48 kHz audio, processing in 480-sample frames.
fn denoise_48k(samples: &[f32]) -> Vec<f32> {
    let mut state = DenoiseState::new();
    let mut output = Vec::with_capacity(samples.len());

    let mut in_frame = [0.0f32; FRAME_SIZE];
    let mut out_frame = [0.0f32; FRAME_SIZE];

    let total_frames = samples.len() / FRAME_SIZE;
    let remainder = samples.len() % FRAME_SIZE;

    for i in 0..total_frames {
        let start = i * FRAME_SIZE;
        // Scale to i16 range for nnnoiseless
        for j in 0..FRAME_SIZE {
            in_frame[j] = samples[start + j] * SCALE_UP;
        }

        let _vad = state.process_frame(&mut out_frame, &in_frame);

        if i == 0 {
            // First frame has fade-in artifacts — pass through original audio
            for j in 0..FRAME_SIZE {
                output.push(samples[start + j]);
            }
        } else {
            for j in 0..FRAME_SIZE {
                output.push(out_frame[j] * SCALE_DOWN);
            }
        }
    }

    // Handle remaining samples (zero-pad last frame)
    if remainder > 0 {
        let start = total_frames * FRAME_SIZE;
        in_frame = [0.0f32; FRAME_SIZE];
        for j in 0..remainder {
            in_frame[j] = samples[start + j] * SCALE_UP;
        }

        let _vad = state.process_frame(&mut out_frame, &in_frame);

        // If this is also the first frame (very short audio), pass through
        if total_frames == 0 {
            for j in 0..remainder {
                output.push(samples[start + j]);
            }
        } else {
            for j in 0..remainder {
                output.push(out_frame[j] * SCALE_DOWN);
            }
        }
    }

    output
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
    fn upsample_downsample_roundtrip() {
        let input = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        let up = upsample_3x(&input);
        assert_eq!(up.len(), input.len() * 3);
        let down = downsample_3x(&up);
        for (a, b) in input.iter().zip(down.iter()) {
            assert!((a - b).abs() < 0.01, "mismatch: {a} vs {b}");
        }
    }
}

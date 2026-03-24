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
    if samples.len() < 2 {
        return;
    }

    let input = &*samples as &[f32];
    let mut state = DenoiseState::new();
    let mut output = Vec::with_capacity(input.len());
    let mut in_frame = [0.0f32; FRAME_SIZE];
    let mut out_frame = [0.0f32; FRAME_SIZE];

    let n_chunks = input.len() / INPUT_CHUNK;
    let remainder = input.len() % INPUT_CHUNK;

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

        let _vad = state.process_frame(&mut out_frame, &in_frame);

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

        let _vad = state.process_frame(&mut out_frame, &in_frame);

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
        // Upsample by 3x (linear interp) then downsample (every 3rd) should
        // recover the original samples.
        let input = vec![0.0, 0.5, 1.0, 0.5, 0.0];
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

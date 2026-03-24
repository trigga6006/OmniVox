/// Target peak amplitude after normalization.
const TARGET_PEAK: f32 = 0.9;

/// Minimum peak amplitude below which normalization is skipped.
/// Prevents amplifying near-silence/noise to full volume.
const SILENCE_THRESHOLD: f32 = 0.01;

/// Normalize audio samples so the peak amplitude reaches TARGET_PEAK.
///
/// This is peak normalization (not RMS): finds the absolute maximum sample
/// and scales all samples uniformly so that maximum reaches 0.9.
///
/// Safety guards:
/// - If peak < SILENCE_THRESHOLD (0.01), audio is near-silence — returns unchanged.
/// - If peak >= TARGET_PEAK, audio is already loud enough — returns unchanged.
/// - Never clips: output peak is exactly TARGET_PEAK.
///
/// Performance: Two passes over the buffer (find peak + scale). O(n) where n
/// is typically 160,000 samples (10s of audio at 16kHz) = sub-millisecond.
pub fn normalize_peak(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }

    // Pass 1: find peak absolute value
    let peak = samples.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);

    // Skip if silence or already loud enough
    if peak < SILENCE_THRESHOLD || peak >= TARGET_PEAK {
        return;
    }

    // Pass 2: scale all samples
    let gain = TARGET_PEAK / peak;
    for sample in samples.iter_mut() {
        *sample *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_quiet_audio() {
        let mut samples = vec![0.1, -0.05, 0.08, -0.1];
        normalize_peak(&mut samples);
        // Peak was 0.1, should now be 0.9
        assert!((samples[0] - 0.9).abs() < 0.001);
        assert!((samples[3] - (-0.9)).abs() < 0.001);
    }

    #[test]
    fn skips_loud_audio() {
        let mut samples = vec![0.95, -0.8, 0.5];
        let original = samples.clone();
        normalize_peak(&mut samples);
        // Peak 0.95 >= TARGET_PEAK, should be unchanged
        assert_eq!(samples, original);
    }

    #[test]
    fn skips_silence() {
        let mut samples = vec![0.001, -0.002, 0.0005];
        let original = samples.clone();
        normalize_peak(&mut samples);
        // Peak 0.002 < SILENCE_THRESHOLD, should be unchanged
        assert_eq!(samples, original);
    }

    #[test]
    fn handles_empty() {
        let mut samples: Vec<f32> = vec![];
        normalize_peak(&mut samples);
        assert!(samples.is_empty());
    }
}

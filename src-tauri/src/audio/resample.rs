//! Streaming anti-aliased downsampler for microphone capture.
//!
//! Windows mics almost always deliver 48 kHz (WASAPI shared mode), which we
//! decimate to the 16 kHz Whisper expects.  Plain linear interpolation — the
//! previous approach — performs no band-limiting, so everything between
//! 8 kHz and the device Nyquist folds back into the speech band as aliasing
//! noise.  Sibilants, keyboard clatter, and hiss all land on top of the
//! 0–8 kHz spectrum Whisper's mel features are computed from.
//!
//! This module low-passes with a windowed-sinc FIR before decimating:
//! passband to ~6.8 kHz, full ~53 dB stopband attenuation by 8 kHz (the
//! output Nyquist).  Cost is a few million multiply-adds per second of
//! audio — microseconds per capture callback.

const OUTPUT_RATE: f64 = 16_000.0;

/// Passband edge → stopband edge (output Nyquist) transition width.
const TRANSITION_HZ: f64 = 1_200.0;
/// −6 dB cutoff, centered in the 6.8–8.0 kHz transition band.
const CUTOFF_HZ: f64 = 7_400.0;

/// Design a Hamming-windowed sinc low-pass for the given input rate.
///
/// Tap count scales with the rate so the transition band stays ~1.2 kHz wide
/// (Hamming transition ≈ 3.3/N normalized), clamped to keep degenerate rates
/// from producing silly filters.
fn design_lowpass(input_rate: f64) -> Vec<f32> {
    let mut n = (3.3 * input_rate / TRANSITION_HZ).ceil() as usize;
    if n % 2 == 0 {
        n += 1;
    }
    let n = n.clamp(33, 255);

    let fc = CUTOFF_HZ / input_rate; // normalized cutoff, cycles/sample
    let mid = (n / 2) as isize;
    let mut taps = Vec::with_capacity(n);
    for i in 0..n as isize {
        let k = (i - mid) as f64;
        // Ideal low-pass: h[k] = 2·fc·sinc(2·fc·k)
        let sinc = if k == 0.0 {
            2.0 * fc
        } else {
            (2.0 * std::f64::consts::PI * fc * k).sin() / (std::f64::consts::PI * k)
        };
        let window = 0.54
            - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
        taps.push((sinc * window) as f32);
    }

    // Normalize DC gain to exactly 1.0 so speech level is preserved.
    let sum: f32 = taps.iter().sum();
    for t in &mut taps {
        *t /= sum;
    }
    taps
}

/// Streaming FIR low-pass + fractional decimator.
///
/// Feed arbitrary-sized chunks of mono samples at the input rate; filtered
/// state and the fractional read position carry across calls, so chunk
/// boundaries introduce no discontinuities (a chunked stream produces
/// bit-identical output to a single large call).
pub struct StreamDownsampler {
    taps: Vec<f32>,
    /// Input samples advanced per output sample (input_rate / 16k, > 1).
    step: f64,
    /// Fractional read position into the filtered-sample sequence.
    pos: f64,
    /// `taps.len()` carried input samples — the overlap each new chunk needs
    /// so its first filtered sample continues the previous chunk's last.
    hist: Vec<f32>,
    /// Scratch: `hist ++ chunk`, reused across calls.
    ext: Vec<f32>,
}

impl StreamDownsampler {
    /// `input_rate` must be greater than 16 000 — use a pass-through or the
    /// linear upsampling path otherwise.
    pub fn new(input_rate: u32) -> Self {
        Self {
            taps: design_lowpass(input_rate as f64),
            step: input_rate as f64 / OUTPUT_RATE,
            pos: 0.0,
            hist: Vec::new(),
            ext: Vec::new(),
        }
    }

    /// Filtered sample at index `i` of the extended buffer (needs
    /// `ext[i .. i + taps.len()]`).
    #[inline]
    fn filtered(&self, i: usize) -> f32 {
        let window = &self.ext[i..i + self.taps.len()];
        self.taps
            .iter()
            .zip(window)
            .map(|(t, s)| t * s)
            .sum()
    }

    /// Low-pass, decimate, and append the 16 kHz result to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        let n_taps = self.taps.len();

        self.ext.clear();
        self.ext.extend_from_slice(&self.hist);
        self.ext.extend_from_slice(input);

        // Not enough samples to produce a single filtered value yet — carry
        // everything and wait for the next chunk (only happens in the first
        // few milliseconds of a recording).
        if self.ext.len() < n_taps {
            self.hist.clear();
            self.hist.extend_from_slice(&self.ext);
            return;
        }

        // Filtered samples y(0) ..= y(n_y - 1) are computable.
        let n_y = self.ext.len() - (n_taps - 1);
        let last = (n_y - 1) as f64;

        out.reserve((n_y as f64 / self.step) as usize + 1);
        while self.pos <= last {
            let idx = self.pos as usize;
            let frac = (self.pos - idx as f64) as f32;
            let a = self.filtered(idx);
            // `idx + 1` is in range whenever frac > 0: pos < last implies
            // idx + 1 <= n_y - 1, and pos == last floors to frac == 0.
            let sample = if frac > 0.0 {
                let b = self.filtered(idx + 1);
                a + (b - a) * frac
            } else {
                a
            };
            out.push(sample);
            self.pos += self.step;
        }

        // Re-base the position so index 0 of the NEXT chunk's filtered
        // sequence is this chunk's y(n_y - 1): we retain the last `n_taps`
        // input samples, and y over that overlap starts exactly there.
        self.pos -= last;
        let keep_from = self.ext.len() - n_taps;
        self.hist.clear();
        self.hist.extend_from_slice(&self.ext[keep_from..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate).sin() as f32)
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    #[test]
    fn preserves_speech_band() {
        // 1 kHz tone at 48 kHz should come through at full level.
        let input = sine(1_000.0, 48_000.0, 48_000);
        let mut ds = StreamDownsampler::new(48_000);
        let mut out = Vec::new();
        ds.process(&input, &mut out);
        let r = rms(&out);
        // Pure sine RMS is 1/√2 ≈ 0.707.
        assert!(
            (r - 0.707).abs() < 0.02,
            "1 kHz tone should pass unattenuated, got RMS {r}"
        );
    }

    #[test]
    fn rejects_aliasing_band() {
        // A 10 kHz tone would alias to 6 kHz under naive decimation; the
        // low-pass must crush it (~53 dB stopband → amplitude ~0.002).
        let input = sine(10_000.0, 48_000.0, 48_000);
        let mut ds = StreamDownsampler::new(48_000);
        let mut out = Vec::new();
        ds.process(&input, &mut out);
        let r = rms(&out);
        assert!(
            r < 0.01,
            "10 kHz tone should be rejected by the anti-aliasing filter, got RMS {r}"
        );
    }

    #[test]
    fn preserves_dc_level() {
        let input = vec![0.5f32; 48_000];
        let mut ds = StreamDownsampler::new(48_000);
        let mut out = Vec::new();
        ds.process(&input, &mut out);
        // Skip the filter warm-up edge at the start.
        let steady = &out[16..];
        for s in steady {
            assert!((s - 0.5).abs() < 0.001, "DC level shifted: {s}");
        }
    }

    #[test]
    fn output_length_matches_ratio() {
        let input = sine(440.0, 48_000.0, 48_000);
        let mut ds = StreamDownsampler::new(48_000);
        let mut out = Vec::new();
        ds.process(&input, &mut out);
        // One second of 48 kHz in → ~16k samples out, minus the filter
        // startup (the first n_taps-1 input samples produce no output).
        assert!(
            (15_900..=16_001).contains(&out.len()),
            "expected ~16000 output samples, got {}",
            out.len()
        );
    }

    #[test]
    fn streaming_chunks_match_one_shot() {
        // Chunked processing must be bit-identical to one big call —
        // proves state carries correctly across capture callbacks.
        let input = sine(2_337.0, 48_000.0, 9_600);

        let mut one = Vec::new();
        StreamDownsampler::new(48_000).process(&input, &mut one);

        let mut chunked = Vec::new();
        let mut ds = StreamDownsampler::new(48_000);
        for chunk in input.chunks(480) {
            ds.process(chunk, &mut chunked);
        }

        assert_eq!(one.len(), chunked.len());
        for (i, (a, b)) in one.iter().zip(chunked.iter()).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "sample {i} diverged: one-shot {a} vs chunked {b}"
            );
        }
    }

    #[test]
    fn handles_44100_rate() {
        // Non-integer ratio (44.1 kHz → 16 kHz = 2.756…) must still pass
        // speech and reject the alias band.
        let pass = sine(1_000.0, 44_100.0, 44_100);
        let mut ds = StreamDownsampler::new(44_100);
        let mut out = Vec::new();
        ds.process(&pass, &mut out);
        assert!((rms(&out) - 0.707).abs() < 0.02);

        let reject = sine(12_000.0, 44_100.0, 44_100);
        let mut ds = StreamDownsampler::new(44_100);
        let mut out = Vec::new();
        ds.process(&reject, &mut out);
        assert!(rms(&out) < 0.01);
    }
}

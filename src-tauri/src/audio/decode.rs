//! Decode arbitrary audio files (WAV / MP3 / M4A / FLAC / OGG / AAC) into
//! the `16 kHz mono f32` shape Whisper expects.
//!
//! Pure-Rust via symphonia (demux + decode) + rubato (resample).  No
//! ffmpeg dependency — shipping ffmpeg on Windows is a deploy headache.
//!
//! The progress callback receives `0.0–1.0` and is throttled internally
//! (only fires when the value changes by ≥1 %).  Cancellation is
//! cooperative: every packet boundary checks `cancel`, returning
//! `AppError::Cancelled` if it has flipped to true.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rubato::{FftFixedIn, Resampler};
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{AppError, AppResult};

/// Whisper's required sample rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Result of decoding + resampling an audio file.
#[derive(Debug)]
pub struct DecodedAudio {
    /// Interleaved mono samples at `TARGET_SAMPLE_RATE`, peak in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
    /// Source media duration, derived from the resampled sample count.
    pub duration_ms: u64,
}

/// Decode + resample `path` to 16 kHz mono f32.
///
/// `progress` receives `0.0–1.0` of decode completion.  The resample
/// step at the end emits one final `1.0` tick so callers always see the
/// bar fill.
///
/// `cancel` is polled between packets — set to `true` from another
/// thread to short-circuit; the function returns `AppError::Cancelled`.
pub fn decode_file<F>(
    path: &Path,
    mut progress: F,
    cancel: &Arc<AtomicBool>,
) -> AppResult<DecodedAudio>
where
    F: FnMut(f32),
{
    let file = std::fs::File::open(path)
        .map_err(|e| AppError::Decode(format!("open {}: {e}", path.display())))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AppError::Decode(format!("probe failed: {e}")))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| AppError::Decode("file contains no audio track".into()))?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let source_sr = codec_params
        .sample_rate
        .ok_or_else(|| AppError::Decode("source sample rate unknown".into()))?;
    let channels = codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1)
        .max(1);
    let n_frames = codec_params.n_frames; // total source frames if known

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| AppError::Decode(format!("decoder init failed: {e}")))?;

    let mut mono_in: Vec<f32> = match n_frames {
        Some(n) => Vec::with_capacity(n as usize),
        None => Vec::new(),
    };

    let mut last_progress = -1.0_f32;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }

        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(AppError::Decode(format!("read packet: {e}"))),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let audio_buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            // A single corrupt frame in a long file shouldn't abort —
            // skip it and keep going.  Hard errors still propagate.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(AppError::Decode(format!("decode: {e}"))),
        };

        append_mono_f32(audio_buf, &mut mono_in, channels);

        if let Some(total) = n_frames {
            let pct = (packet.ts() as f64 / total as f64).clamp(0.0, 1.0) as f32;
            // Reserve the last 5 % of the bar for the resample pass so
            // the user sees motion even when decode finishes fast.
            let scaled = pct * 0.95;
            if scaled - last_progress >= 0.01 {
                progress(scaled);
                last_progress = scaled;
            }
        }
    }

    // Resample to 16 kHz if needed.  The resampler polls `cancel`
    // between chunks so a multi-minute file at 48 kHz stays
    // responsive to cancellation through the entire pass.
    let samples = if source_sr == TARGET_SAMPLE_RATE {
        mono_in
    } else {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        resample_to_target(&mono_in, source_sr, cancel)?
    };

    progress(1.0);

    let duration_ms = if samples.is_empty() {
        0
    } else {
        (samples.len() as u64 * 1000) / TARGET_SAMPLE_RATE as u64
    };

    Ok(DecodedAudio {
        samples,
        duration_ms,
    })
}

/// Interleave + convert an `AudioBufferRef` to mono `f32` and append to `out`.
fn append_mono_f32(audio_buf: AudioBufferRef<'_>, out: &mut Vec<f32>, channels: usize) {
    let spec = *audio_buf.spec();
    let frames = audio_buf.capacity() as u64;
    let mut sample_buf = SampleBuffer::<f32>::new(frames, spec);
    sample_buf.copy_interleaved_ref(audio_buf);
    let samples = sample_buf.samples();

    if channels <= 1 {
        out.extend_from_slice(samples);
        return;
    }

    // Average channels into mono.
    out.reserve(samples.len() / channels);
    let inv = 1.0 / channels as f32;
    for frame in samples.chunks_exact(channels) {
        let sum: f32 = frame.iter().sum();
        out.push(sum * inv);
    }
}

/// Resample `mono` from `source_sr` Hz to `TARGET_SAMPLE_RATE` Hz using
/// rubato's FFT-based fixed-in resampler.  Speech is forgiving — FFT is
/// faster than sinc and indistinguishable at 16 kHz.
///
/// Polls `cancel` between chunks (and before the partial flush) so a
/// multi-minute non-16 kHz file stays responsive to user cancellation
/// through the full resample pass, not just packet decode.
fn resample_to_target(
    mono: &[f32],
    source_sr: u32,
    cancel: &Arc<AtomicBool>,
) -> AppResult<Vec<f32>> {
    const CHUNK: usize = 1024;
    const SUB_CHUNKS: usize = 2;

    let mut resampler = FftFixedIn::<f32>::new(
        source_sr as usize,
        TARGET_SAMPLE_RATE as usize,
        CHUNK,
        SUB_CHUNKS,
        1, // mono
    )
    .map_err(|e| AppError::Decode(format!("rubato init: {e}")))?;

    let estimated =
        ((mono.len() as u64 * TARGET_SAMPLE_RATE as u64) / source_sr as u64) as usize;
    let mut output: Vec<f32> = Vec::with_capacity(estimated + CHUNK);

    let chunk_in = resampler.input_frames_next();
    let mut pos = 0;
    while pos + chunk_in <= mono.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let slice = vec![mono[pos..pos + chunk_in].to_vec()];
        let out = resampler
            .process(&slice, None)
            .map_err(|e| AppError::Decode(format!("rubato process: {e}")))?;
        output.extend_from_slice(&out[0]);
        pos += chunk_in;
    }

    // Flush the tail with zero padding via process_partial.
    if pos < mono.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let tail: Vec<Vec<f32>> = vec![mono[pos..].to_vec()];
        let out = resampler
            .process_partial(Some(&tail), None)
            .map_err(|e| AppError::Decode(format!("rubato flush: {e}")))?;
        output.extend_from_slice(&out[0]);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Generate a 1-second 16 kHz mono WAV (sine wave) and decode it.
    /// Sanity-checks the WAV path without a real fixture file.
    #[test]
    fn decodes_synthetic_wav_at_target_rate() {
        let path = std::env::temp_dir().join("omnivox_decode_test.wav");
        write_sine_wav(&path, TARGET_SAMPLE_RATE, 1, 1.0, 440.0);

        let cancel = Arc::new(AtomicBool::new(false));
        let mut last_pct = 0.0;
        let decoded = decode_file(&path, |p| last_pct = p, &cancel).expect("decode");

        assert!(last_pct >= 0.99, "progress should end near 1.0, got {last_pct}");
        // 1 second at 16 kHz = 16000 samples (give or take rubato latency,
        // but here source==target so no resample).
        assert_eq!(decoded.samples.len(), TARGET_SAMPLE_RATE as usize);
        assert!(decoded.duration_ms >= 990 && decoded.duration_ms <= 1010);
        // Sine wave is bounded in [-1, 1].
        assert!(decoded.samples.iter().all(|s| s.abs() <= 1.001));
        let _ = std::fs::remove_file(&path);
    }

    /// Decode a 48 kHz mono WAV and confirm it gets resampled to 16 kHz.
    #[test]
    fn resamples_48k_to_16k() {
        let path = std::env::temp_dir().join("omnivox_decode_test_48k.wav");
        write_sine_wav(&path, 48_000, 1, 1.0, 440.0);

        let cancel = Arc::new(AtomicBool::new(false));
        let decoded = decode_file(&path, |_| {}, &cancel).expect("decode");

        // Resampled to 16 kHz → ~16000 samples for 1 s of input.
        let delta = (decoded.samples.len() as i64 - TARGET_SAMPLE_RATE as i64).abs();
        assert!(delta < 200, "resampled length off by {delta}");
        let _ = std::fs::remove_file(&path);
    }

    /// Cancellation flips during decode → returns Cancelled.
    #[test]
    fn honors_cancel_flag() {
        let path = std::env::temp_dir().join("omnivox_decode_test_cancel.wav");
        // Long enough to span several packets.
        write_sine_wav(&path, TARGET_SAMPLE_RATE, 1, 10.0, 440.0);

        let cancel = Arc::new(AtomicBool::new(true)); // pre-flipped
        let err = decode_file(&path, |_| {}, &cancel).unwrap_err();
        assert!(matches!(err, AppError::Cancelled));
        let _ = std::fs::remove_file(&path);
    }

    /// Write a simple PCM-16 WAV file with a sine wave.  Minimal RIFF
    /// implementation — enough for symphonia to decode.
    fn write_sine_wav(path: &std::path::Path, sr: u32, channels: u16, secs: f32, freq: f32) {
        let total_samples = (sr as f32 * secs) as u32;
        let bytes_per_sample = 2_u16;
        let block_align = channels * bytes_per_sample;
        let byte_rate = sr * block_align as u32;
        let data_size = total_samples * block_align as u32;
        let chunk_size = 36 + data_size;

        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&chunk_size.to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16_u32.to_le_bytes()).unwrap(); // subchunk1 size
        f.write_all(&1_u16.to_le_bytes()).unwrap(); // PCM
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&sr.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&block_align.to_le_bytes()).unwrap();
        f.write_all(&16_u16.to_le_bytes()).unwrap(); // bits per sample
        f.write_all(b"data").unwrap();
        f.write_all(&data_size.to_le_bytes()).unwrap();

        for i in 0..total_samples {
            let t = i as f32 / sr as f32;
            let sample = (t * freq * std::f32::consts::TAU).sin();
            let pcm = (sample * 32_000.0) as i16;
            for _ in 0..channels {
                f.write_all(&pcm.to_le_bytes()).unwrap();
            }
        }
    }
}

//! Deterministic, redistributable ASR load fixtures.
//!
//! These waveforms are generated mathematically and contain no recorded voice,
//! licensed corpus material, or lexical content. They are intended to compare
//! inference cost and silence/hallucination behavior, not word error rate.

use crate::support::harness_sha256;

pub const SAMPLE_RATE: u32 = 16_000;

pub struct Fixture {
    pub id: String,
    pub description: &'static str,
    pub duration_ms: u64,
    pub pcm_sha256: String,
    pub samples: Vec<f32>,
}

pub fn fixtures() -> Vec<Fixture> {
    [1u32, 5, 10, 15]
        .into_iter()
        .map(synthetic_voice_like_fixture)
        .collect()
}

/// A distinct 3-second fixture reserved for unmeasured warmups.
pub fn warmup_fixture() -> Fixture {
    synthetic_voice_like_fixture(3)
}

fn synthetic_voice_like_fixture(seconds: u32) -> Fixture {
    let sample_count = seconds as usize * SAMPLE_RATE as usize;
    let mut samples = Vec::with_capacity(sample_count);
    let mut noise_state = 0x4f4d_4e49u32 ^ seconds;
    for index in 0..sample_count {
        let time = index as f32 / SAMPLE_RATE as f32;
        let phrase_position = time % 1.2;
        let syllable_index = ((time / 0.3).floor() as usize) % 8;
        let voiced_position = phrase_position % 0.3;
        let voiced = voiced_position < 0.22 && phrase_position < 1.05;
        let envelope = if voiced {
            let attack = (voiced_position / 0.025).min(1.0);
            let release = ((0.22 - voiced_position) / 0.04).clamp(0.0, 1.0);
            attack * release
        } else {
            0.0
        };
        let fundamental = [118.0, 132.0, 146.0, 124.0, 174.0, 156.0, 138.0, 164.0][syllable_index];
        let phase = std::f32::consts::TAU * fundamental * time;
        let voiced_wave = phase.sin() * 0.16
            + (phase * 2.0).sin() * 0.07
            + (std::f32::consts::TAU * 710.0 * time).sin() * 0.045
            + (std::f32::consts::TAU * 1_220.0 * time).sin() * 0.025
            + (std::f32::consts::TAU * 2_450.0 * time).sin() * 0.012;

        noise_state = noise_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let noise = (((noise_state >> 8) as f32 / 16_777_215.0) * 2.0 - 1.0) * 0.0015;
        let edge_fade = (time / 0.02)
            .min((seconds as f32 - time) / 0.02)
            .clamp(0.0, 1.0);
        samples.push((voiced_wave * envelope + noise) * edge_fade);
    }

    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in &samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    Fixture {
        id: format!("synthetic_voice_like_{seconds}s_v1"),
        description: "mathematically generated voiced tones and pauses; no lexical content",
        duration_ms: seconds as u64 * 1_000,
        pcm_sha256: harness_sha256(&[pcm.as_slice()]),
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_deterministic_and_sized_for_whisper() {
        let first = fixtures();
        let second = fixtures();
        assert_eq!(first.len(), 4);
        let warmup = warmup_fixture();
        assert_eq!(warmup.duration_ms, 3_000);
        assert!(first
            .iter()
            .all(|fixture| fixture.pcm_sha256 != warmup.pcm_sha256));
        for (left, right) in first.iter().zip(second.iter()) {
            assert_eq!(left.pcm_sha256, right.pcm_sha256);
            assert_eq!(left.samples.len(), left.duration_ms as usize * 16);
            assert!(left.samples.iter().all(|sample| sample.abs() <= 1.0));
        }
    }
}

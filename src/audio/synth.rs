//! Audio oscillators + noise + silence + Karplus-Strong pluck.
//!
//! Every public `*_samples` function returns a normalised f32 buffer
//! interleaved across channels. [`render`] is the URI / filter
//! dispatcher; both transports consume the resulting [`AudioBuffer`]
//! directly (no intermediate container).

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use oxideav_core::{Error, Result};

use crate::source::{q_f64, q_str, q_u32};

/// `f32` samples produced by a synth, plus the rate/channels they came
/// out at.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}

/// Render the synth into an [`AudioBuffer`].
///
/// Both the URI path (which wraps the buffer in an
/// [`AudioFrame`](oxideav_core::AudioFrame) inside a
/// [`FrameSource`](oxideav_core::FrameSource)) and the zero-input
/// filter path consume this directly.
pub fn render(query: &BTreeMap<String, String>) -> Result<AudioBuffer> {
    let kind = q_str(query, "type", "sine");
    let sample_rate = q_u32(query, "rate", 8_000)?.max(1);
    let channels = q_u32(query, "channels", 1)?.clamp(1, 2) as u16;
    let duration_s = q_f64(query, "duration", 1.0)?.max(0.0);
    let amplitude = q_f64(query, "amplitude", 0.8)?.clamp(0.0, 1.0) as f32;
    let frame_count = (duration_s * sample_rate as f64).round() as usize;

    let mono: Vec<f32> = match kind {
        "sine" => sine(
            q_f64(query, "freq", 440.0)? as f32,
            sample_rate,
            frame_count,
            amplitude,
        ),
        "square" => square(
            q_f64(query, "freq", 440.0)? as f32,
            sample_rate,
            frame_count,
            amplitude,
        ),
        "triangle" => triangle(
            q_f64(query, "freq", 440.0)? as f32,
            sample_rate,
            frame_count,
            amplitude,
        ),
        "sawtooth" | "saw" => sawtooth(
            q_f64(query, "freq", 440.0)? as f32,
            sample_rate,
            frame_count,
            amplitude,
        ),
        "pluck" => karplus_strong(
            q_f64(query, "freq", 440.0)? as f32,
            q_f64(query, "decay", 0.996)? as f32,
            sample_rate,
            frame_count,
            amplitude,
        ),
        "noise" => {
            let color = q_str(query, "color", "white");
            let seed = q_u32(query, "seed", 0x12345678)?;
            match color {
                "white" => noise_white(frame_count, amplitude, seed),
                "pink" => noise_pink(frame_count, amplitude, seed),
                "brown" | "brownian" => noise_brown(frame_count, amplitude, seed),
                other => {
                    return Err(Error::invalid(format!(
                        "synth: noise color {other:?} (expected white|pink|brown)"
                    )));
                }
            }
        }
        "silence" => vec![0.0; frame_count],
        other => {
            return Err(Error::invalid(format!(
                "synth: unknown type {other:?} (expected sine|square|triangle|sawtooth|pluck|noise|silence)"
            )));
        }
    };

    // Channel-replicate to interleaved layout.
    let samples = if channels == 1 {
        mono
    } else {
        let mut out = Vec::with_capacity(mono.len() * channels as usize);
        for s in &mono {
            for _ in 0..channels {
                out.push(*s);
            }
        }
        out
    };

    Ok(AudioBuffer {
        samples,
        channels,
        sample_rate,
    })
}

/// Sine oscillator at `freq` Hz.
pub fn sine(freq: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    let dt = 1.0 / sample_rate as f32;
    (0..n)
        .map(|i| amplitude * (TAU * freq * i as f32 * dt).sin())
        .collect()
}

/// 50%-duty square wave at `freq` Hz.
pub fn square(freq: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    let period_samples = (sample_rate as f32) / freq;
    (0..n)
        .map(|i| {
            let phase = (i as f32 % period_samples) / period_samples;
            if phase < 0.5 {
                amplitude
            } else {
                -amplitude
            }
        })
        .collect()
}

/// Triangle wave at `freq` Hz.
pub fn triangle(freq: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    let period_samples = (sample_rate as f32) / freq;
    (0..n)
        .map(|i| {
            let phase = (i as f32 % period_samples) / period_samples; // 0..1
                                                                      // 0 → 0, 0.25 → 1, 0.5 → 0, 0.75 → -1, 1 → 0
            let v = if phase < 0.25 {
                phase * 4.0
            } else if phase < 0.75 {
                2.0 - phase * 4.0
            } else {
                phase * 4.0 - 4.0
            };
            amplitude * v
        })
        .collect()
}

/// Sawtooth at `freq` Hz. Phase 0 → -1, phase ~1 → +1.
pub fn sawtooth(freq: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    let period_samples = (sample_rate as f32) / freq;
    (0..n)
        .map(|i| {
            let phase = (i as f32 % period_samples) / period_samples; // 0..1
            amplitude * (2.0 * phase - 1.0)
        })
        .collect()
}

/// Karplus-Strong pluck: noise burst feeding a 1-sample averaging
/// delay line tuned to `freq`.
pub fn karplus_strong(
    freq: f32,
    decay: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Vec<f32> {
    let mut delay_len = ((sample_rate as f32) / freq).round() as usize;
    if delay_len < 2 {
        delay_len = 2;
    }
    let mut buf = Vec::with_capacity(delay_len);
    let mut rng = Lcg::new(0xC0FF_EE42);
    for _ in 0..delay_len {
        buf.push((rng.next_f32() * 2.0 - 1.0) * amplitude);
    }
    let mut out = Vec::with_capacity(n);
    let mut idx = 0;
    for _ in 0..n {
        let next_idx = (idx + 1) % delay_len;
        let avg = 0.5 * (buf[idx] + buf[next_idx]);
        let s = avg * decay;
        out.push(buf[idx]);
        buf[idx] = s;
        idx = next_idx;
    }
    out
}

/// White noise — uniform `[-amplitude, amplitude]`.
pub fn noise_white(n: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    (0..n)
        .map(|_| (rng.next_f32() * 2.0 - 1.0) * amplitude)
        .collect()
}

/// Pink noise via Voss-McCartney's 7-row trick (close-to-1/f
/// approximation, perceptually flat).
pub fn noise_pink(n: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    let mut rows = [0.0f32; 7];
    let mut out = Vec::with_capacity(n);
    let mut counter: u32 = 0;
    for _ in 0..n {
        counter = counter.wrapping_add(1);
        let row = counter.trailing_zeros().min(6) as usize;
        rows[row] = rng.next_f32() * 2.0 - 1.0;
        let sum: f32 = rows.iter().sum();
        out.push((sum / 7.0) * amplitude);
    }
    out
}

/// Brown / Brownian noise — running integral of white, normalised so
/// the running max stays in `[-1, 1]`.
pub fn noise_brown(n: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    let mut acc: f32 = 0.0;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let white = rng.next_f32() * 2.0 - 1.0;
        acc = (acc + white * 0.02).clamp(-1.0, 1.0);
        out.push(acc * amplitude);
    }
    out
}

/// Tiny Lcg so we don't need a `rand` dep.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u32) -> Self {
        Self {
            state: (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xDEAD_BEEF_CAFE_F00D,
        }
    }
    fn next_u32(&mut self) -> u32 {
        // Numerical Recipes constants.
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn sine_period_matches_freq() {
        // 1 Hz at 1000 Hz sample rate → 1000 samples per period; the
        // signal at sample 0 is 0 and crosses zero again at sample 500.
        let s = sine(1.0, 1000, 1000, 1.0);
        assert!(s[0].abs() < 1e-3);
        assert!(s[500].abs() < 1e-3);
        assert!(s[250] > 0.99); // peak
        assert!(s[750] < -0.99); // trough
    }

    #[test]
    fn synth_dispatcher_default_sine() {
        let buf = render(&map(&[
            ("type", "sine"),
            ("freq", "1000"),
            ("duration", "0.001"),
        ]))
        .unwrap();
        // 8000 Hz × 0.001s = 8 samples
        assert_eq!(buf.samples.len(), 8);
        assert_eq!(buf.channels, 1);
        assert_eq!(buf.sample_rate, 8000);
    }

    #[test]
    fn synth_silence_is_all_zeros() {
        let buf = render(&map(&[("type", "silence"), ("duration", "0.01")])).unwrap();
        assert_eq!(buf.samples.len(), 80);
        assert!(buf.samples.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn stereo_replicates_mono() {
        let buf = render(&map(&[
            ("type", "sine"),
            ("freq", "1000"),
            ("duration", "0.001"),
            ("channels", "2"),
        ]))
        .unwrap();
        // 8 samples × 2 channels = 16.
        assert_eq!(buf.samples.len(), 16);
        // Adjacent pairs should be equal (mono replication).
        for i in 0..8 {
            assert_eq!(buf.samples[i * 2], buf.samples[i * 2 + 1]);
        }
    }

    #[test]
    fn unknown_synth_type_errors() {
        let err = render(&map(&[("type", "fart")])).unwrap_err();
        assert!(format!("{err}").contains("fart"));
    }

    #[test]
    fn unknown_noise_color_errors() {
        let err = render(&map(&[("type", "noise"), ("color", "purple")])).unwrap_err();
        assert!(format!("{err}").contains("purple"));
    }
}

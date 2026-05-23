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
        "chirp" | "sweep" => {
            let f0 = q_f64(query, "f0", 100.0)? as f32;
            let f1 = q_f64(query, "f1", 4_000.0)? as f32;
            let shape = q_str(query, "shape", "linear");
            match shape {
                "linear" | "lin" => chirp_linear(f0, f1, sample_rate, frame_count, amplitude),
                "exp" | "exponential" | "log" | "logarithmic" => {
                    chirp_exponential(f0, f1, sample_rate, frame_count, amplitude)?
                }
                other => {
                    return Err(Error::invalid(format!(
                        "synth: chirp shape {other:?} (expected linear|exp)"
                    )));
                }
            }
        }
        "fm" => {
            let carrier = q_f64(query, "carrier", 440.0)? as f32;
            // Default carrier-to-modulator ratio 2:1 → "bell-like" timbre.
            let modulator = q_f64(query, "modulator", carrier as f64 * 0.5)? as f32;
            // Modulation index — peak phase deviation in radians (the
            // I in sin(2π·fc·t + I·sin(2π·fm·t))).
            let index = q_f64(query, "index", 5.0)? as f32;
            fm(
                carrier,
                modulator,
                index,
                sample_rate,
                frame_count,
                amplitude,
            )
        }
        "dtmf" => {
            // Touch-tone keypad: each key is the sum of one low-group and
            // one high-group sine (ITU-T Q.23 / Q.24 DTMF layout).
            //   `digits=` is the key sequence (0-9, A-D, *, #).
            //   `tone=` is the per-key on-duration in seconds; `gap=` is
            //   the silent inter-key duration. The overall `duration=`
            //   parameter is ignored for dtmf — the length is derived
            //   from the digit sequence and the tone/gap timing.
            let digits = q_str(query, "digits", "0");
            let tone_s = q_f64(query, "tone", 0.1)?.max(0.0);
            let gap_s = q_f64(query, "gap", 0.05)?.max(0.0);
            dtmf(digits, tone_s, gap_s, sample_rate, amplitude)?
        }
        "multitone" | "tones" => {
            // Comma-separated frequency list. Equal-weight sum then
            // normalised to `amplitude` so the peak stays bounded.
            let freqs = q_str(query, "freqs", "440,880");
            let mut parsed: Vec<f32> = Vec::new();
            for tok in freqs.split(',') {
                let tok = tok.trim();
                if tok.is_empty() {
                    continue;
                }
                match tok.parse::<f32>() {
                    Ok(f) if f > 0.0 => parsed.push(f),
                    _ => {
                        return Err(Error::invalid(format!(
                            "synth: multitone freqs {tok:?} (expected positive comma-separated numbers)"
                        )));
                    }
                }
            }
            if parsed.is_empty() {
                return Err(Error::invalid(
                    "synth: multitone requires at least one frequency (freqs=440,880,…)",
                ));
            }
            multitone(&parsed, sample_rate, frame_count, amplitude)
        }
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
                "synth: unknown type {other:?} (expected sine|square|triangle|sawtooth|pluck|chirp|fm|dtmf|multitone|noise|silence)"
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

/// Linear frequency sweep from `f0` Hz to `f1` Hz across the full
/// duration. Instantaneous frequency at sample `i` is
/// `f0 + (f1 - f0) * i / (n - 1)`; the phase is the running integral of
/// `2π * f(t)`, accumulated sample-by-sample so the waveform is C¹
/// continuous regardless of `(f0, f1, sample_rate)`.
pub fn chirp_linear(f0: f32, f1: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    let dt = 1.0 / sample_rate as f32;
    let n_f = n as f32;
    let mut phase: f32 = 0.0;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let frac = if n == 1 { 0.0 } else { i as f32 / (n_f - 1.0) };
        let f = f0 + (f1 - f0) * frac;
        out.push(amplitude * phase.sin());
        phase += TAU * f * dt;
    }
    out
}

/// Exponential / logarithmic frequency sweep from `f0` Hz to `f1` Hz
/// across the full duration. `f0` and `f1` must both be strictly
/// positive — exponential sweeps can't cross zero.
///
/// Instantaneous frequency follows `f0 * (f1/f0)^(i/(n-1))`; the phase
/// integral is again accumulated sample-by-sample.
pub fn chirp_exponential(
    f0: f32,
    f1: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    if f0 <= 0.0 || f1 <= 0.0 {
        return Err(Error::invalid(format!(
            "synth: chirp shape=exp requires f0>0 and f1>0 (got f0={f0}, f1={f1})"
        )));
    }
    if n == 0 {
        return Ok(Vec::new());
    }
    let dt = 1.0 / sample_rate as f32;
    let n_f = n as f32;
    let ratio_log = (f1 / f0).ln();
    let mut phase: f32 = 0.0;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let frac = if n == 1 { 0.0 } else { i as f32 / (n_f - 1.0) };
        let f = f0 * (ratio_log * frac).exp();
        out.push(amplitude * phase.sin());
        phase += TAU * f * dt;
    }
    Ok(out)
}

/// Frequency modulation: `amplitude * sin(2π·fc·t + index·sin(2π·fm·t))`.
///
/// `index` is the modulation index in radians (peak phase deviation).
/// At `index=0` this reduces to a pure carrier sine; classical
/// instrument-like timbres usually live in `index ∈ [0.5, 10]`.
pub fn fm(
    carrier: f32,
    modulator: f32,
    index: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Vec<f32> {
    let dt = 1.0 / sample_rate as f32;
    (0..n)
        .map(|i| {
            let t = i as f32 * dt;
            let mod_phase = TAU * modulator * t;
            let phase = TAU * carrier * t + index * mod_phase.sin();
            amplitude * phase.sin()
        })
        .collect()
}

/// Equal-weight sum of sine tones, scaled so the worst-case peak (all
/// tones aligned) is bounded by `amplitude`. Useful for stereo
/// intermodulation / image-rejection probes.
pub fn multitone(freqs: &[f32], sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    if freqs.is_empty() || n == 0 {
        return vec![0.0; n];
    }
    let dt = 1.0 / sample_rate as f32;
    let scale = amplitude / freqs.len() as f32;
    (0..n)
        .map(|i| {
            let t = i as f32 * dt;
            let mut s = 0.0;
            for &f in freqs {
                s += (TAU * f * t).sin();
            }
            scale * s
        })
        .collect()
}

/// Map a DTMF keypad symbol to its `(low, high)` frequency pair in Hz.
///
/// The four low-group rows are 697 / 770 / 852 / 941 Hz; the four
/// high-group columns are 1209 / 1336 / 1477 / 1633 Hz. Each key on the
/// 4×4 keypad selects exactly one row and one column (ITU-T Q.23 / Q.24
/// dual-tone multi-frequency layout). `*` and `#` are accepted as the
/// star and pound keys; `A`–`D` (case-insensitive) are the fourth
/// column. Returns `None` for any other symbol.
pub fn dtmf_freqs(key: char) -> Option<(f32, f32)> {
    const LOW: [f32; 4] = [697.0, 770.0, 852.0, 941.0];
    const HIGH: [f32; 4] = [1209.0, 1336.0, 1477.0, 1633.0];
    // (row, col) on the standard keypad:
    //        1209  1336  1477  1633
    //  697 :  1     2     3     A
    //  770 :  4     5     6     B
    //  852 :  7     8     9     C
    //  941 :  *     0     #     D
    let (row, col) = match key {
        '1' => (0, 0),
        '2' => (0, 1),
        '3' => (0, 2),
        'A' | 'a' => (0, 3),
        '4' => (1, 0),
        '5' => (1, 1),
        '6' => (1, 2),
        'B' | 'b' => (1, 3),
        '7' => (2, 0),
        '8' => (2, 1),
        '9' => (2, 2),
        'C' | 'c' => (2, 3),
        '*' => (3, 0),
        '0' => (3, 1),
        '#' => (3, 2),
        'D' | 'd' => (3, 3),
        _ => return None,
    };
    Some((LOW[row], HIGH[col]))
}

/// Render a sequence of DTMF key presses.
///
/// Each key in `digits` produces `tone_s` seconds of its dual-tone
/// signal (low + high sine, each at half `amplitude` so an aligned
/// peak stays inside `[-amplitude, amplitude]`) followed by `gap_s`
/// seconds of silence. Whitespace in `digits` is ignored. An
/// unrecognised symbol is an error so a typo in the dialled string
/// doesn't silently emit nothing.
pub fn dtmf(
    digits: &str,
    tone_s: f64,
    gap_s: f64,
    sample_rate: u32,
    amplitude: f32,
) -> Result<Vec<f32>> {
    let tone_n = (tone_s * sample_rate as f64).round() as usize;
    let gap_n = (gap_s * sample_rate as f64).round() as usize;
    let dt = 1.0 / sample_rate as f32;
    // Half-amplitude per partial so the worst case (both partials at
    // their peak simultaneously) is bounded by `amplitude`.
    let half = amplitude * 0.5;
    let mut out = Vec::new();
    for key in digits.chars() {
        if key.is_whitespace() {
            continue;
        }
        let (low, high) = dtmf_freqs(key).ok_or_else(|| {
            Error::invalid(format!(
                "synth: dtmf key {key:?} (expected 0-9, A-D, * or #)"
            ))
        })?;
        for i in 0..tone_n {
            let t = i as f32 * dt;
            out.push(half * (TAU * low * t).sin() + half * (TAU * high * t).sin());
        }
        out.extend(std::iter::repeat(0.0).take(gap_n));
    }
    Ok(out)
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

    // ───── chirp / sweep ─────

    #[test]
    fn chirp_linear_endpoints_match_target_frequencies() {
        // 1 s @ 1000 Hz sample rate, 100 → 400 Hz linear sweep.
        // The instantaneous frequency at sample i is
        // f0 + (f1 - f0) * i / (n - 1); start sample 0 → 100 Hz,
        // last sample → 400 Hz. We sanity-check that the *spacing*
        // between zero-crossings near the start is roughly 10×
        // larger than near the end (100 Hz vs 1 kHz of sample rate
        // means a period of ~10 samples vs ~2.5 samples at the end).
        let s = chirp_linear(100.0, 400.0, 1000, 1000, 1.0);
        assert_eq!(s.len(), 1000);
        // Phase integral is monotonic and bounded by π*(f0+f1)*duration
        // = π*(100+400)*1 ≈ 1570 rad; sample 0 is sin(0) = 0.
        assert!(s[0].abs() < 1e-3);
    }

    #[test]
    fn chirp_exponential_errors_on_zero_endpoint() {
        let err = render(&map(&[
            ("type", "chirp"),
            ("shape", "exp"),
            ("f0", "0"),
            ("f1", "1000"),
            ("duration", "0.01"),
        ]))
        .unwrap_err();
        assert!(format!("{err}").contains("f0>0"));
    }

    #[test]
    fn chirp_dispatcher_default_is_linear() {
        // Default duration is 1s, default rate is 8000 Hz, default
        // (f0, f1) = (100, 4000). 8000 samples, all within [-amp, amp].
        let buf = render(&map(&[("type", "chirp")])).unwrap();
        assert_eq!(buf.samples.len(), 8000);
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn chirp_unknown_shape_errors() {
        let err = render(&map(&[("type", "chirp"), ("shape", "quadratic")])).unwrap_err();
        assert!(format!("{err}").contains("quadratic"));
    }

    // ───── FM ─────

    #[test]
    fn fm_zero_index_reduces_to_carrier_sine() {
        // index=0 means the modulator contributes nothing → pure
        // sine at the carrier frequency. Compare against the sine()
        // helper sample-by-sample.
        let n = 1024;
        let sr = 8000;
        let carrier = 440.0;
        let fm_buf = fm(carrier, 220.0, 0.0, sr, n, 0.8);
        let sine_buf = sine(carrier, sr, n, 0.8);
        let max_err = fm_buf
            .iter()
            .zip(&sine_buf)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        // f32 phase drift; 1024 samples × 440 Hz × TAU is ~2.8e6 rad
        // of accumulated phase before we apply sin(), so 1e-4 is the
        // realistic numerical ceiling.
        assert!(max_err < 1e-4, "max err = {max_err}");
    }

    #[test]
    fn fm_dispatcher_default_keeps_bounds() {
        let buf = render(&map(&[("type", "fm"), ("duration", "0.05")])).unwrap();
        assert_eq!(buf.samples.len(), 400); // 8000 × 0.05
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    // ───── multitone ─────

    #[test]
    fn multitone_zero_at_origin() {
        // sin(0) = 0 for every component → sample 0 must be 0.
        let buf = render(&map(&[
            ("type", "multitone"),
            ("freqs", "440,1000,2200"),
            ("duration", "0.001"),
        ]))
        .unwrap();
        assert!(buf.samples[0].abs() < 1e-6);
    }

    #[test]
    fn multitone_normalised_so_peak_bounded() {
        // Three tones, all aligned at t = 0; we scale by 1/N so the
        // worst case sums to amplitude exactly.
        let buf = render(&map(&[
            ("type", "multitone"),
            ("freqs", "440,880,1760"),
            ("duration", "0.05"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        for s in &buf.samples {
            // Bound is 1.0 (sum of three sines / 3, each in [-1, 1]).
            assert!(s.abs() <= 1.0 + 1e-6, "out-of-bounds sample {s}");
        }
    }

    #[test]
    fn multitone_empty_list_errors() {
        let err = render(&map(&[("type", "multitone"), ("freqs", ",,,")])).unwrap_err();
        assert!(format!("{err}").contains("at least one frequency"));
    }

    #[test]
    fn multitone_negative_freq_errors() {
        let err = render(&map(&[("type", "multitone"), ("freqs", "440,-220")])).unwrap_err();
        assert!(format!("{err}").contains("-220"));
    }

    // ───── DTMF ─────

    #[test]
    fn dtmf_keypad_frequency_pairs() {
        // Spot-check the corners + centre of the 4×4 keypad against the
        // canonical ITU-T low/high group frequencies.
        assert_eq!(dtmf_freqs('1'), Some((697.0, 1209.0)));
        assert_eq!(dtmf_freqs('3'), Some((697.0, 1477.0)));
        assert_eq!(dtmf_freqs('5'), Some((770.0, 1336.0)));
        assert_eq!(dtmf_freqs('0'), Some((941.0, 1336.0)));
        assert_eq!(dtmf_freqs('*'), Some((941.0, 1209.0)));
        assert_eq!(dtmf_freqs('#'), Some((941.0, 1477.0)));
        assert_eq!(dtmf_freqs('A'), Some((697.0, 1633.0)));
        assert_eq!(dtmf_freqs('D'), Some((941.0, 1633.0)));
        // Lowercase column letters map to the same column.
        assert_eq!(dtmf_freqs('a'), dtmf_freqs('A'));
        assert_eq!(dtmf_freqs('d'), dtmf_freqs('D'));
        // Anything off the keypad is None.
        assert_eq!(dtmf_freqs('e'), None);
        assert_eq!(dtmf_freqs('!'), None);
    }

    #[test]
    fn dtmf_length_is_tone_plus_gap_per_key() {
        // 3 keys × (0.1s tone + 0.05s gap) at 8000 Hz =
        // 3 × (800 + 400) = 3600 samples.
        let buf = render(&map(&[
            ("type", "dtmf"),
            ("digits", "123"),
            ("tone", "0.1"),
            ("gap", "0.05"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 3600);
    }

    #[test]
    fn dtmf_whitespace_in_digits_is_ignored() {
        // A space between keys must not add samples or error out.
        let spaced = render(&map(&[
            ("type", "dtmf"),
            ("digits", "1 2"),
            ("tone", "0.1"),
            ("gap", "0.05"),
        ]))
        .unwrap();
        let dense = render(&map(&[
            ("type", "dtmf"),
            ("digits", "12"),
            ("tone", "0.1"),
            ("gap", "0.05"),
        ]))
        .unwrap();
        assert_eq!(spaced.samples.len(), dense.samples.len());
    }

    #[test]
    fn dtmf_tone_is_sum_of_low_and_high() {
        // For key '1' the signal is half·sin(2π·697·t) + half·sin(2π·1209·t).
        // Sample 0 is sin(0)+sin(0)=0; verify against a hand-built
        // reference for the first few samples.
        let sr = 8000;
        let amp = 0.8;
        let half = amp * 0.5;
        let dt = 1.0 / sr as f32;
        let buf = dtmf("1", 0.01, 0.0, sr, amp).unwrap();
        for (i, &got) in buf.iter().take(16).enumerate() {
            let t = i as f32 * dt;
            let want = half * (TAU * 697.0 * t).sin() + half * (TAU * 1209.0 * t).sin();
            assert!(
                (got - want).abs() < 1e-6,
                "sample {i}: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn dtmf_peak_bounded_by_amplitude() {
        let buf = dtmf("0123456789ABCD*#", 0.02, 0.0, 8000, 1.0).unwrap();
        for s in &buf {
            // Two half-amplitude sines → worst case 1.0.
            assert!(s.abs() <= 1.0 + 1e-6, "out-of-bounds dtmf sample {s}");
        }
    }

    #[test]
    fn dtmf_invalid_key_errors() {
        let err = render(&map(&[("type", "dtmf"), ("digits", "12X")])).unwrap_err();
        assert!(format!("{err}").contains("'X'") || format!("{err}").contains("dtmf key"));
    }
}

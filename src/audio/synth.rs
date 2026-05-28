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
        "formant" | "vowel" => {
            // Klatt-style two-formant vowel synthesizer (see Klatt, 1980,
            // "Software for a cascade/parallel formant synthesizer", JASA
            // 67(3):971-995, public reference — no source-reading of any
            // Klatt / Festival / espeak / mbrola / Praat implementation).
            //
            // Architecture: a glottal-pulse train at the fundamental f0
            // drives two parallel 2-pole resonators tuned to the formant
            // centre frequencies (F1, F2). The two resonator outputs are
            // summed and re-normalised to keep the peak bounded.
            //
            // The vowel selector maps to textbook-standard rounded
            // formant centres for an adult male speaker, in line with
            // the average values reported by Peterson & Barney's 1952
            // acoustical study (which has been reproduced in essentially
            // every introductory phonetics textbook since):
            //
            //   vowel  F1 (Hz)  F2 (Hz)   example
            //     A      730      1090     "father"  /ɑ/
            //     E      530      1840     "bed"     /ɛ/
            //     I      270      2290     "beet"    /i/
            //     O      570      840      "bought"  /ɔ/
            //     U      300      870      "boot"    /u/
            //
            // `vowel=` selects the (F1, F2) pair; `f0=` is the
            // fundamental (pitch); `bw=` is the per-formant bandwidth in
            // Hz (default 80, a textbook-typical value); `duration=` is
            // the note length in seconds.
            let vowel = q_str(query, "vowel", "A");
            let f0 = q_f64(query, "f0", 220.0)? as f32;
            let bw = q_f64(query, "bw", 80.0)? as f32;
            let (f1, f2) = vowel_formants(vowel)?;
            formant(f0, f1, f2, bw, sample_rate, frame_count, amplitude)
        }
        "ringmod" | "ring" => {
            // Classical analogue ring modulation:
            // amplitude * sin(2π·f1·t) * sin(2π·f2·t)
            //
            // The product of two sines is the prosthaphaeresis identity
            //   sin(α) · sin(β) = ½·[cos(α − β) − cos(α + β)],
            // so the output is the sum and difference frequencies
            // (f1 ± f2) at half amplitude each. The carrier is fully
            // suppressed (no f1 or f2 component) — this is what makes
            // ring-modulation distinct from amplitude modulation
            // (1 + m·sin(fm·t))·sin(fc·t), which keeps the carrier.
            let f1 = q_f64(query, "f1", 440.0)? as f32;
            let f2 = q_f64(query, "f2", 60.0)? as f32;
            ringmod(f1, f2, sample_rate, frame_count, amplitude)
        }
        "am" => {
            // Classical analogue amplitude modulation:
            //   amplitude · 0.5 · (1 + m·sin(2π·fm·t)) · sin(2π·fc·t)
            //
            // Distinguishing feature versus ring modulation: AM keeps
            // the carrier. By the prosthaphaeresis identity the
            // expanded form is
            //   0.5·sin(2π·fc·t)
            //   + 0.25·m·[cos(2π·(fc−fm)·t) − cos(2π·(fc+fm)·t)],
            // i.e. an unsuppressed carrier at fc plus sidebands at
            // fc ± fm. `index=` is the modulation index m in [0, 1]
            // (100 % modulation at m=1, pure carrier at m=0). The
            // leading 0.5 keeps the worst-case envelope `(1 + m)·1` at
            // m=1 inside `[-amplitude, amplitude]`.
            let carrier = q_f64(query, "carrier", 440.0)? as f32;
            // Default carrier:modulator ratio 2:1 (modulator is half
            // the carrier) — matches the FM default and produces a
            // textbook AM example.
            let modulator = q_f64(query, "modulator", carrier as f64 * 0.5)? as f32;
            // Modulation index in [0, 1]. Out-of-range values clamp
            // (negative would invert the phase of the modulator only,
            // and >1 over-modulates which the bounded form here
            // explicitly avoids).
            let index = (q_f64(query, "index", 0.5)? as f32).clamp(0.0, 1.0);
            am(
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
        "adsr" => {
            // Attack-Decay-Sustain-Release amplitude envelope applied to a
            // base oscillator. `wave=` picks the carrier (sine default);
            // `attack` / `decay` / `release` are durations in seconds and
            // `sustain` is the hold level in [0, 1]. The release tail is
            // taken from the end of the configured `duration=`; the sustain
            // phase fills whatever is left between decay and release.
            let freq = q_f64(query, "freq", 440.0)? as f32;
            let wave = q_str(query, "wave", "sine");
            let attack_s = q_f64(query, "attack", 0.01)?.max(0.0);
            let decay_s = q_f64(query, "decay", 0.1)?.max(0.0);
            let sustain = q_f64(query, "sustain", 0.7)?.clamp(0.0, 1.0) as f32;
            let release_s = q_f64(query, "release", 0.2)?.max(0.0);
            adsr(
                freq,
                wave,
                attack_s,
                decay_s,
                sustain,
                release_s,
                sample_rate,
                frame_count,
                amplitude,
            )?
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
                "synth: unknown type {other:?} (expected sine|square|triangle|sawtooth|pluck|chirp|fm|am|ringmod|dtmf|adsr|formant|multitone|noise|silence)"
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

/// Ring modulation: `amplitude · sin(2π·f1·t) · sin(2π·f2·t)`.
///
/// The product of two sines is, by the prosthaphaeresis identity,
///
/// ```text
/// sin(α) · sin(β) = ½·[cos(α − β) − cos(α + β)],
/// ```
///
/// so the spectrum consists of just the sum and difference tones
/// `f1 ± f2`, each at half amplitude. The carrier components at `f1`
/// and `f2` are entirely suppressed — that is the distinguishing
/// feature of ring modulation versus amplitude modulation, which keeps
/// the carrier.
///
/// Worst case `|sin(α)| · |sin(β)| = 1`, so the output stays bounded
/// by `amplitude` for every `(f1, f2)` and every sample rate.
pub fn ringmod(f1: f32, f2: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    let dt = 1.0 / sample_rate as f32;
    (0..n)
        .map(|i| {
            let t = i as f32 * dt;
            let a = (TAU * f1 * t).sin();
            let b = (TAU * f2 * t).sin();
            amplitude * a * b
        })
        .collect()
}

/// Classical analogue amplitude modulation:
///
/// ```text
/// amplitude · 0.5 · (1 + m·sin(2π·fm·t)) · sin(2π·fc·t)
/// ```
///
/// Expanded via the prosthaphaeresis identity, the spectrum is
///
/// ```text
/// 0.5·sin(2π·fc·t)
///   + 0.25·m·[cos(2π·(fc − fm)·t) − cos(2π·(fc + fm)·t)],
/// ```
///
/// i.e. an unsuppressed carrier at `fc` plus two sidebands at `fc ± fm`
/// — this is exactly what distinguishes AM from ring modulation, which
/// suppresses the carrier entirely. `index` is the modulation index
/// `m ∈ [0, 1]` (100 % modulation at `m=1`, pure carrier at `m=0`); it
/// is clamped into that range by the caller. The leading `0.5` keeps
/// the worst-case envelope `(1 + m)·1 ≤ 2` at `m=1` inside
/// `[-amplitude, amplitude]`.
pub fn am(
    carrier: f32,
    modulator: f32,
    index: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Vec<f32> {
    let dt = 1.0 / sample_rate as f32;
    let half = 0.5 * amplitude;
    (0..n)
        .map(|i| {
            let t = i as f32 * dt;
            let envelope = 1.0 + index * (TAU * modulator * t).sin();
            half * envelope * (TAU * carrier * t).sin()
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

/// Sample `i` of a piecewise-linear ADSR amplitude envelope, in `[0, 1]`.
///
/// The envelope has four contiguous segments measured from sample 0:
///
/// * **Attack** — linear ramp `0 → 1` over the first `attack_n` samples.
/// * **Decay** — linear ramp `1 → sustain` over the next `decay_n`.
/// * **Sustain** — held flat at `sustain` until the release begins.
/// * **Release** — linear ramp `sustain → 0` over the final `release_n`
///   samples (the tail of the note), so the envelope reaches exactly 0
///   at sample `n`.
///
/// When the note is too short to fit attack + decay + release, the
/// release window is clamped to start no earlier than the end of decay
/// (the sustain segment shrinks to zero first, then decay is allowed to
/// overlap the release as a shortened note); the value is still computed
/// as the linear interpolation within whichever segment `i` lands in, so
/// the envelope stays continuous and bounded in `[0, 1]`.
fn adsr_envelope(
    i: usize,
    n: usize,
    attack_n: usize,
    decay_n: usize,
    sustain: f32,
    release_n: usize,
) -> f32 {
    if n == 0 {
        return 0.0;
    }
    // Release occupies the last `release_n` samples of the note. Its
    // start index is clamped so it never begins before the attack ends.
    let release_start = n.saturating_sub(release_n).max(attack_n.min(n));
    let decay_end = (attack_n + decay_n).min(release_start);

    if i < attack_n {
        // Attack: 0 → 1.
        (i as f32 + 1.0) / attack_n as f32
    } else if i < decay_end {
        // Decay: 1 → sustain.
        let frac = (i - attack_n) as f32 / decay_n as f32;
        1.0 + (sustain - 1.0) * frac
    } else if i < release_start {
        // Sustain: flat hold.
        sustain
    } else {
        // Release: sustain → 0 over the tail. The level entering the
        // release is whatever the prior segment left us at — `sustain`
        // once decay has completed, which is the common case.
        let tail = n - release_start;
        if tail == 0 {
            0.0
        } else {
            let frac = (i - release_start) as f32 / tail as f32;
            sustain * (1.0 - frac).max(0.0)
        }
    }
}

/// ADSR-enveloped tone: a base oscillator (`wave`) scaled sample-by-sample
/// by a piecewise-linear [`adsr_envelope`].
///
/// `attack_s` / `decay_s` / `release_s` are segment durations in seconds
/// and `sustain` is the hold level in `[0, 1]`; the sustain segment fills
/// whatever time is left between the decay and the release tail. The
/// carrier amplitude is the full `amplitude` — the envelope does the
/// shaping — so the output stays bounded by `amplitude`.
#[allow(clippy::too_many_arguments)]
pub fn adsr(
    freq: f32,
    wave: &str,
    attack_s: f64,
    decay_s: f64,
    sustain: f32,
    release_s: f64,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    // Base oscillator at full amplitude; the envelope shapes it.
    let base = match wave {
        "sine" => sine(freq, sample_rate, n, amplitude),
        "square" => square(freq, sample_rate, n, amplitude),
        "triangle" => triangle(freq, sample_rate, n, amplitude),
        "sawtooth" | "saw" => sawtooth(freq, sample_rate, n, amplitude),
        other => {
            return Err(Error::invalid(format!(
                "synth: adsr wave {other:?} (expected sine|square|triangle|sawtooth)"
            )));
        }
    };
    let attack_n = (attack_s * sample_rate as f64).round() as usize;
    let decay_n = (decay_s * sample_rate as f64).round() as usize;
    let release_n = (release_s * sample_rate as f64).round() as usize;
    let out = base
        .iter()
        .enumerate()
        .map(|(i, &s)| s * adsr_envelope(i, n, attack_n, decay_n, sustain, release_n))
        .collect();
    Ok(out)
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

/// Map a single-letter vowel selector (`A`/`E`/`I`/`O`/`U`,
/// case-insensitive) to its `(F1, F2)` centre-frequency pair in Hz.
///
/// The values are textbook-standard rounded formant centres for an
/// adult male speaker, in line with averages from Peterson & Barney's
/// 1952 acoustical study of the vowels (reproduced in virtually every
/// introductory phonetics textbook since):
///
/// | Vowel | F1 (Hz) | F2 (Hz) | Example          |
/// |-------|---------|---------|------------------|
/// | `A`   | 730     | 1090    | "father" (/ɑ/)   |
/// | `E`   | 530     | 1840    | "bed"    (/ɛ/)   |
/// | `I`   | 270     | 2290    | "beet"   (/i/)   |
/// | `O`   | 570     | 840     | "bought" (/ɔ/)   |
/// | `U`   | 300     | 870     | "boot"   (/u/)   |
///
/// Returns an error for any other selector so a typo doesn't silently
/// pick a default vowel.
pub fn vowel_formants(vowel: &str) -> Result<(f32, f32)> {
    let v = vowel.trim();
    if v.len() != 1 {
        return Err(Error::invalid(format!(
            "synth: vowel {vowel:?} (expected single letter A|E|I|O|U)"
        )));
    }
    let c = v.chars().next().unwrap().to_ascii_uppercase();
    let pair = match c {
        'A' => (730.0, 1090.0),
        'E' => (530.0, 1840.0),
        'I' => (270.0, 2290.0),
        'O' => (570.0, 840.0),
        'U' => (300.0, 870.0),
        _ => {
            return Err(Error::invalid(format!(
                "synth: vowel {vowel:?} (expected A|E|I|O|U)"
            )));
        }
    };
    Ok(pair)
}

/// Klatt-style two-formant vowel synthesiser.
///
/// Architecture (after Klatt 1980, JASA 67(3):971-995 — public-reference
/// citation only, no source-reading of any Klatt implementation):
///
/// 1. **Glottal source.** A periodic impulse train at `f0`. Each glottal
///    period contributes a single non-zero sample of `+1`; all other
///    samples are zero. This is the simplest periodic excitation with
///    the correct line spectrum at integer multiples of `f0`, and it
///    keeps the source spectrally flat so the formant filters fully
///    determine the resonance peaks. A one-zero low-pass `(x[n] +
///    x[n-1]) / 2` lightly shapes the pulse so the upper harmonics roll
///    off gently (Klatt's "shaped pulse" precondition).
/// 2. **Two parallel resonators.** Each resonator is a 2-pole filter
///    `y[n] = b·x[n] + 2·r·cos(ω)·y[n-1] − r²·y[n-2]`, with pole radius
///    `r = exp(−π·BW/Fs)` and pole angle `ω = 2π·F/Fs`. The transfer
///    function has a sharp magnitude peak at the formant frequency `F`
///    with a −3 dB bandwidth of `BW` Hz. The input gain `b = 1 − r²` is
///    Klatt's normalisation that holds the peak-frequency magnitude at
///    unity across `(F, BW, Fs)`, so the post-sum scaling is bounded.
/// 3. **Sum + normalise.** The two resonator outputs are summed and
///    re-scaled to `amplitude`. The pre-scale peak is bounded by the
///    sum of two unity-peak resonances, so we divide by the empirical
///    running peak (with a 1e-3 floor for the all-zero edge case) so
///    the final samples sit safely inside `[-amplitude, amplitude]`.
pub fn formant(
    f0: f32,
    f1: f32,
    f2: f32,
    bw: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Vec<f32> {
    if n == 0 || sample_rate == 0 {
        return vec![0.0; n];
    }
    let sr = sample_rate as f32;

    // Glottal-pulse train: an impulse every `period` samples. Floating
    // accumulator so non-integer periods (the common case) don't drift.
    let period = sr / f0.max(1.0);
    let mut pulses = vec![0.0f32; n];
    let mut next = 0.0f32;
    let mut i = 0usize;
    while (i as f32) < n as f32 {
        let idx = next.round() as usize;
        if idx >= n {
            break;
        }
        pulses[idx] = 1.0;
        next += period;
        i = idx + 1;
    }
    // One-zero low-pass: y[n] = 0.5·(x[n] + x[n-1]); softens the
    // impulses' upper harmonics so the resonator peaks dominate.
    let mut shaped = vec![0.0f32; n];
    let mut prev = 0.0f32;
    for (i, &p) in pulses.iter().enumerate() {
        shaped[i] = 0.5 * (p + prev);
        prev = p;
    }

    // Two parallel resonators.
    let r1 = resonator(&shaped, f1, bw, sr);
    let r2 = resonator(&shaped, f2, bw, sr);

    // Sum, find peak, normalise to `amplitude`.
    let mut out = Vec::with_capacity(n);
    let mut peak = 0.0f32;
    for i in 0..n {
        let v = r1[i] + r2[i];
        if v.abs() > peak {
            peak = v.abs();
        }
        out.push(v);
    }
    let scale = if peak > 1e-3 { amplitude / peak } else { 0.0 };
    for s in out.iter_mut() {
        *s *= scale;
    }
    out
}

/// Single 2-pole resonator at `freq` Hz with `bw` Hz of bandwidth.
///
/// Difference equation
///
/// ```text
/// y[n] = (1 − r²)·x[n] + 2·r·cos(ω)·y[n−1] − r²·y[n−2]
/// ```
///
/// with `r = exp(−π·BW/Fs)` (pole radius) and `ω = 2π·F/Fs` (pole
/// angle). The `(1 − r²)` input gain is Klatt's normalisation that
/// holds the magnitude response at exactly 1 at the resonance peak —
/// the response then falls off either side at the standard 2-pole
/// rate, with the −3 dB points `BW` Hz apart.
fn resonator(x: &[f32], freq: f32, bw: f32, sr: f32) -> Vec<f32> {
    let r = (-std::f32::consts::PI * bw / sr).exp();
    let omega = TAU * freq / sr;
    let a1 = 2.0 * r * omega.cos();
    let a2 = -(r * r);
    let b0 = 1.0 - r * r;
    let mut y1 = 0.0f32;
    let mut y2 = 0.0f32;
    let mut out = Vec::with_capacity(x.len());
    for &s in x {
        let y = b0 * s + a1 * y1 + a2 * y2;
        out.push(y);
        y2 = y1;
        y1 = y;
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

    // ───── ring modulation ─────

    #[test]
    fn ringmod_matches_prosthaphaeresis_identity() {
        // sin(α) · sin(β) = ½·[cos(α − β) − cos(α + β)]. Build the RHS
        // sample-by-sample and compare against the LHS that ringmod()
        // produces.
        let sr = 8000;
        let n = 1024;
        let f1 = 440.0_f32;
        let f2 = 60.0_f32;
        let amp = 0.8_f32;
        let got = ringmod(f1, f2, sr, n, amp);
        let dt = 1.0 / sr as f32;
        for (i, &g) in got.iter().enumerate() {
            let t = i as f32 * dt;
            let want = 0.5 * amp * ((TAU * (f1 - f2) * t).cos() - (TAU * (f1 + f2) * t).cos());
            // f32 trig drift over ~1024 samples × 500 Hz × TAU of accumulated
            // phase puts the realistic floor for the LHS-vs-RHS comparison
            // around 1e-4 — well below the half-amplitude (0.4) signal level.
            assert!((g - want).abs() < 1e-4, "sample {i}: got {g}, want {want}");
        }
    }

    #[test]
    fn ringmod_starts_at_zero() {
        // sin(0)·sin(0) = 0 regardless of f1, f2, amplitude.
        let buf = render(&map(&[
            ("type", "ringmod"),
            ("f1", "440"),
            ("f2", "60"),
            ("duration", "0.05"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        assert!(buf.samples[0].abs() < 1e-6);
    }

    #[test]
    fn ringmod_output_bounded_by_amplitude() {
        // |sin(α)| · |sin(β)| ≤ 1 ⇒ output bounded by amplitude.
        let buf = render(&map(&[
            ("type", "ringmod"),
            ("f1", "440"),
            ("f2", "60"),
            ("duration", "0.2"),
            ("amplitude", "0.7"),
        ]))
        .unwrap();
        for s in &buf.samples {
            assert!(s.abs() <= 0.7 + 1e-6, "out-of-bounds ringmod sample {s}");
        }
    }

    #[test]
    fn ringmod_equal_frequencies_is_half_minus_half_cos_2f() {
        // f1 == f2 == f ⇒ sin²(2πft) = ½ − ½·cos(4πft). DC offset 0.5·amp,
        // and a single tone at 2f at amplitude amp/2. We just check the
        // mean over an integer number of (2f) periods is ≈ amp/2.
        let sr = 8000;
        let f = 200.0_f32;
        // 2f period at 8000 Hz = 8000 / 400 = 20 samples; pick 400 samples
        // = exactly 20 full periods of the 2f component.
        let n = 400;
        let amp = 1.0_f32;
        let out = ringmod(f, f, sr, n, amp);
        let mean: f32 = out.iter().copied().sum::<f32>() / n as f32;
        assert!(
            (mean - 0.5).abs() < 1e-3,
            "mean of sin²(2πft) over an integer number of periods should be 0.5, got {mean}"
        );
    }

    #[test]
    fn ringmod_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("ringmod"));
    }

    // ───── AM (amplitude modulation) ─────

    #[test]
    fn am_matches_prosthaphaeresis_expansion() {
        // 0.5·(1 + m·sin(2π·fm·t))·sin(2π·fc·t)
        //   = 0.5·sin(2π·fc·t)
        //     + 0.25·m·[cos(2π·(fc-fm)·t) - cos(2π·(fc+fm)·t)]
        // Verify the closed form sample-by-sample.
        let sr = 8000;
        let n = 1024;
        let fc = 440.0_f32;
        let fm_freq = 60.0_f32;
        let m = 0.5_f32;
        let amp = 0.8_f32;
        let got = am(fc, fm_freq, m, sr, n, amp);
        let dt = 1.0 / sr as f32;
        let half = 0.5 * amp;
        let quart = 0.25 * amp * m;
        for (i, &g) in got.iter().enumerate() {
            let t = i as f32 * dt;
            let want = half * (TAU * fc * t).sin()
                + quart * ((TAU * (fc - fm_freq) * t).cos() - (TAU * (fc + fm_freq) * t).cos());
            // f32 trig drift over 1024 samples × 500 Hz × TAU of
            // accumulated phase puts the realistic floor for the
            // direct-vs-expanded comparison around 1e-4.
            assert!((g - want).abs() < 1e-4, "sample {i}: got {g}, want {want}");
        }
    }

    #[test]
    fn am_zero_index_reduces_to_half_amplitude_carrier() {
        // m=0 → envelope (1 + 0·sin) = 1; the output collapses to
        // 0.5·amplitude·sin(2π·fc·t), i.e. a pure carrier sine at
        // half amplitude. The leading 0.5 is intentional — it's how
        // AM stays bounded by `amplitude` at the worst case m=1.
        let sr = 8000;
        let n = 1024;
        let fc = 440.0_f32;
        let amp = 0.8_f32;
        let am_buf = am(fc, 110.0, 0.0, sr, n, amp);
        let sine_half = sine(fc, sr, n, 0.5 * amp);
        let max_err = am_buf
            .iter()
            .zip(&sine_half)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_err < 1e-4, "max err = {max_err}");
    }

    #[test]
    fn am_carrier_is_present_unlike_ringmod() {
        // Distinguishing feature: AM keeps a carrier component at
        // f=fc, ringmod does not. Render both with the same fc/fm
        // and confirm the DFT magnitude at fc is large for AM but
        // negligible for ringmod.
        let sr = 8000u32;
        let sr_f = sr as f32;
        let n = 2048;
        let fc = 440.0_f32;
        let fm_freq = 60.0_f32;
        let am_buf = am(fc, fm_freq, 0.5, sr, n, 0.8);
        let rm_buf = ringmod(fc, fm_freq, sr, n, 0.8);
        let mag_am_carrier = dft_mag(&am_buf, fc, sr_f);
        let mag_rm_carrier = dft_mag(&rm_buf, fc, sr_f);
        // AM's carrier sits at 0.5·amplitude — by far the strongest
        // bin in the spectrum. Ringmod's bin at fc is whatever leakage
        // an integer-period mismatch produces (essentially noise floor).
        // A 10× ratio is conservative for a clean separation.
        assert!(
            mag_am_carrier > 10.0 * mag_rm_carrier,
            "AM carrier ({mag_am_carrier}) should dominate ringmod carrier ({mag_rm_carrier})"
        );
    }

    #[test]
    fn am_output_bounded_by_amplitude() {
        // At m=1 the envelope peaks at 2, the carrier peaks at 1, and
        // the leading 0.5 keeps the product at exactly amplitude in
        // the worst case. Sample-wise check across the full m range.
        for &m in &[0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let buf = am(440.0, 60.0, m, 8000, 4000, 0.7);
            for s in &buf {
                assert!(
                    s.abs() <= 0.7 + 1e-6,
                    "out-of-bounds AM sample {s} at m={m}"
                );
            }
        }
    }

    #[test]
    fn am_dispatcher_default_keeps_bounds() {
        let buf = render(&map(&[("type", "am"), ("duration", "0.05")])).unwrap();
        assert_eq!(buf.samples.len(), 400); // 8000 × 0.05
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn am_dispatcher_clamps_index() {
        // index=2 is out of the [0, 1] range. The dispatcher clamps
        // to 1.0 before calling `am`; verify by comparing against an
        // explicit index=1 render.
        let clamped = render(&map(&[
            ("type", "am"),
            ("duration", "0.05"),
            ("index", "2"),
            ("carrier", "440"),
            ("modulator", "60"),
        ]))
        .unwrap();
        let explicit = render(&map(&[
            ("type", "am"),
            ("duration", "0.05"),
            ("index", "1"),
            ("carrier", "440"),
            ("modulator", "60"),
        ]))
        .unwrap();
        assert_eq!(clamped.samples.len(), explicit.samples.len());
        for (i, (&a, &b)) in clamped.samples.iter().zip(&explicit.samples).enumerate() {
            assert!(
                (a - b).abs() < 1e-6,
                "sample {i}: clamped {a}, explicit {b}"
            );
        }
    }

    #[test]
    fn am_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("am"));
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

    // ───── ADSR ─────

    #[test]
    fn adsr_envelope_breakpoints_are_exact() {
        // 1000-sample note: attack 100, decay 200, release 300, sustain 0.5.
        // Sustain segment fills samples [300, 700); release runs [700, 1000).
        let n = 1000;
        let (a, d, r) = (100, 200, 300);
        let s = 0.5_f32;
        // End of attack (sample 99 is the last attack sample; envelope is
        // (i+1)/a so sample 99 → 100/100 = 1.0).
        assert!((adsr_envelope(99, n, a, d, s, r) - 1.0).abs() < 1e-6);
        // Midway through decay: sample at attack+decay/2 = 100+100 = 200,
        // frac = 100/200 = 0.5 → 1.0 + (0.5-1.0)*0.5 = 0.75.
        assert!((adsr_envelope(200, n, a, d, s, r) - 0.75).abs() < 1e-6);
        // Sustain plateau: any sample in [300, 700) is exactly `sustain`.
        assert!((adsr_envelope(400, n, a, d, s, r) - s).abs() < 1e-6);
        assert!((adsr_envelope(699, n, a, d, s, r) - s).abs() < 1e-6);
        // Release start (sample 700): still at the sustain level.
        assert!((adsr_envelope(700, n, a, d, s, r) - s).abs() < 1e-6);
        // Midway through release (sample 850): frac = 150/300 = 0.5 →
        // sustain * (1 - 0.5) = 0.25.
        assert!((adsr_envelope(850, n, a, d, s, r) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn adsr_envelope_is_bounded_unit_interval() {
        // Over a full note every envelope value must stay in [0, 1].
        let n = 4096;
        for i in 0..n {
            let v = adsr_envelope(i, n, 200, 400, 0.6, 800);
            assert!((0.0..=1.0).contains(&v), "sample {i} env {v} out of [0,1]");
        }
    }

    #[test]
    fn adsr_output_bounded_by_amplitude() {
        // The carrier is full-amplitude; the envelope is in [0, 1], so the
        // product can never exceed `amplitude`.
        let buf = render(&map(&[
            ("type", "adsr"),
            ("freq", "440"),
            ("attack", "0.02"),
            ("decay", "0.05"),
            ("sustain", "0.6"),
            ("release", "0.1"),
            ("duration", "0.5"),
            ("amplitude", "0.9"),
        ]))
        .unwrap();
        for s in &buf.samples {
            assert!(s.abs() <= 0.9 + 1e-6, "out-of-bounds adsr sample {s}");
        }
    }

    #[test]
    fn adsr_starts_silent_and_decays_to_silence() {
        // A note that starts at envelope 0 and ends at envelope 0 means the
        // very first sample (attack from 0) is near-silent and the final
        // sample (release to 0) is exactly 0.
        let sr = 8000;
        let out = adsr(440.0, "sine", 0.05, 0.05, 0.7, 0.2, sr, 4000, 1.0).unwrap();
        assert_eq!(out.len(), 4000);
        // sin(0)=0 and the attack envelope is small at i=0, so sample 0 is
        // tiny regardless.
        assert!(out[0].abs() < 0.05);
        // Last sample: release envelope reaches 0 at i = n, so the very
        // last in-range sample is the carrier times a near-zero envelope.
        let last = *out.last().unwrap();
        assert!(last.abs() < 0.05, "final sample {last} not near silence");
    }

    #[test]
    fn adsr_default_wave_is_sine_carrier() {
        // With a flat-ish envelope (long sustain), the mid-note samples
        // should track a pure sine at the carrier amplitude scaled by the
        // sustain level. We compare the envelope-removed signal against the
        // sine helper at a sustain-region sample.
        let sr = 8000;
        let n = 8000;
        let sustain = 0.5_f32;
        let out = adsr(440.0, "sine", 0.01, 0.01, sustain, 0.01, sr, n, 1.0).unwrap();
        let reference = sine(440.0, sr, n, 1.0);
        // Sample 4000 is deep in the sustain plateau (attack+decay ≈ 160
        // samples, release window is the last 80), so envelope == sustain.
        let i = 4000;
        let recovered = out[i] / sustain;
        assert!(
            (recovered - reference[i]).abs() < 1e-4,
            "recovered {recovered} vs reference {}",
            reference[i]
        );
    }

    #[test]
    fn adsr_unknown_wave_errors() {
        let err = render(&map(&[("type", "adsr"), ("wave", "noise")])).unwrap_err();
        assert!(format!("{err}").contains("noise"));
    }

    #[test]
    fn adsr_listed_in_unknown_type_help() {
        // The dispatcher's "expected …" hint should advertise adsr.
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("adsr"));
    }

    // ───── Formant (Klatt-style two-formant vowel synth) ─────

    /// Tiny single-bin Goertzel-style DFT magnitude at frequency `freq`
    /// (Hz) over `x` sampled at `sr` Hz. Returns the unnormalised
    /// `sqrt(real² + imag²)` magnitude — only the *relative* strength
    /// across frequencies matters for the peak-detection tests below.
    fn dft_mag(x: &[f32], freq: f32, sr: f32) -> f32 {
        let omega = TAU * freq / sr;
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for (i, &s) in x.iter().enumerate() {
            let phi = omega * i as f32;
            re += s * phi.cos();
            im += s * phi.sin();
        }
        (re * re + im * im).sqrt()
    }

    #[test]
    fn vowel_formants_lookup_table() {
        // Spot-check the five vowels against the textbook-standard
        // adult-male formant centres documented on `vowel_formants`.
        assert_eq!(vowel_formants("A").unwrap(), (730.0, 1090.0));
        assert_eq!(vowel_formants("E").unwrap(), (530.0, 1840.0));
        assert_eq!(vowel_formants("I").unwrap(), (270.0, 2290.0));
        assert_eq!(vowel_formants("O").unwrap(), (570.0, 840.0));
        assert_eq!(vowel_formants("U").unwrap(), (300.0, 870.0));
        // Case-insensitive.
        assert_eq!(vowel_formants("a").unwrap(), vowel_formants("A").unwrap());
        // Unknown vowel is an error, not a silent default.
        assert!(vowel_formants("X").is_err());
        // Multi-character input is an error too.
        assert!(vowel_formants("AE").is_err());
    }

    #[test]
    fn formant_output_bounded_by_amplitude() {
        // Peak normalisation should keep every sample inside [-amp, amp].
        let sr = 16_000;
        let n = 1600; // 100 ms
        let amp = 0.8;
        let out = formant(220.0, 730.0, 1090.0, 80.0, sr, n, amp);
        for s in &out {
            assert!(
                s.abs() <= amp + 1e-6,
                "out-of-bounds formant sample {s} (amp={amp})"
            );
        }
    }

    #[test]
    fn formant_zero_length_is_empty() {
        let out = formant(220.0, 730.0, 1090.0, 80.0, 16_000, 0, 0.8);
        assert!(out.is_empty());
    }

    #[test]
    fn formant_spectral_peaks_near_expected_centres() {
        // Per-vowel: render 100 ms of the vowel at 220 Hz @ 16 kHz, then
        // compare the DFT magnitude at the formant centre against the
        // magnitude at several off-formant probe frequencies. The
        // formant centres should dominate by a clear margin — that's
        // the whole point of the two-pole resonator pair.
        //
        // The probe frequencies are picked so they lie comfortably
        // outside the ±80 Hz resonator −3 dB bandwidth of both F1 and
        // F2 for *every* vowel in the table (we use 3000 Hz, which is
        // above every F2 in the table by ≥710 Hz, and 4500 Hz which is
        // higher still).
        let sr = 16_000u32;
        let sr_f = sr as f32;
        let n = (0.1 * sr_f) as usize; // 100 ms
        let f0 = 220.0;
        let bw = 80.0;

        for vowel in ["A", "E", "I", "O", "U"] {
            let (f1, f2) = vowel_formants(vowel).unwrap();
            let buf = formant(f0, f1, f2, bw, sr, n, 0.9);
            assert_eq!(buf.len(), n);

            // The DFT magnitude at f0's harmonic *closest* to a formant
            // is what we measure — a 220 Hz harmonic comb means peak
            // energy lands not at F1 / F2 themselves but at the nearest
            // multiple of f0. Round the formant centres down/up to the
            // nearest harmonic for the probe.
            let nearest_harmonic = |f_target: f32| {
                let k = (f_target / f0).round();
                (k * f0).max(f0)
            };
            let probe_f1 = nearest_harmonic(f1);
            let probe_f2 = nearest_harmonic(f2);

            let mag_f1 = dft_mag(&buf, probe_f1, sr_f);
            let mag_f2 = dft_mag(&buf, probe_f2, sr_f);

            // Off-formant probe — pick a harmonic of f0 between F2 and
            // Nyquist that is at least 800 Hz above every formant in
            // the table (max F2 is 2290 Hz, so 3300 Hz is a clear
            // outside-band probe — and it's a multiple of f0=220 Hz,
            // so we're sampling on the harmonic grid).
            let mag_off = dft_mag(&buf, 3300.0, sr_f);

            // Each formant magnitude should clearly dominate the
            // off-band magnitude. A 3× ratio is conservative: a 2-pole
            // resonator with BW=80 Hz delivers ~20+ dB of peak/trough
            // contrast across ~1 kHz of detuning.
            assert!(
                mag_f1 > 3.0 * mag_off,
                "{vowel}: F1 peak too weak ({mag_f1} at {probe_f1} Hz vs {mag_off} at 3300 Hz)"
            );
            assert!(
                mag_f2 > 3.0 * mag_off,
                "{vowel}: F2 peak too weak ({mag_f2} at {probe_f2} Hz vs {mag_off} at 3300 Hz)"
            );
        }
    }

    #[test]
    fn formant_dispatcher_default_keeps_bounds() {
        // 1 s @ 8 kHz default, default vowel=A → no panic, samples bounded.
        let buf = render(&map(&[("type", "formant"), ("duration", "0.1")])).unwrap();
        assert_eq!(buf.samples.len(), 800); // 8000 × 0.1
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn formant_vowel_alias_works() {
        // `type=vowel` is an accepted alias for `type=formant`.
        let buf = render(&map(&[
            ("type", "vowel"),
            ("vowel", "E"),
            ("duration", "0.05"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 400);
    }

    #[test]
    fn formant_unknown_vowel_errors() {
        let err = render(&map(&[("type", "formant"), ("vowel", "Z")])).unwrap_err();
        assert!(format!("{err}").contains("'Z'") || format!("{err}").contains("Z"));
    }

    #[test]
    fn formant_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("formant"));
    }

    #[test]
    fn resonator_peak_response_at_centre() {
        // Drive a single 2-pole resonator with an impulse and confirm
        // the resulting (decaying-sinusoid) response has its single-bin
        // DFT peak at the resonator's centre frequency. Sanity test
        // for the underlying biquad.
        let sr = 16_000.0_f32;
        let n = 2048;
        let f = 1000.0_f32;
        let bw = 50.0_f32;
        let mut x = vec![0.0f32; n];
        x[0] = 1.0;
        let y = resonator(&x, f, bw, sr);
        let mag_peak = dft_mag(&y, f, sr);
        // 200 Hz away from the centre should be appreciably weaker.
        let mag_off = dft_mag(&y, f + 400.0, sr);
        assert!(
            mag_peak > 3.0 * mag_off,
            "resonator peak {mag_peak} not dominant over off-centre {mag_off}"
        );
    }
}

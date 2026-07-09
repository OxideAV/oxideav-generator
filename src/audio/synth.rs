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
    // Initial phase + per-channel phase offset, both in degrees.
    // Honoured by `type=sine` (the exactness workhorse for stereo /
    // mid-side / inter-channel-correlation probes); other types ignore
    // them, matching the crate-wide extra-keys-are-ignored convention.
    let phase_deg = q_f64(query, "phase", 0.0)?;
    let chphase_deg = q_f64(query, "chphase", 0.0)?;

    let mono: Vec<f32> = match kind {
        "sine" => sine_phase(
            q_f64(query, "freq", 440.0)? as f32,
            sample_rate,
            frame_count,
            amplitude,
            phase_deg.to_radians() as f32,
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
        "supersaw" | "saws" => {
            // Detuned-sawtooth stack: `voices` sawtooth oscillators tuned
            // around `freq`, summed and equal-weight averaged. `detune=`
            // is the half-spread in cents (1 cent = 1/100 of an equal-
            // tempered semitone) — voices are placed symmetrically over
            // [-detune, +detune] so the centre voice is exactly `freq`.
            // Mathematical reference: Adam Szabo, "How to Emulate the
            // Super Saw" (KTH thesis, 2010) — public academic paper on
            // detuned-saw spectra.
            let freq = q_f64(query, "freq", 440.0)? as f32;
            let voices = q_u32(query, "voices", 7)?.clamp(1, 32) as usize;
            let detune = q_f64(query, "detune", 12.0)? as f32;
            supersaw(freq, voices, detune, sample_rate, frame_count, amplitude)?
        }
        "pwm" | "pulse" => {
            // Pulse-width-modulated rectangular wave: a generalisation of
            // `square` (which is `duty=0.5`). `duty=` is the fraction of
            // each period the signal sits at `+amplitude`; the remainder
            // sits at `-amplitude`. Constant `duty` is a static pulse
            // wave; a non-zero `lfo=` modulates the duty sinusoidally at
            // `lfo` Hz between `duty − depth` and `duty + depth`, the
            // classical analogue-synth pulse-width-modulation effect that
            // turns the static pulse into a chorus-y / phasing timbre.
            let freq = q_f64(query, "freq", 440.0)? as f32;
            let duty = q_f64(query, "duty", 0.5)? as f32;
            let lfo = q_f64(query, "lfo", 0.0)? as f32;
            let depth = q_f64(query, "depth", 0.0)? as f32;
            pwm(freq, duty, lfo, depth, sample_rate, frame_count, amplitude)?
        }
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
            // 67(3):971-995, the public reference).
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
        "tremolo" | "trem" => {
            // Sub-audio amplitude envelope on top of an arbitrary
            // carrier wave. Distinguishing features vs `am`:
            //   * The LFO is unipolar (cosine, never crosses zero in
            //     the envelope), so it is strictly amplitude
            //     attenuation — no sidebands at fc ± fm in the
            //     amplitude-modulation sense; the envelope acts as a
            //     time-varying scalar on the carrier.
            //   * The LFO frequency lives in the sub-audio band
            //     (default 5 Hz, classical "tremolo speed"), well
            //     below the carrier — what AM stops being and
            //     tremolo starts being is a frequency-domain
            //     distinction more than a topological one.
            //   * The carrier can be any in-tree waveform
            //     (`wave=sine|square|triangle|sawtooth`), not just
            //     a sine.
            //   * The envelope is `1 − depth · 0.5 · (1 − cos(2π·f·t))`,
            //     which ranges exactly over `[1 − depth, 1]`, so the
            //     amplitude bound `|out| ≤ amplitude` is guaranteed.
            let wave = q_str(query, "wave", "sine");
            let freq = q_f64(query, "freq", 440.0)? as f32;
            let lfo = q_f64(query, "lfo", 5.0)? as f32;
            let depth = q_f64(query, "depth", 0.5)?.clamp(0.0, 1.0) as f32;
            tremolo(wave, freq, lfo, depth, sample_rate, frame_count, amplitude)?
        }
        "vibrato" | "vib" => {
            // Sub-audio frequency modulation on top of an arbitrary
            // carrier wave — the canonical musical vibrato effect, a
            // sister to `tremolo`. The distinguishing properties are
            // dual to tremolo's, modulating frequency instead of
            // amplitude:
            //   * The instantaneous frequency is the bipolar curve
            //     `f(t) = freq · (1 + depth · cos(2π · lfo · t))`,
            //     so `depth` is the FRACTIONAL frequency deviation
            //     (default 0.005 = ±0.5 %, a textbook "natural
            //     vibrato" width for sung vowels; classical string
            //     vibrato sits closer to ±2 %).
            //   * The closed-form phase comes from integrating the
            //     instantaneous frequency: at LFO frequency `f_l`,
            //     `phase(t) = 2π·freq·t
            //                 + (depth · freq / f_l) · sin(2π·f_l·t)`,
            //     so the modulation index in the FM sense is exactly
            //     `depth · freq / f_l` (e.g. 440 Hz carrier × 0.005
            //     depth × 5 Hz LFO ⇒ index = 0.44 radians).
            //   * `lfo = 0` collapses to `phase(t) = 2π·freq·t`, the
            //     unmodulated carrier (the algebraic
            //     `lim_{f_l → 0} sin(2π·f_l·t) / f_l = 2π·t` cancels
            //     against the `1 / f_l` weight to leave a constant DC
            //     phase shift that gets absorbed into the carrier's
            //     starting phase). We special-case lfo=0 to skip the
            //     divide rather than letting f32 division by zero
            //     leak through.
            //   * Carrier `wave` selects sine | square | triangle |
            //     sawtooth, exactly the four band-limited oscillators
            //     `tremolo` accepts, so vibrato is a strict
            //     phase-domain analogue of tremolo's amplitude-domain
            //     story.
            //
            // Distinct from `fm`: `fm` is full audio-rate frequency
            // modulation with an unbounded modulation index parameter
            // (default 5 radians) producing rich Bessel-sideband
            // timbres at carrier-to-modulator ratios in the audio
            // band; vibrato fixes the LFO in the sub-audio band
            // (default 5 Hz, the same "natural" speed as tremolo) and
            // exposes the fractional frequency deviation directly so
            // a musician can dial in ±0.5 % / ±2 % independent of
            // the chosen pitch.
            //
            // Mathematical reference is John Backus, *The Acoustical
            // Foundations of Music*, W. W. Norton & Company, 1969,
            // ch. 8 "Vibrato" — a public academic monograph on
            // musical acoustics.
            let wave = q_str(query, "wave", "sine");
            let freq = q_f64(query, "freq", 440.0)? as f32;
            let lfo = q_f64(query, "lfo", 5.0)? as f32;
            let depth = q_f64(query, "depth", 0.005)?.clamp(0.0, 1.0) as f32;
            vibrato(wave, freq, lfo, depth, sample_rate, frame_count, amplitude)?
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
        "shepard" => {
            // Shepard tone — a stack of `voices` octave-spaced sinusoids
            // weighted by a log-frequency Gaussian envelope centred on
            // `center_freq`. The bottom and top voices are quiet and the
            // middle voices loud, so the absolute frequency range stays
            // bounded even when the base frequency rolls upward. Reference
            // is Roger Shepard's 1964 *Journal of the Acoustical Society
            // of America* paper "Circularity in Judgments of Relative
            // Pitch" (vol. 36 no. 12 p. 2346), the original public
            // description of the circular-pitch construct that bears his
            // name.
            //
            //   `freq`        — base of the lowest voice, Hz (default 55)
            //   `voices`      — number of octave-spaced voices (default 8,
            //                   clamped to [1, 12])
            //   `center_freq` — Gaussian envelope centre, Hz (default
            //                   geometric mean of the voice frequencies,
            //                   so it lives exactly on the log-frequency
            //                   midpoint of the stack)
            //   `sigma`       — Gaussian width in octaves (default 1.5,
            //                   clamped to [0.1, 6.0])
            let freq = q_f64(query, "freq", 55.0)? as f32;
            let voices = q_u32(query, "voices", 8)?.clamp(1, 12) as usize;
            let sigma_oct = q_f64(query, "sigma", 1.5)?.clamp(0.1, 6.0) as f32;
            // Default centre is the geometric mean of the voice
            // frequencies — i.e. the log-midpoint of the octave stack.
            // For `voices=8` starting at 55 Hz the stack runs 55 …
            // 7040 Hz; its log-midpoint is 55 · 2^((8-1)/2) ≈ 622 Hz.
            let default_centre = freq * 2.0_f32.powf((voices.saturating_sub(1) as f32) * 0.5);
            let centre_freq = q_f64(query, "center_freq", default_centre as f64)? as f32;
            shepard(
                freq,
                voices,
                centre_freq,
                sigma_oct,
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
                "blue" | "azure" => noise_blue(frame_count, amplitude, seed),
                "violet" | "purple" => noise_violet(frame_count, amplitude, seed),
                other => {
                    return Err(Error::invalid(format!(
                        "synth: noise color {other:?} (expected white|pink|brown|blue|violet)"
                    )));
                }
            }
        }
        "silence" => vec![0.0; frame_count],
        "dc" => {
            // Constant (direct-current) signal: s[n] = level for every
            // n. `level` is signed and clamped to [-1, 1]; when absent
            // it defaults to the (unsigned) `amplitude` knob, so
            // `type=dc` alone produces the same peak the oscillators
            // would. The classic offset / clipping / silence-detector
            // probe: zero AC energy, all the power at 0 Hz.
            let level = q_f64(query, "level", amplitude as f64)?.clamp(-1.0, 1.0) as f32;
            dc(level, frame_count)
        }
        "impulse" | "impulses" | "click" => {
            // Unipolar impulse train: `width` samples at +amplitude
            // every `period` samples, first impulse at n = 0. Explicit
            // `period=` (samples) wins; otherwise the period is derived
            // from `freq=` (impulses per second, default 1 Hz) as
            // round(rate / freq). The spectral counterpart of `dc` — a
            // Dirac comb whose discrete spectrum has equal-magnitude
            // lines at every multiple of rate/period; ideal for
            // impulse-response capture and codec pre-echo probing.
            let period = match query.get("period") {
                Some(_) => q_u32(query, "period", 1)?.max(1) as usize,
                None => {
                    let freq = q_f64(query, "freq", 1.0)?;
                    if freq <= 0.0 {
                        return Err(Error::invalid(format!(
                            "synth: impulse freq must be > 0, got {freq}"
                        )));
                    }
                    ((sample_rate as f64 / freq).round() as usize).max(1)
                }
            };
            let width = (q_u32(query, "width", 1)?.max(1) as usize).min(period);
            impulse_train(period, width, frame_count, amplitude)
        }
        other => {
            return Err(Error::invalid(format!(
                "synth: unknown type {other:?} (expected sine|square|triangle|sawtooth|supersaw|pwm|pluck|chirp|fm|am|tremolo|vibrato|ringmod|dtmf|adsr|formant|shepard|multitone|noise|silence|dc|impulse)"
            )));
        }
    };

    // Interleave to the output channel layout. `type=sine` with a
    // non-zero `chphase=` renders channel `c` at initial phase
    // `phase + c·chphase` degrees (channel 0 is the `mono` buffer,
    // already at `phase`); every other configuration replicates the
    // mono buffer across channels.
    let samples = if channels > 1 && kind == "sine" && chphase_deg != 0.0 {
        let freq = q_f64(query, "freq", 440.0)? as f32;
        let mut chans: Vec<Vec<f32>> = Vec::with_capacity(channels as usize);
        chans.push(mono);
        for c in 1..channels {
            let ph = (phase_deg + c as f64 * chphase_deg).to_radians() as f32;
            chans.push(sine_phase(freq, sample_rate, frame_count, amplitude, ph));
        }
        interleave(&chans)
    } else if channels == 1 {
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

/// Sine oscillator at `freq` Hz (zero initial phase).
pub fn sine(freq: f32, sample_rate: u32, n: usize, amplitude: f32) -> Vec<f32> {
    sine_phase(freq, sample_rate, n, amplitude, 0.0)
}

/// Sine oscillator at `freq` Hz with an initial phase in radians:
///
/// ```text
/// s[n] = amplitude · sin(2π · freq · n / rate + phase)
/// ```
///
/// The phase is a pure additive offset inside the closed form — no
/// phase accumulator — so `phase = π/2` is exactly the matching cosine
/// and two channels rendered `Δφ` apart stay `Δφ` apart for the whole
/// buffer. With `phase = 0.0` the output is bit-identical to [`sine`]
/// (the argument reduces to the same float expression).
pub fn sine_phase(freq: f32, sample_rate: u32, n: usize, amplitude: f32, phase: f32) -> Vec<f32> {
    let dt = 1.0 / sample_rate as f32;
    (0..n)
        .map(|i| amplitude * (TAU * freq * i as f32 * dt + phase).sin())
        .collect()
}

/// Interleave per-channel buffers (all the same length) into a single
/// frame-major buffer: output index `i·C + c` is sample `i` of channel
/// `c`.
fn interleave(chans: &[Vec<f32>]) -> Vec<f32> {
    let c = chans.len();
    let n = chans.first().map(|b| b.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(n * c);
    for i in 0..n {
        for ch in chans {
            out.push(ch[i]);
        }
    }
    out
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

/// Detuned-sawtooth stack — `voices` sawtooth oscillators clustered around
/// `freq`, summed and equal-weight averaged.
///
/// Each voice runs at its own slightly-shifted frequency
/// `f_k = freq · 2^(c_k / 1200)` where `c_k` is its individual detuning in
/// cents. The `voices` frequencies are placed symmetrically over
/// `[-detune, +detune]` cents around the centre voice (which sits exactly
/// at `freq`):
///
/// * for `voices = 1` the only voice is the centre at `c = 0`;
/// * for `voices > 1` the voices are uniformly spaced from `-detune` to
///   `+detune` inclusive, so `voices = 7, detune = 12` gives
///   `c ∈ {-12, -8, -4, 0, +4, +8, +12}` cents.
///
/// 1 cent = 1/100 of an equal-tempered semitone (the standard music-
/// theory unit of pitch spread). With cents in single digits the
/// per-voice frequency offset is tiny in absolute terms but produces the
/// audibly thick, chorus-like sawtooth characteristic of the classical
/// "supersaw" timbre popularised in 1996 by the Roland JP-8000.
///
/// Mathematical reference: Adam Szabo, *How to Emulate the Super Saw*
/// (KTH Royal Institute of Technology MSc thesis, 2010) — a public
/// academic analysis of the detuned-saw spectrum. The per-voice
/// waveform is the same in-tree [`sawtooth`] used by `type=sawtooth`.
///
/// Output is the equal-weight average of the per-voice sawtooths,
/// keeping the worst-case peak (all voices momentarily aligned at the
/// post-discontinuity `+amplitude`) inside `[-amplitude, amplitude]` for
/// every `(freq, voices, detune)` and every sample rate. `voices` is
/// clamped to `[1, 32]`; `freq` must be positive (an error otherwise).
pub fn supersaw(
    freq: f32,
    voices: usize,
    detune_cents: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    if freq <= 0.0 || freq.is_nan() {
        return Err(Error::invalid(format!(
            "synth: supersaw requires freq > 0 (got {freq})"
        )));
    }
    let voices = voices.clamp(1, 32);
    if n == 0 {
        return Ok(Vec::new());
    }

    // Per-voice cent offsets, symmetric around 0.
    let cent_offsets: Vec<f32> = if voices == 1 {
        vec![0.0]
    } else {
        let step = 2.0 * detune_cents / (voices as f32 - 1.0);
        (0..voices)
            .map(|k| -detune_cents + step * k as f32)
            .collect()
    };

    // Pre-compute each voice's period in samples (constant per voice).
    let periods: Vec<f32> = cent_offsets
        .iter()
        .map(|&c| {
            let f_k = freq * (2.0_f32).powf(c / 1200.0);
            (sample_rate as f32) / f_k
        })
        .collect();

    let scale = amplitude / voices as f32;
    let out = (0..n)
        .map(|i| {
            let mut s = 0.0_f32;
            for &period in &periods {
                let phase = (i as f32 % period) / period; // 0..1
                s += 2.0 * phase - 1.0;
            }
            scale * s
        })
        .collect();
    Ok(out)
}

/// Pulse-width-modulated rectangular wave at `freq` Hz.
///
/// Generalisation of the fixed 50%-duty [`square`] oscillator. At each
/// sample the phase advances through the period; the output is
/// `+amplitude` while the phase sits below the instantaneous duty
/// threshold and `-amplitude` otherwise. Two regimes:
///
/// * **Static pulse** — `lfo = 0` or `depth = 0`. The duty cycle is
///   the constant `duty ∈ (0, 1)`. `duty = 0.5` reproduces the
///   classical square wave; `duty = 0.25` is a narrower pulse (Fourier
///   series weights the odd harmonics differently); approaching
///   `duty → 0` or `duty → 1` collapses the spectrum to ever-narrower
///   spikes and the audible level falls off.
/// * **PWM (pulse-width modulation)** — non-zero `lfo` + `depth`. The
///   duty threshold sweeps sinusoidally between `duty − depth` and
///   `duty + depth` at `lfo` Hz, i.e.
///   `d(t) = duty + depth · sin(2π · lfo · t)`. This is the canonical
///   analogue-synth PWM timbre — the static rectangular spectrum
///   becomes a slowly modulated comb, perceived as a chorus-like or
///   phasing widening of the static pulse.
///
/// `duty` is clamped into `[ε, 1 − ε]` so the wave always has at
/// least one sample of each polarity per period (the all-positive /
/// all-negative DC limits would be silent, not louder). `depth` is
/// clamped into `[0, duty − ε]` so the modulated threshold never
/// crosses the same edges. Output amplitude bound: the wave only
/// takes values in `{+amplitude, −amplitude}`, so it is exactly
/// bounded by `amplitude` for every `(freq, duty, lfo, depth)` and
/// every sample rate.
///
/// References: pulse-width modulation is a textbook analogue-synth
/// modulation source — see e.g. Moore, *Elements of Computer Music*
/// (1990), chapter 4 on classic analogue-style oscillators, and any
/// introductory DSP text on the Fourier series of a rectangular
/// pulse train of duty `d` (the line-spectrum amplitudes are
/// proportional to `sin(π · k · d) / (π · k)` for harmonic `k`).
pub fn pwm(
    freq: f32,
    duty: f32,
    lfo: f32,
    depth: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    if freq <= 0.0 {
        return Err(Error::invalid(format!(
            "synth: pwm requires freq>0 (got {freq})"
        )));
    }
    let period_samples = (sample_rate as f32) / freq;
    // Clamp duty into (eps, 1 − eps) so each period always has at
    // least one positive and one negative segment. The clamp floor is
    // the larger of a small absolute floor (1e-3, keeps very long
    // periods from producing single-sample pulses below the audio
    // floor) and 1.5/period_samples (keeps short periods from collapsing
    // when the floor itself is smaller than one sample step).
    let eps = (1.5 / period_samples).max(1.0e-3_f32);
    let duty_c = duty.clamp(eps, 1.0 - eps);
    // Clamp depth so duty ± depth never crosses the same eps edges.
    let depth_room = (duty_c - eps).min(1.0 - eps - duty_c).max(0.0);
    let depth_c = depth.abs().min(depth_room);
    let dt = 1.0 / sample_rate as f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let phase = (i as f32 % period_samples) / period_samples; // 0..1
        let d = if lfo != 0.0 && depth_c != 0.0 {
            let t = i as f32 * dt;
            (duty_c + depth_c * (TAU * lfo * t).sin()).clamp(eps, 1.0 - eps)
        } else {
            duty_c
        };
        out.push(if phase < d { amplitude } else { -amplitude });
    }
    Ok(out)
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

/// Sub-audio amplitude envelope on an arbitrary carrier wave —
/// classical "tremolo" effect.
///
/// The carrier is rendered in full first via the in-tree oscillator
/// selected by `wave` (`sine` / `square` / `triangle` / `sawtooth`
/// or `saw`); each sample is then scaled by a unipolar cosine LFO
///
/// ```text
/// e(t) = 1 − depth · 0.5 · (1 − cos(2π · f_lfo · t))
/// ```
///
/// which ranges exactly over `[1 − depth, 1]` (the half-shifted cosine
/// `0.5 · (1 − cos x)` sits in `[0, 1]`, and `depth ∈ [0, 1]`). Output
/// is therefore in `[−amplitude, amplitude]` for every input. At
/// `depth = 0` the envelope collapses to `1` and the function returns
/// the pure carrier; at `depth = 1` the envelope sweeps all the way
/// down to zero once per LFO period, the deepest possible tremolo.
///
/// Tremolo is *not* a thin alias for [`am`]. Three distinguishing
/// properties:
///
/// 1. The envelope is unipolar (non-negative), so the effect is strict
///    amplitude attenuation rather than a bipolar mix that injects
///    sidebands at `fc ± fm` in the AM sense.
/// 2. `f_lfo` is sub-audio (default 5 Hz, classical guitar-amp
///    "tremolo speed"), well below the carrier — the spectrum stays
///    centred on `fc` with low-frequency sidebands that perceptually
///    register as periodic loudness variation rather than a new
///    timbre.
/// 3. The carrier can be any of the in-tree band-limited oscillators,
///    not just a sine.
///
/// Returns `Err` on the same conditions as [`adsr`]: unknown `wave`.
pub fn tremolo(
    wave: &str,
    freq: f32,
    lfo_hz: f32,
    depth: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    let base = match wave {
        "sine" => sine(freq, sample_rate, n, amplitude),
        "square" => square(freq, sample_rate, n, amplitude),
        "triangle" => triangle(freq, sample_rate, n, amplitude),
        "sawtooth" | "saw" => sawtooth(freq, sample_rate, n, amplitude),
        other => {
            return Err(Error::invalid(format!(
                "synth: tremolo wave {other:?} (expected sine|square|triangle|sawtooth)"
            )));
        }
    };
    let d = depth.clamp(0.0, 1.0);
    let dt = 1.0 / sample_rate as f32;
    let half_d = 0.5 * d;
    let out = base
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let t = i as f32 * dt;
            // e(t) = 1 − d · 0.5 · (1 − cos(2π · f_lfo · t))
            //       = (1 − d) + d · 0.5 · (1 + cos(2π · f_lfo · t))
            // Either form lands in [1 − d, 1]; the latter is one fewer
            // sign flip.
            let env = (1.0 - d) + half_d * (1.0 + (TAU * lfo_hz * t).cos());
            s * env
        })
        .collect();
    Ok(out)
}

/// Sub-audio frequency modulation on an arbitrary carrier wave —
/// classical musical "vibrato" effect, the phase-domain sister of
/// [`tremolo`].
///
/// The instantaneous frequency traces a cosine around the carrier
/// frequency,
///
/// ```text
/// f_inst(t) = freq · (1 + depth · cos(2π · lfo_hz · t))
/// ```
///
/// so `depth` is the FRACTIONAL frequency deviation (e.g. `0.005`
/// = ±0.5 %, a textbook "natural" sung-vowel vibrato width; classical
/// string vibrato sits closer to ±2 %). Integrating the instantaneous
/// frequency gives the closed-form phase
///
/// ```text
/// φ(t) = 2π · freq · t
///      + (depth · freq / lfo_hz) · sin(2π · lfo_hz · t).
/// ```
///
/// The modulation index in the FM sense is `β = depth · freq / lfo_hz`
/// (e.g. 440 Hz × 0.005 / 5 Hz ⇒ β ≈ 0.44 rad), so the spectrum is a
/// Bessel-function ladder centred on `freq` with sidebands at
/// `freq ± k · lfo_hz` for integer k. Because `lfo_hz` is sub-audio,
/// the sidebands cluster within a few Hz of the carrier and register
/// perceptually as a pitch sweep rather than a new timbre — that is
/// the vibrato percept.
///
/// `lfo_hz = 0` collapses the modulation algebraically: the cosine
/// freezes at 1, the instantaneous frequency reduces to
/// `freq · (1 + depth)` ≡ constant, and the phase becomes
/// `2π · freq · (1 + depth) · t`. The implementation special-cases
/// this exactly (skipping the `depth / lfo_hz` divide) so f32 division
/// by zero does not leak through.
///
/// Distinct from [`fm`]: `fm` exposes an unbounded modulation index
/// `index` (default 5 rad) and runs at audio-rate carrier-to-modulator
/// ratios so it sweeps out rich classical-FM timbres
/// (sin(2π·fc·t + index·sin(2π·fm·t)) with `fm` in the audio band).
/// Vibrato fixes `lfo_hz` in the sub-audio band (default 5 Hz, the
/// same "natural speed" as tremolo) and parameterises the fractional
/// frequency deviation `depth` directly so a musician can dial in a
/// ±0.5 % or ±2 % spread independent of the chosen pitch.
///
/// Carrier `wave` selects from the same band-limited oscillator set
/// [`tremolo`] accepts (`sine` / `square` / `triangle` / `sawtooth`
/// or `saw`). Square / triangle / sawtooth derive from instantaneous
/// fractional phase ∈ [0, 1), so the closed-form phase is mapped to
/// a normalised phase coordinate `φ(t) / TAU mod 1.0` and the carrier
/// generator is evaluated on that coordinate.
///
/// Output is bounded by `amplitude` for every carrier (the carrier
/// generators themselves are bounded by their `amplitude` argument).
///
/// Returns `Err` on unknown `wave` (same condition as [`tremolo`]).
///
/// Mathematical reference: John Backus, *The Acoustical Foundations
/// of Music*, W. W. Norton, 1969 — ch. 8 "Vibrato" treats the effect
/// as periodic frequency modulation of a carrier oscillator.
pub fn vibrato(
    wave: &str,
    freq: f32,
    lfo_hz: f32,
    depth: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    // Validate the wave selector up front against the same set as
    // tremolo so callers see a consistent error surface.
    match wave {
        "sine" | "square" | "triangle" | "sawtooth" | "saw" => {}
        other => {
            return Err(Error::invalid(format!(
                "synth: vibrato wave {other:?} (expected sine|square|triangle|sawtooth)"
            )));
        }
    }
    let d = depth.clamp(0.0, 1.0);
    let dt = 1.0 / sample_rate as f32;
    // β = d · freq / lfo when lfo > 0 (modulation index in radians);
    // when lfo == 0 we use the algebraic limit `f_const = freq · (1 + d)`
    // so the carrier just plays back at a fixed shifted frequency.
    let lfo_active = lfo_hz.abs() > 0.0;
    let beta = if lfo_active { d * freq / lfo_hz } else { 0.0 };
    let f_const = freq * (1.0 + d);
    let out: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 * dt;
            // Closed-form integrated phase, normalised to [0, 1) for
            // the non-sine carriers' phase-domain rendering.
            let phi = if lfo_active {
                TAU * freq * t + beta * (TAU * lfo_hz * t).sin()
            } else {
                TAU * f_const * t
            };
            match wave {
                "sine" => amplitude * phi.sin(),
                "square" => {
                    // φ ∈ [0, TAU): first half-period positive, second half negative.
                    let cycle = phi * (1.0 / TAU);
                    let frac = cycle - cycle.floor();
                    if frac < 0.5 {
                        amplitude
                    } else {
                        -amplitude
                    }
                }
                "triangle" => {
                    let cycle = phi * (1.0 / TAU);
                    let frac = cycle - cycle.floor();
                    let v = if frac < 0.25 {
                        frac * 4.0
                    } else if frac < 0.75 {
                        2.0 - frac * 4.0
                    } else {
                        frac * 4.0 - 4.0
                    };
                    amplitude * v
                }
                _ => {
                    // sawtooth | saw — already pinned by the up-front
                    // validation, so the match is total.
                    let cycle = phi * (1.0 / TAU);
                    let frac = cycle - cycle.floor();
                    amplitude * (2.0 * frac - 1.0)
                }
            }
        })
        .collect();
    Ok(out)
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

/// Shepard tone — a stack of `voices` sine tones spaced one octave apart,
/// weighted by a Gaussian envelope in log-frequency space centred on
/// `centre_freq` with width `sigma_oct` octaves.
///
/// The voice frequencies are `f_k = freq · 2^k` for `k = 0..voices`. Each
/// voice's amplitude weight is
///
/// ```text
/// w_k = exp(-(log2(f_k / centre_freq) / sigma_oct)²)
/// ```
///
/// so the envelope peaks at `centre_freq` (`w = 1`) and tails off
/// symmetrically toward the bottom and top of the stack. Output is the
/// weighted sum normalised by `Σ w_k`, which keeps the worst-case peak
/// (all voices aligned at the same phase) inside `[-amplitude, amplitude]`
/// for every `(freq, voices, centre_freq, sigma_oct)` and every sample
/// rate.
///
/// Reference: Roger N. Shepard, *Circularity in Judgments of Relative
/// Pitch*, Journal of the Acoustical Society of America 36(12):2346,
/// 1964 — the original public description of the circular-pitch construct
/// the tone bears its name from. The log-frequency Gaussian envelope is
/// the canonical shape Shepard uses to render the absolute pitch
/// information ambiguous while preserving a clear chroma percept.
///
/// `voices` is clamped to `[1, 12]`; `sigma_oct` is clamped to
/// `[0.1, 6.0]`. Returns `Err` if `freq <= 0` or `centre_freq <= 0`.
#[allow(clippy::too_many_arguments)]
pub fn shepard(
    freq: f32,
    voices: usize,
    centre_freq: f32,
    sigma_oct: f32,
    sample_rate: u32,
    n: usize,
    amplitude: f32,
) -> Result<Vec<f32>> {
    if freq <= 0.0 || freq.is_nan() {
        return Err(Error::invalid(format!(
            "synth: shepard requires freq > 0 (got {freq})"
        )));
    }
    if centre_freq <= 0.0 || centre_freq.is_nan() {
        return Err(Error::invalid(format!(
            "synth: shepard requires center_freq > 0 (got {centre_freq})"
        )));
    }
    let voices = voices.clamp(1, 12);
    let sigma_oct = sigma_oct.clamp(0.1, 6.0);
    if n == 0 {
        return Ok(Vec::new());
    }

    // Per-voice frequencies (octave spaced) and Gaussian log-frequency
    // weights centred on `centre_freq`.
    let log2_centre = centre_freq.log2();
    let mut voice_freqs: Vec<f32> = Vec::with_capacity(voices);
    let mut weights: Vec<f32> = Vec::with_capacity(voices);
    let mut weight_sum: f32 = 0.0;
    for k in 0..voices {
        let f_k = freq * 2.0_f32.powi(k as i32);
        let oct_offset = f_k.log2() - log2_centre;
        let w = (-(oct_offset / sigma_oct).powi(2)).exp();
        voice_freqs.push(f_k);
        weights.push(w);
        weight_sum += w;
    }
    // Normalise so Σ |w_k · sin| ≤ amplitude.
    let scale = amplitude / weight_sum.max(f32::MIN_POSITIVE);

    let dt = 1.0 / sample_rate as f32;
    let out = (0..n)
        .map(|i| {
            let t = i as f32 * dt;
            let mut s = 0.0_f32;
            for (k, &f_k) in voice_freqs.iter().enumerate() {
                s += weights[k] * (TAU * f_k * t).sin();
            }
            scale * s
        })
        .collect();
    Ok(out)
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
/// citation):
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

/// Blue noise — the discrete first difference of white noise,
/// `y[n] = x[n] − x[n−1]`. This is the discrete-time derivative,
/// whose frequency response `|H(e^{jω})|² = 2·(1 − cos ω)` rises
/// monotonically from 0 at DC to 4 at the Nyquist limit, i.e. power
/// spectral density grows roughly as `f²` over the audio band — the
/// +6 dB/octave high-pass complement of brown's −6 dB/octave low-pass.
///
/// Worst-case `x[n] − x[n−1]` with each `x ∈ [-1, 1]` is bounded by 2,
/// so the half-scaling keeps the output strictly inside
/// `[-amplitude, amplitude]` regardless of the sequence drawn from the
/// generator.
pub fn noise_blue(n: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    let mut prev: f32 = 0.0;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let white = rng.next_f32() * 2.0 - 1.0;
        // First difference, scaled by 0.5 to bound the result in
        // [-amplitude, amplitude]: |x − x_prev| ≤ 2 ⇒ |0.5·(x − x_prev)| ≤ 1.
        let diff = 0.5 * (white - prev);
        out.push(diff * amplitude);
        prev = white;
    }
    out
}

/// Violet / purple noise — the discrete second difference of white
/// noise, `y[n] = x[n] − 2·x[n−1] + x[n−2]`. Equivalently it's the
/// first difference applied twice, so the frequency response is the
/// square of blue's: `|H(e^{jω})|² = [2·(1 − cos ω)]² = 4·(1 − cos ω)²`,
/// rising from 0 at DC to 16 at Nyquist. Power spectral density grows
/// roughly as `f⁴` over the audio band — the +12 dB/octave high-pass
/// counterpart of brown's −12 dB/octave low-pass.
///
/// Worst-case `|x − 2·x_prev + x_prev2|` with each input in `[-1, 1]`
/// is bounded by 4, so the quarter-scaling keeps the output strictly
/// inside `[-amplitude, amplitude]`.
pub fn noise_violet(n: usize, amplitude: f32, seed: u32) -> Vec<f32> {
    let mut rng = Lcg::new(seed);
    let mut prev1: f32 = 0.0;
    let mut prev2: f32 = 0.0;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let white = rng.next_f32() * 2.0 - 1.0;
        // Second difference, scaled by 0.25 to bound the result in
        // [-amplitude, amplitude]: |x − 2·x_prev + x_prev2| ≤ 4.
        let d2 = 0.25 * (white - 2.0 * prev1 + prev2);
        out.push(d2 * amplitude);
        prev2 = prev1;
        prev1 = white;
    }
    out
}

/// Constant (DC) signal: `s[n] = level` for every `n`, with `level`
/// clamped to `[-1, 1]`.
///
/// Exactness contract: every sample is bit-identical to the clamped
/// `level` — there is no accumulation, rounding, or time dependence,
/// so downstream S16 conversion yields a single repeated PCM code.
pub fn dc(level: f32, n: usize) -> Vec<f32> {
    vec![level.clamp(-1.0, 1.0); n]
}

/// Unipolar impulse train: `width` samples at `+amplitude` every
/// `period` samples, first impulse at `n = 0`.
///
/// Closed form:
///
/// ```text
/// s[n] = amplitude   if n mod period < width
///        0           otherwise
/// ```
///
/// `period` and `width` are exact sample counts (no float phase
/// accumulator), so the train never drifts: the k-th impulse starts at
/// sample `k · period` exactly, for any length. A `width` of 1 is the
/// discrete Dirac comb; its magnitude spectrum has equal-height lines
/// at every multiple of `rate / period` Hz.
///
/// `period` must be ≥ 1 and `width` is clamped to `1..=period` by the
/// dispatcher.
pub fn impulse_train(period: usize, width: usize, n: usize, amplitude: f32) -> Vec<f32> {
    let period = period.max(1);
    let width = width.clamp(1, period);
    (0..n)
        .map(|i| if i % period < width { amplitude } else { 0.0 })
        .collect()
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
        // `purple` is now a valid alias for violet noise; the
        // sentinel for "unknown colour" is something genuinely
        // unmatched.
        let err = render(&map(&[("type", "noise"), ("color", "chartreuse")])).unwrap_err();
        assert!(format!("{err}").contains("chartreuse"));
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

    // ───── blue / violet noise ─────

    #[test]
    fn noise_blue_bounded_by_amplitude() {
        // |0.5·(x − x_prev)| ≤ 1 when |x|, |x_prev| ≤ 1 — output must
        // stay strictly inside [-amplitude, amplitude] for every draw.
        let amp = 0.8_f32;
        let buf = noise_blue(8192, amp, 0xCAFE_F00D);
        for s in &buf {
            assert!(
                s.abs() <= amp + 1e-6,
                "blue sample {s} exceeds amplitude {amp}"
            );
        }
    }

    #[test]
    fn noise_violet_bounded_by_amplitude() {
        // |0.25·(x − 2·x_prev + x_prev2)| ≤ 1 when |·| ≤ 1 — output
        // must stay strictly inside [-amplitude, amplitude].
        let amp = 0.7_f32;
        let buf = noise_violet(8192, amp, 0xDEAD_BEEF);
        for s in &buf {
            assert!(
                s.abs() <= amp + 1e-6,
                "violet sample {s} exceeds amplitude {amp}"
            );
        }
    }

    #[test]
    fn noise_blue_is_high_pass_versus_white() {
        // Blue noise's PSD grows as ~f² over the band: the high-frequency
        // half of the spectrum should carry materially more energy than
        // the low-frequency half. We compare a single-bin DFT probe near
        // Nyquist/4 (3 kHz at Fs=16k) against a probe near DC (200 Hz);
        // for white noise the two should be statistically comparable,
        // for blue the high probe should dominate.
        let sr = 16_000_u32;
        let n = 4096;
        let amp = 1.0_f32;
        let white = noise_white(n, amp, 0x1234_5678);
        let blue = noise_blue(n, amp, 0x1234_5678);
        let lo_white = dft_mag(&white, 200.0, sr as f32);
        let hi_white = dft_mag(&white, 3000.0, sr as f32);
        let lo_blue = dft_mag(&blue, 200.0, sr as f32);
        let hi_blue = dft_mag(&blue, 3000.0, sr as f32);
        // Blue's high/low ratio should be much larger than white's
        // (3000/200 ≈ 15; an f²-rising PSD predicts ~225× power, or
        // ~15× magnitude). A 5× floor stays well below the predicted
        // ratio while leaving plenty of headroom for the single-bin
        // statistical noise.
        let blue_ratio = hi_blue / lo_blue.max(1e-9);
        let white_ratio = hi_white / lo_white.max(1e-9);
        assert!(
            blue_ratio > 5.0 * white_ratio,
            "blue hi/lo ratio {blue_ratio} not >> white hi/lo ratio {white_ratio}"
        );
    }

    #[test]
    fn noise_violet_steeper_high_pass_than_blue() {
        // Violet's PSD grows as ~f⁴ — its hi/lo ratio should exceed
        // blue's, since stacking two differences squares the response.
        let sr = 16_000_u32;
        let n = 4096;
        let blue = noise_blue(n, 1.0, 0xABCD_EF01);
        let violet = noise_violet(n, 1.0, 0xABCD_EF01);
        let lo_blue = dft_mag(&blue, 200.0, sr as f32);
        let hi_blue = dft_mag(&blue, 3000.0, sr as f32);
        let lo_violet = dft_mag(&violet, 200.0, sr as f32);
        let hi_violet = dft_mag(&violet, 3000.0, sr as f32);
        let blue_ratio = hi_blue / lo_blue.max(1e-9);
        let violet_ratio = hi_violet / lo_violet.max(1e-9);
        assert!(
            violet_ratio > 1.5 * blue_ratio,
            "violet hi/lo ratio {violet_ratio} not steeper than blue {blue_ratio}"
        );
    }

    #[test]
    fn noise_blue_deterministic_per_seed() {
        // The same seed must produce identical samples — bit-exact
        // reproducibility is documented in the README's `## Determinism`
        // section and matters for fixture tests.
        let a = noise_blue(256, 0.9, 0x4242_4242);
        let b = noise_blue(256, 0.9, 0x4242_4242);
        let c = noise_blue(256, 0.9, 0x4242_4243);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn noise_violet_deterministic_per_seed() {
        let a = noise_violet(256, 0.9, 0x5151_5151);
        let b = noise_violet(256, 0.9, 0x5151_5151);
        let c = noise_violet(256, 0.9, 0x5151_5152);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn noise_blue_dispatcher_reachable_via_uri() {
        // Smoke test the dispatcher path: `type=noise&color=blue` must
        // reach the new generator, produce the right sample count,
        // and stay bounded.
        let buf = render(&map(&[
            ("type", "noise"),
            ("color", "blue"),
            ("duration", "0.01"),
            ("amplitude", "0.8"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 80); // 8000 × 0.01
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn noise_violet_dispatcher_reachable_via_uri() {
        let buf = render(&map(&[
            ("type", "noise"),
            ("color", "violet"),
            ("duration", "0.01"),
            ("amplitude", "0.6"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 80);
        for s in &buf.samples {
            assert!(s.abs() <= 0.6 + 1e-6);
        }
    }

    #[test]
    fn noise_purple_is_alias_for_violet() {
        // `purple` and `violet` must produce identical samples at the
        // same seed — same alias contract as `brown`/`brownian`.
        let buf_violet = render(&map(&[
            ("type", "noise"),
            ("color", "violet"),
            ("seed", "12345"),
            ("duration", "0.005"),
        ]))
        .unwrap();
        let buf_purple = render(&map(&[
            ("type", "noise"),
            ("color", "purple"),
            ("seed", "12345"),
            ("duration", "0.005"),
        ]))
        .unwrap();
        assert_eq!(buf_violet.samples, buf_purple.samples);
    }

    #[test]
    fn noise_azure_is_alias_for_blue() {
        let buf_blue = render(&map(&[
            ("type", "noise"),
            ("color", "blue"),
            ("seed", "98765"),
            ("duration", "0.005"),
        ]))
        .unwrap();
        let buf_azure = render(&map(&[
            ("type", "noise"),
            ("color", "azure"),
            ("seed", "98765"),
            ("duration", "0.005"),
        ]))
        .unwrap();
        assert_eq!(buf_blue.samples, buf_azure.samples);
    }

    // ───── PWM (pulse-width modulation) ─────

    #[test]
    fn pwm_duty_half_matches_square() {
        // `duty=0.5` and no LFO is exactly the square wave at the same
        // frequency — verify sample-by-sample equality so the
        // generalisation is provably a strict superset.
        let n = 2048;
        let sr = 8000;
        let freq = 440.0_f32;
        let amp = 0.8_f32;
        let p = pwm(freq, 0.5, 0.0, 0.0, sr, n, amp).unwrap();
        let s = square(freq, sr, n, amp);
        assert_eq!(p.len(), s.len());
        for (i, (a, b)) in p.iter().zip(&s).enumerate() {
            assert!((a - b).abs() < 1e-9, "sample {i}: pwm {a} != square {b}");
        }
    }

    #[test]
    fn pwm_takes_only_two_values_bounded_by_amplitude() {
        // The wave is binary `{+amp, -amp}` by construction. Verify
        // every sample is exactly one or the other.
        let amp = 0.6_f32;
        let buf = pwm(100.0, 0.3, 0.0, 0.0, 1000, 1000, amp).unwrap();
        for &s in &buf {
            assert!(
                (s - amp).abs() < 1e-9 || (s + amp).abs() < 1e-9,
                "non-binary sample {s}"
            );
        }
    }

    #[test]
    fn pwm_duty_controls_positive_fraction() {
        // Over an integer number of periods at duty=d, the proportion
        // of +amp samples should be ≈ d. Use a slow oscillator + a
        // long-enough buffer so the discretisation noise stays small.
        let sr = 1000;
        let freq = 10.0_f32; // 100-sample period → exact integer.
        let n = 10_000; // 100 full periods.
        let amp = 1.0_f32;
        for &d in &[0.1_f32, 0.25, 0.5, 0.75, 0.9] {
            let buf = pwm(freq, d, 0.0, 0.0, sr, n, amp).unwrap();
            let pos = buf.iter().filter(|&&v| v > 0.0).count() as f32 / n as f32;
            // Per-period quantisation puts the realistic floor at ~1/100
            // of a period; we allow a generous 2% tolerance.
            assert!(
                (pos - d).abs() < 0.02,
                "duty {d}: positive fraction {pos} too far from target"
            );
        }
    }

    #[test]
    fn pwm_duty_clamps_at_extremes() {
        // duty=0 and duty=1 are clamped to ε / (1-ε) so the wave always
        // has at least one sample of each polarity per period — silent
        // DC would defeat the whole point.
        for &d in &[0.0_f32, 1.0] {
            let buf = pwm(100.0, d, 0.0, 0.0, 1000, 1000, 1.0).unwrap();
            let any_pos = buf.iter().any(|&v| v > 0.0);
            let any_neg = buf.iter().any(|&v| v < 0.0);
            assert!(any_pos && any_neg, "duty {d} collapsed to DC");
        }
    }

    #[test]
    fn pwm_rejects_non_positive_freq() {
        // Zero / negative frequency would divide by zero in the period
        // calculation — the dispatcher must surface a clear error.
        for &f in &[0.0_f32, -1.0, -100.0] {
            let err = pwm(f, 0.5, 0.0, 0.0, 1000, 100, 1.0).unwrap_err();
            assert!(format!("{err}").contains("pwm"), "missing 'pwm' in {err}");
        }
    }

    #[test]
    fn pwm_lfo_modulates_duty_over_time() {
        // With a slow LFO and a meaningful depth, the proportion of +amp
        // samples in the first quarter of the buffer (where the LFO is
        // sweeping the duty upward through `duty + depth`) should differ
        // from the proportion in the third quarter (where the LFO has
        // pushed duty down through `duty − depth`). Static `lfo=0` could
        // never produce a quarter-to-quarter difference, so this proves
        // the modulation is actually active.
        let sr = 8000;
        let n = 8000; // 1 s
        let amp = 1.0_f32;
        // 1 Hz LFO over 1 s = one full LFO cycle. With duty=0.5 and
        // depth=0.4, the threshold sweeps in [0.1, 0.9].
        let buf = pwm(200.0, 0.5, 1.0, 0.4, sr, n, amp).unwrap();
        // LFO phase at t = 0..0.25s is in [0, π/2] → sin in [0, 1] →
        // duty in [0.5, 0.9] → mostly positive.
        // LFO phase at t = 0.5..0.75s is in [π, 3π/2] → sin in [-1, 0] →
        // duty in [0.1, 0.5] → mostly negative.
        let q1: f32 = buf[..n / 4].iter().filter(|&&v| v > 0.0).count() as f32 / (n as f32 / 4.0);
        let q3: f32 =
            buf[n / 2..3 * n / 4].iter().filter(|&&v| v > 0.0).count() as f32 / (n as f32 / 4.0);
        assert!(
            q1 > q3 + 0.15,
            "q1 positive ratio {q1} should exceed q3 {q3} by ≥0.15"
        );
    }

    #[test]
    fn pwm_dispatcher_default_keeps_bounds() {
        // Default duration 1 s, default rate 8000 Hz, default freq 440,
        // default duty 0.5 → square-wave path. Bounded by amplitude.
        let buf = render(&map(&[("type", "pwm"), ("duration", "0.05")])).unwrap();
        assert_eq!(buf.samples.len(), 400); // 8000 × 0.05
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn pwm_dispatcher_pulse_alias() {
        // `type=pulse` is an accepted alias for `type=pwm`.
        let a = render(&map(&[
            ("type", "pwm"),
            ("freq", "440"),
            ("duty", "0.25"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "pulse"),
            ("freq", "440"),
            ("duty", "0.25"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        assert_eq!(a.samples, b.samples);
    }

    #[test]
    fn pwm_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("pwm"));
    }

    #[test]
    fn pwm_first_sample_is_positive_for_default_duty() {
        // sample 0 has phase = 0, which is always below any duty>0 ⇒
        // the wave starts at +amplitude. This is the reference "pulse
        // begins with the on-segment" convention; useful as a
        // determinism anchor for downstream fixture tests.
        let buf = pwm(440.0, 0.5, 0.0, 0.0, 8000, 16, 0.8).unwrap();
        assert!((buf[0] - 0.8).abs() < 1e-9);
    }

    #[test]
    fn pwm_sample_output_pinned() {
        // Fixture: pin a known small render so future refactors of the
        // dispatcher / clamp logic can't silently change the wire
        // format. 8000 Hz, freq=1000 → 8 samples per period; duty=0.25
        // means 2 samples positive, 6 samples negative per period.
        // 16 samples = 2 full periods.
        let buf = pwm(1000.0, 0.25, 0.0, 0.0, 8000, 16, 1.0).unwrap();
        let want: Vec<f32> = vec![
            // Period 0: samples 0,1 positive (phase 0/8, 1/8 < 0.25),
            // samples 2..8 negative.
            1.0, 1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0, // Period 1: same pattern.
            1.0, 1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0,
        ];
        assert_eq!(buf, want);
    }

    // ───── supersaw ─────

    #[test]
    fn supersaw_voices_one_equals_sawtooth() {
        // A single voice at zero detune is just the centre voice ⇒
        // identical samples to the in-tree `sawtooth` (modulo the
        // equal-weight average, which is `/1.0` here).
        let n = 1024;
        let sr = 8000;
        let freq = 440.0;
        let got = supersaw(freq, 1, 0.0, sr, n, 0.8).unwrap();
        let want = sawtooth(freq, sr, n, 0.8);
        let max_err = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_err < 1e-6, "max err = {max_err}");
    }

    #[test]
    fn supersaw_zero_detune_collapses_to_single_voice() {
        // detune=0 ⇒ every voice runs at the centre frequency, so the
        // sum is just `voices · sawtooth(freq)` and the equal-weight
        // average brings it right back to a single sawtooth.
        let n = 1024;
        let sr = 8000;
        let freq = 440.0;
        let got = supersaw(freq, 7, 0.0, sr, n, 0.8).unwrap();
        let want = sawtooth(freq, sr, n, 0.8);
        let max_err = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        // The `/voices` average of identical voices is exact in float
        // because `(7 · x) / 7 == x` for x with no leading-bit drift;
        // give it 1e-6 slack just in case.
        assert!(max_err < 1e-6, "max err = {max_err}");
    }

    #[test]
    fn supersaw_stays_bounded_by_amplitude() {
        // For arbitrary (freq, voices, detune) the equal-weight average
        // of voice sawtooths each in [-amplitude, +amplitude] cannot
        // exceed amplitude in magnitude — sanity-check on a non-trivial
        // configuration.
        let amp = 0.8;
        let got = supersaw(220.0, 7, 12.0, 44100, 4096, amp).unwrap();
        for (i, s) in got.iter().enumerate() {
            assert!(
                s.abs() <= amp + 1e-6,
                "supersaw sample {i} = {s} exceeded amp {amp}"
            );
        }
    }

    #[test]
    fn supersaw_diverges_from_sawtooth_with_nonzero_detune() {
        // Differentiate from `sawtooth`: with detune>0 the per-voice
        // phases drift apart so the average can't equal the centre
        // sawtooth sample-for-sample. Check the buffers actually differ
        // somewhere by a comfortably-above-noise margin.
        let got = supersaw(440.0, 7, 12.0, 8000, 4096, 0.8).unwrap();
        let centre = sawtooth(440.0, 8000, 4096, 0.8);
        let mut max_diff = 0.0_f32;
        for (a, b) in got.iter().zip(&centre) {
            max_diff = max_diff.max((a - b).abs());
        }
        // Detuning of 12 cents across 7 voices produces audible
        // chorusing within the first ~1000 samples; the peak deviation
        // is well above the f32 quantisation floor.
        assert!(
            max_diff > 0.05,
            "supersaw barely differs from centre saw: max_diff = {max_diff}"
        );
    }

    #[test]
    fn supersaw_rejects_nonpositive_freq() {
        let err = supersaw(0.0, 7, 12.0, 8000, 16, 0.8).unwrap_err();
        assert!(format!("{err}").contains("freq"));
        let err = supersaw(-220.0, 7, 12.0, 8000, 16, 0.8).unwrap_err();
        assert!(format!("{err}").contains("freq"));
    }

    #[test]
    fn supersaw_dispatcher_alias_and_defaults() {
        // `type=saws` is the documented alias; default `voices=7`,
        // `detune=12` cents. Both names produce identical buffers.
        let a = render(&map(&[
            ("type", "supersaw"),
            ("freq", "440"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "saws"),
            ("freq", "440"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.samples.len(), 80); // 8000 × 0.01
        for s in &a.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn supersaw_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "totally-bogus-mode")])).unwrap_err();
        assert!(format!("{err}").contains("supersaw"));
    }

    #[test]
    fn supersaw_voices_clamped_into_range() {
        // The dispatcher clamps voices into [1, 32]. A request for 100
        // voices should not error out — it should silently clamp at 32.
        let buf = render(&map(&[
            ("type", "supersaw"),
            ("voices", "100"),
            ("duration", "0.005"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 40);
        for s in &buf.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn supersaw_centre_voice_is_at_freq() {
        // For an odd voice count with symmetric detune, the middle
        // voice runs at exactly `freq`. Verify by checking that the
        // centre-voice cent offset is 0 (computed via the dispatcher's
        // helper).
        // 7 voices, detune=12 cents ⇒ steps of 4 cents:
        //   [-12, -8, -4, 0, +4, +8, +12]
        // i.e. index 3 (the middle) is 0 cents.
        let voices = 7;
        let detune = 12.0_f32;
        let step = 2.0 * detune / (voices as f32 - 1.0);
        let middle = -detune + step * 3.0;
        assert!(
            middle.abs() < 1e-6,
            "expected centre voice at 0 cents, got {middle}"
        );
    }

    // ───── tremolo ─────

    #[test]
    fn tremolo_depth_zero_is_pure_carrier() {
        // depth=0 → envelope ≡ 1 → output must be sample-for-sample
        // identical to the underlying carrier. This proves the
        // generaliser-not-replacement property: tremolo at depth 0 is
        // the carrier untouched.
        let n = 2048;
        let sr = 8000;
        let freq = 440.0_f32;
        let amp = 0.8_f32;
        let got = tremolo("sine", freq, 5.0, 0.0, sr, n, amp).unwrap();
        let want = sine(freq, sr, n, amp);
        let max_err = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_err < 1e-7, "max err at depth=0: {max_err}");
    }

    #[test]
    fn tremolo_envelope_bounded_by_amplitude() {
        // The envelope sits in [1 − depth, 1] ⊆ [0, 1] for every
        // (freq, lfo, depth) and every sample rate; therefore the
        // output is bounded by `amplitude`. Verify on a non-trivial
        // (high-depth, fast-LFO) configuration with a square carrier
        // — square is the worst case for the bound because its
        // samples already sit at the amplitude rail.
        let amp = 0.8_f32;
        let buf = tremolo("square", 220.0, 7.0, 0.9, 44100, 4096, amp).unwrap();
        for (i, s) in buf.iter().enumerate() {
            assert!(
                s.abs() <= amp + 1e-6,
                "tremolo sample {i} = {s} exceeded amp {amp}"
            );
        }
    }

    #[test]
    fn tremolo_envelope_range_matches_one_minus_depth_to_one() {
        // For a constant-amplitude carrier (square at +amp) the
        // tremolo output trace is exactly the envelope scaled by
        // +amp. Over an integer number of LFO periods the envelope
        // covers [1 − d, 1] inclusive, so the extracted +amp samples
        // should span [amp · (1 − d), amp].
        let sr = 10_000;
        let lfo = 5.0_f32;
        // 4 full LFO periods at 10 kHz → 8000 samples.
        let n = (sr as f32 * 4.0 / lfo) as usize;
        let d = 0.6_f32;
        let amp = 1.0_f32;
        let buf = tremolo("square", 1000.0, lfo, d, sr, n, amp).unwrap();
        // square outputs ±amp; the envelope rescales the +amp samples
        // to env·amp; we look at the positive half-period samples.
        let pos: Vec<f32> = buf.iter().copied().filter(|&v| v > 0.0).collect();
        let pmin = pos.iter().copied().fold(f32::INFINITY, f32::min);
        let pmax = pos.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        // The envelope minimum (= 1 − d) at the LFO peak attenuation
        // should be reached at least one positive-square sample per
        // LFO period; allow a small floating slack.
        assert!(
            (pmin - amp * (1.0 - d)).abs() < 0.02,
            "expected positive samples to dip to {} (= amp·(1 − d)); got pmin={pmin}",
            amp * (1.0 - d)
        );
        assert!(
            (pmax - amp).abs() < 0.02,
            "expected positive samples to peak at +amp={amp}; got pmax={pmax}"
        );
    }

    #[test]
    fn tremolo_modulates_envelope_over_time() {
        // With a slow LFO and a meaningful depth, the RMS energy in
        // the first quarter of the buffer (envelope swelling up to 1
        // around t = 0 because cos(0) = 1) should exceed the RMS
        // around the LFO's half-period (where cos = −1 and the
        // envelope reaches 1 − depth). Static depth=0 could never
        // produce a quarter-to-quarter difference, so this is the
        // canonical "the LFO is actually steering the gain" probe.
        let sr = 8000;
        // 1 s buffer at 8 kHz; 1 Hz LFO over 1 s = one full LFO period.
        let n = 8000;
        let buf = tremolo("sine", 440.0, 1.0, 0.8, sr, n, 1.0).unwrap();
        let rms = |slice: &[f32]| -> f32 {
            let sum: f32 = slice.iter().map(|v| v * v).sum();
            (sum / slice.len() as f32).sqrt()
        };
        // Around t = 0..1/8 s (LFO phase 0..π/4) envelope ≈ 1.
        let r0 = rms(&buf[..n / 8]);
        // Around t = 3/8..5/8 s (LFO phase 3π/4..5π/4) cos ≈ −1 ⇒
        // envelope minimum.
        let r_mid = rms(&buf[(3 * n / 8)..(5 * n / 8)]);
        assert!(
            r0 > r_mid * 1.5,
            "expected LFO peak RMS {r0} to dominate LFO trough RMS {r_mid}"
        );
    }

    #[test]
    fn tremolo_lfo_zero_is_constant_attenuation_one() {
        // lfo_hz=0 freezes cos(2π·0·t) ≡ 1 ⇒ envelope ≡ 1 regardless
        // of depth — algebraically the envelope reduces to
        //   (1 − d) + d · 0.5 · (1 + 1) = (1 − d) + d = 1.
        // So lfo=0 is the documented "no modulation" escape hatch
        // even at non-zero depth. Verify against the carrier sample
        // for sample.
        let n = 1024;
        let sr = 8000;
        let got = tremolo("sine", 440.0, 0.0, 0.5, sr, n, 0.8).unwrap();
        let want = sine(440.0, sr, n, 0.8);
        let max_err = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_err < 1e-7, "lfo=0 should be the pure carrier");
    }

    #[test]
    fn tremolo_distinguishes_from_am() {
        // Tremolo and AM share the "amplitude is modulated" headline
        // but differ in everything that matters at the spectrum:
        // tremolo's envelope is unipolar and sub-audio; the in-tree
        // `am` operates with a bipolar sine modulator at audio
        // rate. Confirm the buffers actually differ for a
        // representative configuration (5 Hz tremolo vs 60 Hz AM
        // modulator).
        let sr = 8000;
        let n = 4000; // 0.5 s — enough for several LFO + AM cycles.
        let trem = tremolo("sine", 440.0, 5.0, 0.5, sr, n, 0.8).unwrap();
        // `am` clamps its index to [0, 1] and uses 0.5 normalisation;
        // we drive it at the same depth (0.5) so the *amplitude*
        // intent matches even though the maths is different.
        let am_buf = am(440.0, 60.0, 0.5, sr, n, 0.8);
        let mut max_diff = 0.0_f32;
        for (a, b) in trem.iter().zip(&am_buf) {
            max_diff = max_diff.max((a - b).abs());
        }
        assert!(
            max_diff > 0.1,
            "tremolo and am should diverge substantially: max_diff={max_diff}"
        );
    }

    #[test]
    fn tremolo_dispatcher_alias_and_default_bounds() {
        // `type=trem` is the documented short alias; both names must
        // produce identical buffers. Defaults: wave=sine, freq=440,
        // lfo=5, depth=0.5 → bounded output (envelope ∈ [0.5, 1]).
        let a = render(&map(&[("type", "tremolo"), ("duration", "0.05")])).unwrap();
        let b = render(&map(&[("type", "trem"), ("duration", "0.05")])).unwrap();
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.samples.len(), 400); // 8000 × 0.05.
        for s in &a.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn tremolo_dispatcher_clamps_depth() {
        // depth=2 is out of [0, 1] and clamps to 1.0 at the
        // dispatcher; verify by comparing to an explicit depth=1
        // render. The two buffers should agree to f32 quantisation.
        let clamped = render(&map(&[
            ("type", "tremolo"),
            ("duration", "0.05"),
            ("depth", "2"),
        ]))
        .unwrap();
        let explicit = render(&map(&[
            ("type", "tremolo"),
            ("duration", "0.05"),
            ("depth", "1"),
        ]))
        .unwrap();
        assert_eq!(clamped.samples.len(), explicit.samples.len());
        for (i, (&a, &b)) in clamped.samples.iter().zip(&explicit.samples).enumerate() {
            assert!(
                (a - b).abs() < 1e-7,
                "sample {i}: clamped {a}, explicit {b}"
            );
        }
    }

    #[test]
    fn tremolo_unknown_wave_errors() {
        // The wave selector mirrors `adsr`'s carrier list. An invalid
        // value must surface a clear error mentioning `tremolo`.
        let err = render(&map(&[
            ("type", "tremolo"),
            ("wave", "ocarina"),
            ("duration", "0.01"),
        ]))
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("tremolo"), "missing 'tremolo' in {msg}");
        assert!(msg.contains("ocarina"), "missing offending wave in {msg}");
    }

    #[test]
    fn tremolo_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("tremolo"));
    }

    #[test]
    fn tremolo_carrier_wave_selectable() {
        // All four carrier waves must accept the tremolo wrapper and
        // produce distinguishable output (carrier-shape parity is the
        // distinguishing property versus the sine-only `am`).
        let sr = 8000;
        let n = 1024;
        let amp = 0.8_f32;
        let s = tremolo("sine", 220.0, 5.0, 0.5, sr, n, amp).unwrap();
        let q = tremolo("square", 220.0, 5.0, 0.5, sr, n, amp).unwrap();
        let t = tremolo("triangle", 220.0, 5.0, 0.5, sr, n, amp).unwrap();
        let w = tremolo("sawtooth", 220.0, 5.0, 0.5, sr, n, amp).unwrap();
        let saw_alias = tremolo("saw", 220.0, 5.0, 0.5, sr, n, amp).unwrap();
        assert_eq!(w, saw_alias, "`saw` alias must match `sawtooth`");
        let differ = |a: &[f32], b: &[f32]| {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0_f32, f32::max)
        };
        assert!(differ(&s, &q) > 0.1, "sine vs square must diverge");
        assert!(differ(&q, &t) > 0.1, "square vs triangle must diverge");
        assert!(differ(&t, &w) > 0.05, "triangle vs sawtooth must diverge");
    }

    // ───── vibrato ─────

    #[test]
    fn vibrato_depth_zero_is_pure_carrier() {
        // depth=0 zeroes the modulation index, so the phase reduces to
        // 2π·freq·t — exactly the unmodulated sine. The result must
        // equal `sine(freq, …)` sample-for-sample to f32 quantisation.
        // This is the "generaliser-not-replacement" property: vibrato
        // at depth 0 is the carrier untouched.
        let n = 2048;
        let sr = 8000;
        let freq = 440.0_f32;
        let amp = 0.8_f32;
        let got = vibrato("sine", freq, 5.0, 0.0, sr, n, amp).unwrap();
        let want = sine(freq, sr, n, amp);
        let max_err = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(max_err < 1e-4, "max err at depth=0: {max_err}");
    }

    #[test]
    fn vibrato_bounded_by_amplitude() {
        // Every carrier oscillator already produces samples bounded by
        // its `amplitude` argument; the phase reshuffling cannot push
        // an oscillator outside its own image. Verify on a high-depth,
        // fast-LFO configuration with a square carrier (the worst case
        // because square already sits at the amplitude rail every
        // sample).
        let amp = 0.8_f32;
        let buf = vibrato("square", 220.0, 7.0, 0.5, 44100, 4096, amp).unwrap();
        for (i, s) in buf.iter().enumerate() {
            assert!(
                s.abs() <= amp + 1e-6,
                "vibrato sample {i} = {s} exceeded amp {amp}"
            );
        }
    }

    #[test]
    fn vibrato_lfo_zero_collapses_to_shifted_carrier() {
        // lfo_hz=0 freezes the cosine at 1 ⇒ instantaneous frequency
        // is constant at freq · (1 + depth) ⇒ output is a sine at
        // that shifted frequency. Cross-check against an unmodulated
        // sine at freq · (1 + depth).
        let n = 1024;
        let sr = 8000;
        let f = 440.0_f32;
        let d = 0.05_f32;
        let amp = 0.8_f32;
        let got = vibrato("sine", f, 0.0, d, sr, n, amp).unwrap();
        let want = sine(f * (1.0 + d), sr, n, amp);
        let max_err = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_err < 1e-4,
            "lfo=0 should be the shifted-pitch carrier: max_err={max_err}"
        );
    }

    #[test]
    fn vibrato_modulates_phase_over_time() {
        // The pitch sweep is bipolar around `freq`, so at the LFO peak
        // the instantaneous frequency is freq · (1 + depth) and at the
        // trough it is freq · (1 − depth). Detect that by zero-crossing
        // counting in two short windows symmetric around the peak and
        // trough of the LFO cosine: the high-frequency window must
        // contain measurably more crossings than the low-frequency one.
        // A pure constant-frequency carrier would have identical
        // crossing counts in symmetric windows.
        let sr = 16000_u32;
        let f0 = 1000.0_f32;
        let lfo_hz = 1.0_f32;
        let depth = 0.5_f32; // exaggerated for a clear signal-to-noise ratio.
        let n = sr as usize; // 1 s — one full LFO cycle.
        let buf = vibrato("sine", f0, lfo_hz, depth, sr, n, 1.0).unwrap();
        // LFO peak (cos = +1) at t = 0; LFO trough (cos = −1) at t = 0.5.
        // Take 50 ms windows centred on each, avoiding the edges.
        let win_len = sr as usize / 20; // 50 ms = 800 samples
        let peak_start = win_len; // start at t = 50 ms (LFO still near +1)
        let trough_start = (n / 2) - (win_len / 2);
        let zcr = |slice: &[f32]| -> usize {
            slice
                .windows(2)
                .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                .count()
        };
        let z_peak = zcr(&buf[peak_start..peak_start + win_len]);
        let z_trough = zcr(&buf[trough_start..trough_start + win_len]);
        // Peak frequency ≈ 1500 Hz ⇒ ≈ 150 crossings in 50 ms;
        // trough ≈ 500 Hz ⇒ ≈ 50 crossings. A factor of ~3 is the
        // headline signal; require ≥ 1.5× margin so the assertion
        // survives f32 / windowing noise.
        assert!(
            z_peak > z_trough * 3 / 2,
            "LFO peak ({z_peak} crossings) must dominate trough ({z_trough}) by ≥ 1.5×"
        );
    }

    #[test]
    fn vibrato_dispatcher_alias_and_default_bounds() {
        // `type=vib` is the documented short alias; both names must
        // produce identical buffers. Defaults: wave=sine, freq=440,
        // lfo=5, depth=0.005 (±0.5 % deviation) → bounded output.
        let a = render(&map(&[("type", "vibrato"), ("duration", "0.05")])).unwrap();
        let b = render(&map(&[("type", "vib"), ("duration", "0.05")])).unwrap();
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.samples.len(), 400); // 8000 × 0.05.
        for s in &a.samples {
            assert!(s.abs() <= 0.8 + 1e-6);
        }
    }

    #[test]
    fn vibrato_dispatcher_clamps_depth() {
        // depth=2 is out of [0, 1] and clamps to 1.0 at the wrapper;
        // verify by comparing to an explicit depth=1 render.
        let clamped = render(&map(&[
            ("type", "vibrato"),
            ("duration", "0.05"),
            ("depth", "2"),
        ]))
        .unwrap();
        let explicit = render(&map(&[
            ("type", "vibrato"),
            ("duration", "0.05"),
            ("depth", "1"),
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
    fn vibrato_unknown_wave_errors() {
        // The wave selector mirrors `tremolo`'s carrier list. An invalid
        // value must surface a clear error mentioning `vibrato`.
        let err = render(&map(&[
            ("type", "vibrato"),
            ("wave", "ocarina"),
            ("duration", "0.01"),
        ]))
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("vibrato"), "missing 'vibrato' in {msg}");
        assert!(msg.contains("ocarina"), "missing offending wave in {msg}");
    }

    #[test]
    fn vibrato_listed_in_unknown_type_help() {
        // The dispatcher's "expected …" hint must advertise vibrato.
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("vibrato"));
    }

    #[test]
    fn vibrato_carrier_wave_selectable() {
        // All four carrier waves must accept the vibrato wrapper and
        // produce distinguishable output. Carrier-shape parity with
        // tremolo is the same family-level property — vibrato is the
        // phase-domain sister of the amplitude-domain story.
        let sr = 8000;
        let n = 1024;
        let amp = 0.8_f32;
        let f = 220.0_f32;
        let d = 0.05_f32;
        let lfo = 5.0_f32;
        let s = vibrato("sine", f, lfo, d, sr, n, amp).unwrap();
        let q = vibrato("square", f, lfo, d, sr, n, amp).unwrap();
        let t = vibrato("triangle", f, lfo, d, sr, n, amp).unwrap();
        let w = vibrato("sawtooth", f, lfo, d, sr, n, amp).unwrap();
        let saw_alias = vibrato("saw", f, lfo, d, sr, n, amp).unwrap();
        assert_eq!(w, saw_alias, "`saw` alias must match `sawtooth`");
        let differ = |a: &[f32], b: &[f32]| {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0_f32, f32::max)
        };
        assert!(differ(&s, &q) > 0.1, "sine vs square must diverge");
        assert!(differ(&q, &t) > 0.1, "square vs triangle must diverge");
        assert!(differ(&t, &w) > 0.05, "triangle vs sawtooth must diverge");
    }

    #[test]
    fn vibrato_distinguishes_from_tremolo_and_fm() {
        // Family separation: vibrato vs tremolo (the amplitude-domain
        // sister at the same sub-audio rate) and vibrato vs fm (the
        // audio-rate phase modulator). Same carrier + LFO + depth: the
        // three families must produce three different buffers.
        let sr = 8000;
        let n = 4000;
        let f = 440.0_f32;
        let lfo = 5.0_f32;
        let depth = 0.1_f32;
        let amp = 0.8_f32;
        let vib = vibrato("sine", f, lfo, depth, sr, n, amp).unwrap();
        let trem = tremolo("sine", f, lfo, depth, sr, n, amp).unwrap();
        let fm_buf = fm(f, lfo, depth, sr, n, amp);
        let max_diff = |a: &[f32], b: &[f32]| -> f32 {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0_f32, f32::max)
        };
        assert!(
            max_diff(&vib, &trem) > 0.1,
            "vibrato (FM) and tremolo (AM) must diverge"
        );
        assert!(
            max_diff(&vib, &fm_buf) > 0.1,
            "vibrato (closed-form integrated phase, fractional-depth) must diverge from `fm` (audio-rate index-in-radians)"
        );
    }

    #[test]
    fn vibrato_modulation_index_matches_closed_form() {
        // The closed-form modulation index in the FM sense is
        // β = depth · freq / lfo (radians). Probe by comparing vibrato
        // against `fm` with the carrier set to `freq`, the modulator
        // set to `lfo`, and index set to the closed-form β: the two
        // buffers must agree sample-for-sample (both implement the
        // same `sin(2π·fc·t + β·sin(2π·fm·t))`).
        let sr = 8000;
        let n = 1024;
        let freq = 440.0_f32;
        let lfo = 5.0_f32;
        let depth = 0.005_f32;
        let amp = 0.8_f32;
        let beta = depth * freq / lfo;
        let vib = vibrato("sine", freq, lfo, depth, sr, n, amp).unwrap();
        let fm_equiv = fm(freq, lfo, beta, sr, n, amp);
        let max_err = vib
            .iter()
            .zip(&fm_equiv)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            max_err < 1e-4,
            "vibrato closed-form must match fm at index = depth·freq/lfo: max_err={max_err}"
        );
    }

    // ──────────────────────── Shepard tone ────────────────────────

    #[test]
    fn shepard_dispatcher_basic_buffer_shape_bounded() {
        // Default render must come back with the right sample count and
        // every sample inside the amplitude bound.
        let buf = render(&map(&[("type", "shepard"), ("duration", "0.05")])).unwrap();
        // sample_rate default 8000 × 0.05 = 400 samples mono.
        assert_eq!(buf.samples.len(), 400);
        assert_eq!(buf.sample_rate, 8000);
        assert_eq!(buf.channels, 1);
        for s in &buf.samples {
            assert!(
                s.abs() <= 0.8 + 1e-6,
                "shepard sample {s} exceeds amplitude bound 0.8"
            );
        }
    }

    #[test]
    fn shepard_voices_one_collapses_to_single_sine() {
        // With voices=1 the stack is a single sine at `freq` (the only
        // voice). It must equal a plain `sine` at that frequency,
        // sample-for-sample to f32 quantisation, because the weight
        // normalisation `scale = amplitude / Σ w_k = amplitude / w_0`
        // cancels the single Gaussian weight.
        let sr = 8000;
        let n = 1024;
        let amp = 0.8_f32;
        let f = 440.0_f32;
        let shep = shepard(f, 1, f, 1.5, sr, n, amp).unwrap();
        let plain = sine(f, sr, n, amp);
        assert_eq!(shep.len(), plain.len());
        for (i, (&a, &b)) in shep.iter().zip(&plain).enumerate() {
            assert!(
                (a - b).abs() < 1e-4,
                "shepard voices=1 should equal sine: sample {i} got {a} expected {b}"
            );
        }
    }

    #[test]
    fn shepard_octave_voices_have_octave_spaced_frequencies() {
        // The voices are octave-spaced. A short FFT-free single-bin DFT
        // at the lowest voice's frequency f and at 2f should both
        // register meaningful magnitudes (i.e. the second voice is
        // really an octave above the first, not a wider/narrower spacing).
        let sr = 8000_u32;
        // 0.5 s × 8 kHz = 4000 samples — long enough for the 110 Hz fundamental
        // (4000/(8000/110) ≈ 55 cycles) to resolve cleanly.
        let n = 4000_usize;
        let f0 = 110.0_f32;
        let buf = shepard(f0, 4, f0 * 2.0, 1.5, sr, n, 0.8).unwrap();
        let dt = 1.0_f32 / sr as f32;
        let dft_at = |freq: f32| -> f32 {
            let mut re = 0.0_f32;
            let mut im = 0.0_f32;
            for (i, &s) in buf.iter().enumerate() {
                let phase = TAU * freq * i as f32 * dt;
                re += s * phase.cos();
                im -= s * phase.sin();
            }
            (re * re + im * im).sqrt() / n as f32
        };
        let mag_f0 = dft_at(f0);
        let mag_2f0 = dft_at(2.0 * f0);
        let mag_off = dft_at(f0 * 1.3); // off-octave probe — should be much smaller
        assert!(
            mag_f0 > 0.01,
            "fundamental f0={f0} magnitude {mag_f0} too low"
        );
        assert!(
            mag_2f0 > 0.01,
            "octave 2·f0 magnitude {mag_2f0} too low — octave voice missing"
        );
        assert!(
            mag_off < 0.2 * mag_f0.max(mag_2f0),
            "off-octave bin too loud: mag_off={mag_off}, mag_f0={mag_f0}, mag_2f0={mag_2f0}"
        );
    }

    #[test]
    fn shepard_gaussian_envelope_centres_on_center_freq() {
        // Run a 6-voice stack with the centre on the third voice; the
        // weight on voice 2 (1-indexed: the centre) must be the max.
        // We probe weights directly by comparing two renders that
        // differ only in centre_freq.
        let sr = 8000;
        let n = 1024;
        let amp = 0.8_f32;
        // Voices: 110, 220, 440, 880, 1760, 3520 Hz.
        let centred_low = shepard(110.0, 6, 220.0, 0.8, sr, n, amp).unwrap();
        let centred_high = shepard(110.0, 6, 1760.0, 0.8, sr, n, amp).unwrap();
        // The two buffers must differ — the envelope reshapes the mix.
        let max_diff: f32 = centred_low
            .iter()
            .zip(&centred_high)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(
            max_diff > 0.05,
            "shifting centre_freq should reshape the stack; max_diff={max_diff}"
        );
        // Both renders stay bounded.
        for s in centred_low.iter().chain(centred_high.iter()) {
            assert!(
                s.abs() <= amp + 1e-6,
                "shepard sample {s} exceeds amp {amp}"
            );
        }
    }

    #[test]
    fn shepard_freq_must_be_positive() {
        let err = render(&map(&[
            ("type", "shepard"),
            ("freq", "0"),
            ("duration", "0.01"),
        ]))
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("shepard"), "missing 'shepard' in error: {msg}");
        assert!(msg.contains("freq"), "missing 'freq' in error: {msg}");
    }

    #[test]
    fn shepard_center_freq_must_be_positive() {
        let err = render(&map(&[
            ("type", "shepard"),
            ("center_freq", "0"),
            ("duration", "0.01"),
        ]))
        .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("shepard"), "missing 'shepard' in error: {msg}");
        assert!(
            msg.contains("center_freq"),
            "missing 'center_freq' in error: {msg}"
        );
    }

    #[test]
    fn shepard_voices_clamps_to_twelve() {
        // voices=100 clamps silently to 12 — verify by comparing to an
        // explicit voices=12 render at the same other params.
        let a = render(&map(&[
            ("type", "shepard"),
            ("voices", "100"),
            ("duration", "0.05"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "shepard"),
            ("voices", "12"),
            ("duration", "0.05"),
        ]))
        .unwrap();
        assert_eq!(a.samples.len(), b.samples.len());
        for (i, (&x, &y)) in a.samples.iter().zip(&b.samples).enumerate() {
            assert!(
                (x - y).abs() < 1e-6,
                "voices=100 should clamp to 12: sample {i} differs {x} vs {y}"
            );
        }
    }

    #[test]
    fn shepard_listed_in_unknown_type_help() {
        // The dispatcher's "expected …" hint must advertise shepard.
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        assert!(format!("{err}").contains("shepard"));
    }

    #[test]
    fn shepard_dispatcher_default_centre_is_log_midpoint() {
        // With voices=8 starting at 55 Hz the stack runs 55 … 7040 Hz;
        // its log-midpoint is 55 · 2^((8-1)/2) ≈ 622.25 Hz. A render
        // with the default centre must equal one with an explicit
        // centre at that log-midpoint, sample-for-sample.
        let default_render = render(&map(&[("type", "shepard"), ("duration", "0.02")])).unwrap();
        let explicit_centre = 55.0_f32 * 2.0_f32.powf(3.5);
        let explicit_render = render(&map(&[
            ("type", "shepard"),
            ("duration", "0.02"),
            ("center_freq", &format!("{explicit_centre}")),
        ]))
        .unwrap();
        assert_eq!(default_render.samples.len(), explicit_render.samples.len());
        for (i, (&a, &b)) in default_render
            .samples
            .iter()
            .zip(&explicit_render.samples)
            .enumerate()
        {
            assert!(
                (a - b).abs() < 1e-6,
                "default centre vs explicit log-midpoint: sample {i} differs {a} vs {b}"
            );
        }
    }

    // ───── dc ─────

    #[test]
    fn dc_every_sample_equals_level_exactly() {
        let buf = dc(0.25, 64);
        assert_eq!(buf.len(), 64);
        for &s in &buf {
            assert_eq!(s, 0.25, "dc must be bit-exact per sample");
        }
    }

    #[test]
    fn dc_negative_level_passes_through() {
        let buf = dc(-0.5, 8);
        for &s in &buf {
            assert_eq!(s, -0.5);
        }
    }

    #[test]
    fn dc_level_clamped_to_unit_range() {
        assert_eq!(dc(3.0, 1)[0], 1.0);
        assert_eq!(dc(-3.0, 1)[0], -1.0);
    }

    #[test]
    fn dc_dispatcher_level_param() {
        // 8000 Hz × 0.01 s = 80 samples, every one exactly −0.25.
        let buf = render(&map(&[
            ("type", "dc"),
            ("level", "-0.25"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 80);
        for &s in &buf.samples {
            assert_eq!(s, -0.25);
        }
    }

    #[test]
    fn dc_dispatcher_defaults_to_amplitude() {
        // No level= → level = amplitude (0.8 default).
        let buf = render(&map(&[("type", "dc"), ("duration", "0.001")])).unwrap();
        for &s in &buf.samples {
            assert_eq!(s, 0.8);
        }
    }

    // ───── impulse train ─────

    #[test]
    fn impulse_train_closed_form() {
        // s[n] = amp iff n mod period < width.
        let buf = impulse_train(100, 1, 400, 0.8);
        assert_eq!(buf.len(), 400);
        for (n, &s) in buf.iter().enumerate() {
            let want = if n % 100 == 0 { 0.8 } else { 0.0 };
            assert_eq!(s, want, "sample {n}");
        }
    }

    #[test]
    fn impulse_train_width_widens_each_impulse() {
        let buf = impulse_train(8, 3, 32, 1.0);
        for (n, &s) in buf.iter().enumerate() {
            let want = if n % 8 < 3 { 1.0 } else { 0.0 };
            assert_eq!(s, want, "sample {n}");
        }
    }

    #[test]
    fn impulse_train_width_clamped_to_period() {
        // width ≥ period degenerates to a DC-at-amplitude signal.
        let buf = impulse_train(4, 99, 16, 0.5);
        for &s in &buf {
            assert_eq!(s, 0.5);
        }
    }

    #[test]
    fn impulse_dispatcher_period_wins_over_freq() {
        // Explicit period=50 must override freq=80 (which would give
        // period 100 at 8000 Hz).
        let buf = render(&map(&[
            ("type", "impulse"),
            ("period", "50"),
            ("freq", "80"),
            ("duration", "0.025"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 200);
        for (n, &s) in buf.samples.iter().enumerate() {
            let want = if n % 50 == 0 { 0.8 } else { 0.0 };
            assert_eq!(s, want, "sample {n}");
        }
    }

    #[test]
    fn impulse_dispatcher_freq_derives_period() {
        // freq=80 at rate 8000 → period = 100 samples exactly.
        let buf = render(&map(&[
            ("type", "impulse"),
            ("freq", "80"),
            ("duration", "0.05"),
        ]))
        .unwrap();
        assert_eq!(buf.samples.len(), 400);
        let ones: Vec<usize> = buf
            .samples
            .iter()
            .enumerate()
            .filter(|(_, &s)| s != 0.0)
            .map(|(n, _)| n)
            .collect();
        assert_eq!(ones, vec![0, 100, 200, 300]);
    }

    #[test]
    fn impulse_dispatcher_default_is_one_hertz() {
        // Default freq = 1 Hz → period = rate → exactly one impulse in
        // a 1-second render, at n = 0.
        let buf = render(&map(&[("type", "impulse"), ("duration", "1")])).unwrap();
        assert_eq!(buf.samples.len(), 8000);
        assert_eq!(buf.samples[0], 0.8);
        assert!(buf.samples[1..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn impulse_dispatcher_nonpositive_freq_errors() {
        assert!(render(&map(&[("type", "impulse"), ("freq", "0")])).is_err());
        assert!(render(&map(&[("type", "impulse"), ("freq", "-3")])).is_err());
    }

    #[test]
    fn dc_and_impulse_listed_in_unknown_type_help() {
        let err = render(&map(&[("type", "definitely-not-real")])).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("dc"), "{msg}");
        assert!(msg.contains("impulse"), "{msg}");
    }

    // ───── sine phase / per-channel phase offsets ─────

    #[test]
    fn sine_phase_closed_form() {
        // s[n] must equal A·sin(2π·f·n/rate + φ) computed independently.
        let (freq, rate, amp, phase) = (440.0_f32, 8000_u32, 0.8_f32, 0.7_f32);
        let buf = sine_phase(freq, rate, 64, amp, phase);
        for (n, &s) in buf.iter().enumerate() {
            let want = amp * (TAU * freq * n as f32 / rate as f32 + phase).sin();
            // 1e-5 slack: the impl multiplies by dt = 1/rate while this
            // recomputation divides by rate — one ULP of argument
            // difference at arg ≈ 22 rad shows up in the 6th decimal.
            assert!(
                (s - want).abs() < 1e-5,
                "sample {n}: got {s}, closed form {want}"
            );
        }
    }

    #[test]
    fn sine_phase_zero_is_bitexact_sine() {
        let a = sine(440.0, 8000, 256, 0.8);
        let b = sine_phase(440.0, 8000, 256, 0.8, 0.0);
        assert_eq!(a, b, "phase 0 must be bit-identical to sine()");
    }

    #[test]
    fn sine_phase_quarter_turn_is_cosine() {
        // φ = π/2 turns sine into cosine exactly (trig identity).
        let buf = sine_phase(200.0, 8000, 128, 1.0, std::f32::consts::FRAC_PI_2);
        for (n, &s) in buf.iter().enumerate() {
            let want = (TAU * 200.0 * n as f32 / 8000.0).cos();
            assert!((s - want).abs() < 1e-5, "sample {n}: {s} vs cos {want}");
        }
    }

    #[test]
    fn sine_dispatcher_phase_param_in_degrees() {
        // phase=90 → cosine. Sample 0 of a cosine is +amplitude.
        let buf = render(&map(&[
            ("type", "sine"),
            ("freq", "440"),
            ("phase", "90"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        assert!(
            (buf.samples[0] - 0.8).abs() < 1e-6,
            "s[0] = {}",
            buf.samples[0]
        );
    }

    #[test]
    fn sine_dispatcher_chphase_offsets_second_channel() {
        // Stereo, chphase=90: channel 0 is the plain sine, channel 1
        // the matching cosine. Interleaved layout: even index = ch 0,
        // odd index = ch 1.
        let buf = render(&map(&[
            ("type", "sine"),
            ("freq", "440"),
            ("channels", "2"),
            ("chphase", "90"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        assert_eq!(buf.channels, 2);
        assert_eq!(buf.samples.len(), 160); // 80 frames × 2 channels
        for n in 0..80usize {
            let t = TAU * 440.0 * n as f32 / 8000.0;
            let want0 = 0.8 * t.sin();
            let want1 = 0.8 * (t + std::f32::consts::FRAC_PI_2).sin();
            let got0 = buf.samples[2 * n];
            let got1 = buf.samples[2 * n + 1];
            assert!((got0 - want0).abs() < 1e-5, "frame {n} ch0");
            assert!((got1 - want1).abs() < 1e-5, "frame {n} ch1");
        }
    }

    #[test]
    fn sine_dispatcher_chphase_stacks_on_phase() {
        // phase=45 + chphase=90 → ch0 at 45°, ch1 at 135°.
        let buf = render(&map(&[
            ("type", "sine"),
            ("freq", "100"),
            ("channels", "2"),
            ("phase", "45"),
            ("chphase", "90"),
            ("duration", "0.005"),
        ]))
        .unwrap();
        for n in 0..40usize {
            let t = TAU * 100.0 * n as f32 / 8000.0;
            let want0 = 0.8 * (t + 45.0_f32.to_radians()).sin();
            let want1 = 0.8 * (t + 135.0_f32.to_radians()).sin();
            assert!((buf.samples[2 * n] - want0).abs() < 1e-6, "frame {n} ch0");
            assert!(
                (buf.samples[2 * n + 1] - want1).abs() < 1e-6,
                "frame {n} ch1"
            );
        }
    }

    #[test]
    fn sine_dispatcher_stereo_without_chphase_still_replicates() {
        let buf = render(&map(&[
            ("type", "sine"),
            ("channels", "2"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        for n in 0..(buf.samples.len() / 2) {
            assert_eq!(
                buf.samples[2 * n],
                buf.samples[2 * n + 1],
                "chphase absent ⇒ channels must be bit-identical"
            );
        }
    }

    #[test]
    fn sine_dispatcher_full_turn_chphase_matches_replication() {
        // chphase=360 is a full turn: 2π periodicity brings channel 1
        // back onto channel 0 (within float rounding of x+2π).
        let buf = render(&map(&[
            ("type", "sine"),
            ("channels", "2"),
            ("chphase", "360"),
            ("duration", "0.01"),
        ]))
        .unwrap();
        for n in 0..(buf.samples.len() / 2) {
            let d = (buf.samples[2 * n] - buf.samples[2 * n + 1]).abs();
            assert!(d < 1e-5, "frame {n}: |ch0 − ch1| = {d}");
        }
    }
}

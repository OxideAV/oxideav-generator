# oxideav-generator

Pure-Rust synthetic media generator for the oxideav framework. Provides
audio synth (sine / square / triangle / sawtooth / Karplus-Strong pluck /
linear + exponential chirp / FM / AM / ring modulation / DTMF touch-tones /
ADSR-enveloped tone / Klatt-style two-formant vowel synthesizer /
multi-tone / white-pink-brown-blue-violet noise / silence),
image basics (solid colour, linear / radial gradient,
checkerboard, horizontal / vertical stripes), procedural imagery
(Mandelbrot + Julia fractals, plasma, Perlin noise), and video
(ffmpeg-style `testsrc`, SMPTE colour bars, animated Mandelbrot zoom,
hue-rotating gradient, zone-plate `cos(k·r²)` spatial-frequency probe).

Two integration shapes are exposed:

1. **Source driver** — `generate://...` URIs, registered through the
   standard `SourceRegistry`. Opening one returns a
   `SourceOutput::Frames` handle (`Box<dyn FrameSource>`) — frames are
   produced natively (audio: one `AudioFrame` per call until the
   configured duration is exhausted; image: a single still `VideoFrame`
   followed by `Eof`; video: one `VideoFrame` per call until the
   configured frame count is exhausted). Both audio and video URI
   inputs are supported end-to-end; `generate://testsrc?…` no longer
   bails with `Unsupported`.
2. **Zero-input filter** — every generator is also exposed as a
   `StreamFilter` factory (`audio.synth`, `image.xc`, …,
   `video.testsrc`, …) that emits frames in `flush()` without any
   upstream input.

Dependency-only on `oxideav-core` and `serde_json` — no `image`, no
`png`, no `wav` crate, no `rand`. LCG / Perlin / diamond-square are all
hand-rolled in tree. (Earlier rounds shipped hand-rolled WAV / PNG
encoders for the byte-shaped URI path; those are gone now that the URI
path produces frames natively.)

## URI catalogue

```
generate://synth?type=sine&freq=440&duration=5
generate://synth?type=square&freq=220&duration=2&amplitude=0.5
generate://synth?type=pluck&freq=440&decay=0.99&duration=3
generate://synth?type=chirp&shape=linear&f0=200&f1=4000&duration=4
generate://synth?type=chirp&shape=exp&f0=20&f1=20000&duration=4
generate://synth?type=fm&carrier=440&modulator=110&index=5&duration=2
generate://synth?type=am&carrier=440&modulator=60&index=0.5&duration=2
generate://synth?type=ringmod&f1=440&f2=60&duration=2
generate://synth?type=dtmf&digits=0123456789&tone=0.1&gap=0.05
generate://synth?type=adsr&wave=sine&freq=440&attack=0.02&decay=0.1&sustain=0.7&release=0.2&duration=2
generate://synth?type=formant&vowel=A&f0=220&duration=0.5
generate://synth?type=multitone&freqs=440,1000,2200&duration=1
generate://synth?type=noise&color=pink&duration=10
generate://synth?type=noise&color=blue&seed=42&duration=10
generate://synth?type=noise&color=violet&seed=42&duration=10

generate://xc?color=red&w=640&h=480
generate://xc?color=%23ff0000      # #ff0000 percent-encoded
generate://gradient?w=640&h=480&from=red&to=blue&direction=horizontal
generate://gradient?w=640&h=480&from=red&to=blue&type=radial
generate://pattern?type=checkerboard&w=640&h=480&size=32
generate://fractal?type=mandelbrot&w=640&h=480&cx=-0.5&cy=0&zoom=2&iter=256
generate://fractal?type=julia&w=640&h=480&cx=-0.7&cy=0.27&iter=256
generate://plasma?w=640&h=480&seed=42
generate://noise?type=perlin&w=640&h=480&scale=64&seed=42

generate://testsrc?w=640&h=480&duration=5&fps=30
generate://smptebars?w=640&h=480&duration=5&fps=30
generate://zoneplate?w=640&h=480&duration=5&fps=30&k=0.05&motion=temporal
```

## CLI shorthands (convert verb only)

The convert verb's arg parser runs every input through
`oxideav_generator::shorthand::translate` before reaching the source
registry. Recognised prefixes:

| Shorthand              | Canonical                                                    |
| ---------------------- | ------------------------------------------------------------ |
| `xc:red`               | `generate://xc?color=red`                                    |
| `xc:#ff0000`           | `generate://xc?color=%23ff0000`                              |
| `pattern:checkerboard` | `generate://pattern?type=checkerboard`                       |
| `gradient:red-blue`    | `generate://gradient?from=red&to=blue`                       |
| `radial:red-blue`      | `generate://gradient?type=radial&from=red&to=blue`           |
| `plasma:`              | `generate://plasma`                                          |
| `mandelbrot:`          | `generate://fractal?type=mandelbrot`                         |
| `julia:`               | `generate://fractal?type=julia`                              |
| `synth:5,sine,440`     | `generate://synth?duration=5&type=sine&freq=440`             |
| `testsrc:`             | `generate://testsrc`                                         |
| `smptebars:`           | `generate://smptebars`                                       |
| `zoneplate:`           | `generate://zoneplate`                                       |
| `noise:perlin`         | `generate://noise?type=perlin`                               |

`probe` / `transcode` / `remux` / `run` accept the canonical
`generate://` URI form only — they don't expand shorthands.

## Wiring

```rust,ignore
use oxideav_core::{RuntimeContext, SourceRegistry};

let mut ctx = RuntimeContext::new();
oxideav_source::register(&mut ctx);                      // file://
oxideav_generator::register_source(&mut ctx.sources);    // generate://
oxideav_generator::register_filters(&mut ctx);           // audio.synth, image.xc, ...
```

## Status

Round 9 (2026-05-29): synth `noise` catalogue gained two new colours
that complete the symmetric high-pass side of the family. `blue`
(alias `azure`) is the discrete first difference of white noise,
`y[n] = 0.5·(x[n] − x[n−1])`, whose frequency response
`|H(e^{jω})|² = 2·(1 − cos ω)` is the discrete-derivative magnitude:
zero at DC, monotonically rising to 4 at the Nyquist limit — power
spectral density grows roughly as `f²` over the audio band,
+6 dB/octave, the explicit complement of brown's −6 dB/octave
low-pass running integral. `violet` (alias `purple`) is the second
difference `y[n] = 0.25·(x[n] − 2·x[n−1] + x[n−2])`, the same filter
applied twice so the response squares to
`[2·(1 − cos ω)]² = 4·(1 − cos ω)²` — rising from 0 at DC to 16 at
Nyquist, +12 dB/octave PSD slope, the discrete second-derivative
counterpart of brown's −12 dB/octave double-integral. The 0.5 / 0.25
scalings come from the worst-case input bounds (`|x − x_prev| ≤ 2`,
`|x − 2·x_prev + x_prev2| ≤ 4` when each draw is in `[−1, 1]`) and
guarantee every sample stays strictly inside `[−amplitude, amplitude]`
for every `(n, seed, amplitude)` and every sample rate. Validated by
an in-tree single-bin DFT — blue's 3 kHz / 200 Hz magnitude ratio
dominates white's by ≥5×, and violet's ratio is ≥1.5× steeper than
blue's, both well clear of the asserted floors. Same seed produces
identical samples (`Determinism` section's contract) and the
dispatcher's `expected …` error message now lists all five colours.
Pure first-principles DSP, no spec or external-library dependency.
Reaches the URI path
(`generate://synth?type=noise&color=blue&seed=…`), the `synth:`
shorthand, and the `audio.synth` filter through the existing
dispatcher (no new registration).

Round 8 (2026-05-29): synth catalogue gained `am` — classical analogue
amplitude modulation `amplitude · 0.5 · (1 + m·sin(2π·fm·t)) ·
sin(2π·fc·t)`. By the prosthaphaeresis identity the expanded form is
`0.5·sin(2π·fc·t) + 0.25·m·[cos(2π·(fc − fm)·t) − cos(2π·(fc + fm)·t)]`,
so the spectrum is an unsuppressed carrier at `fc` plus two sidebands
at `fc ± fm` — explicitly the carrier-preserving counterpart of the
existing `ringmod` mode (which suppresses the carrier entirely; the
side-by-side test compares DFT magnitude at `fc` for both and confirms
AM's carrier dominates ringmod's by ≥10×). `index=` is the modulation
index `m ∈ [0, 1]` (100 % modulation at `m=1`, pure half-amplitude
carrier at `m=0`); out-of-range values are clamped at the dispatcher.
The leading `0.5` keeps the worst-case `(1 + m)·1 = 2` at `m=1` inside
`[-amplitude, amplitude]` for every `(fc, fm, index)` and every sample
rate. Pure first-principles DSP, no spec or external-library dependency.
Reaches the URI path (`generate://synth?type=am&carrier=…&modulator=…`),
the `synth:` shorthand, and the `audio.synth` filter through the
existing dispatcher (no new registration).

Round 7 (2026-05-25): synth catalogue gained `formant` (alias `vowel`)
— a Klatt-style two-formant vowel synthesizer (after Klatt, 1980,
"Software for a cascade/parallel formant synthesizer", JASA
67(3):971-995 — paper is the public reference, no source-reading of any
Klatt / Festival / espeak / mbrola / Praat implementation). A
glottal-pulse train at `f0=` (impulse every `Fs/f0` samples, lightly
low-passed) drives two parallel 2-pole resonators tuned to the formant
centres `(F1, F2)`, with the standard Klatt-normalised biquad
`y[n] = (1−r²)·x[n] + 2·r·cos(ω)·y[n−1] − r²·y[n−2]` holding the
magnitude response at unity at the formant peak with `bw=` Hz of
bandwidth (default 80). The two resonator outputs are summed and
peak-normalised so output stays inside `[-amplitude, amplitude]`.
`vowel=A|E|I|O|U` (case-insensitive) selects textbook-standard
adult-male centres consistent with the 1952 Peterson & Barney study:
`A→(730,1090)`, `E→(530,1840)`, `I→(270,2290)`, `O→(570,840)`,
`U→(300,870)` Hz. Validated by an in-tree single-bin DFT — every
vowel's peaks at the f0-harmonic nearest each formant dominate an
out-of-band probe at 3300 Hz by ≥3× (measured ratios well clear of
the asserted floor). Reaches the URI path
(`generate://synth?type=formant&vowel=A&f0=220`), the `synth:`
shorthand, and the `audio.synth` filter through the existing
dispatcher (no new registration).

Round 6 (2026-05-24): synth catalogue gained `ringmod` — classical
analogue ring modulation, the literal product of two sines:
`amplitude · sin(2π·f1·t) · sin(2π·f2·t)`. By the prosthaphaeresis
identity `sin(α)·sin(β) = ½·[cos(α−β) − cos(α+β)]`, the spectrum
collapses to the sum and difference tones `f1 ± f2` at half amplitude
each — the carrier components at `f1` and `f2` are fully suppressed,
which is exactly what distinguishes ring modulation from amplitude
modulation (the latter keeps the carrier). Worst-case
`|sin·sin| ≤ 1`, so the output stays bounded by `amplitude` for every
`(f1, f2)` and every sample rate. Pure first-principles DSP, no spec
or external-library dependency. Reaches the URI path
(`generate://synth?type=ringmod&f1=…&f2=…`), the `synth:` shorthand,
and the `audio.synth` filter through the existing dispatcher (no new
registration).

Round 5 (2026-05-24): synth catalogue gained `adsr` — an
Attack-Decay-Sustain-Release amplitude envelope applied to a base
oscillator. `wave=` picks the carrier (`sine` default, plus `square` /
`triangle` / `sawtooth`); `attack=` / `decay=` / `release=` are segment
durations in seconds and `sustain=` is the hold level in `[0, 1]`. The
envelope is piecewise-linear: a `0 → 1` attack ramp, a `1 → sustain`
decay ramp, a flat sustain hold, then a `sustain → 0` release ramp taken
from the tail of the overall `duration=`, reaching exactly 0 at the final
sample. Because the carrier runs at full amplitude and the envelope is
bounded in `[0, 1]`, the output stays inside `[-amplitude, amplitude]`.
Math-only piecewise-linear shaping; no spec or external-library
dependency. Reaches the URI path, the `synth:` shorthand, and the
`audio.synth` filter through the existing dispatcher (no new
registration).

Round 4 (2026-05-23): synth catalogue gained `dtmf` — telephone
touch-tone dual-tone multi-frequency dialling. `digits=` is the key
sequence (`0`-`9`, `A`-`D`, `*`, `#`); each key is the sum of one
low-group (697/770/852/941 Hz) and one high-group (1209/1336/1477/1633
Hz) sine, both at half amplitude so an aligned peak stays bounded. Per-key
on/off timing comes from `tone=` / `gap=` (seconds); the overall
`duration=` is ignored — the length is derived from the dialled string.
Frequency layout follows the ITU-T Q.23 / Q.24 keypad. Math-only, no
spec dependency.

Round 3 (2026-05-20): synth catalogue grew chirp / FM / multitone
modes (linear + exponential frequency sweeps; classical 2-operator
frequency modulation; equal-weight tone sums). Video catalogue
gained `zoneplate` — `cos(k·r²)` radial chirp, optional
`motion=temporal|horizontal|vertical` to animate it without
changing structure. All three additions are math-only (no spec
dependency); useful for codec PSNR / motion-search / spatial-
frequency probes.

Round 2 (2026-05-02): URI source path migrated to the new typed
`SourceRegistry` `FrameSource` shape — every `generate://…` URI returns
`SourceOutput::Frames` directly, and the round-1 video-bails-with-
`Unsupported` gotcha is gone. Audio + image + video URIs all work
end-to-end with no intermediate encode/decode round-trip; the
hand-rolled WAV / PNG emitters that the bytes-shaped path required have
been removed (they were internal-only — no public API change for the
filter or shorthand surfaces). The filter API and CLI shorthand
translator are unchanged.

Round 1: audio basics + image basics + procedural images + video
generators all landed.

## CSS colour parser

Hand-rolled. Accepts a curated subset of the CSS/HTML4 named colours
plus `#RGB`, `#RGBA`, `#RRGGBB`, and `#RRGGBBAA`.

## Determinism

All randomness is seeded — every generator that takes a `seed=` query
parameter is bit-deterministic across builds. Defaults: `seed=42` for
plasma / Perlin, `seed=0x12345678` for white / pink / brown noise.

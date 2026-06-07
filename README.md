# oxideav-generator

Pure-Rust synthetic media generator for the oxideav framework. Provides
audio synth (sine / square / triangle / sawtooth / supersaw
(detuned-sawtooth stack) / pulse-width-modulated rectangle /
Karplus-Strong pluck / linear + exponential chirp / FM / AM /
sub-audio tremolo (unipolar-cosine LFO over any carrier) /
ring modulation / DTMF touch-tones / ADSR-enveloped tone / Klatt-style
two-formant vowel synthesizer / Shepard tone (octave-spaced
Gaussian-weighted sine stack) / multi-tone /
white-pink-brown-blue-violet noise / silence),
image basics (solid colour, linear / radial gradient,
checkerboard, horizontal / vertical stripes, sinusoidal grating —
single-frequency cos at a chosen orientation), procedural imagery
(Mandelbrot + Julia fractals, plasma, Perlin + simplex gradient
noise, value / lattice noise, Worley cellular noise), and video
(classical broadcast `testsrc`, SMPTE colour bars, animated Mandelbrot
zoom, hue-rotating gradient, zone-plate `cos(k·r²)` spatial-frequency
probe).

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
generate://synth?type=supersaw&freq=440&voices=7&detune=12&duration=2
generate://synth?type=pwm&freq=220&duty=0.25&duration=2
generate://synth?type=pwm&freq=220&duty=0.5&lfo=2&depth=0.3&duration=3
generate://synth?type=pluck&freq=440&decay=0.99&duration=3
generate://synth?type=chirp&shape=linear&f0=200&f1=4000&duration=4
generate://synth?type=chirp&shape=exp&f0=20&f1=20000&duration=4
generate://synth?type=fm&carrier=440&modulator=110&index=5&duration=2
generate://synth?type=am&carrier=440&modulator=60&index=0.5&duration=2
generate://synth?type=tremolo&wave=sine&freq=440&lfo=5&depth=0.7&duration=2
generate://synth?type=ringmod&f1=440&f2=60&duration=2
generate://synth?type=dtmf&digits=0123456789&tone=0.1&gap=0.05
generate://synth?type=adsr&wave=sine&freq=440&attack=0.02&decay=0.1&sustain=0.7&release=0.2&duration=2
generate://synth?type=formant&vowel=A&f0=220&duration=0.5
generate://synth?type=shepard&voices=8&duration=2
generate://synth?type=shepard&freq=55&voices=8&center_freq=622&sigma=1.5&duration=2
generate://synth?type=multitone&freqs=440,1000,2200&duration=1
generate://synth?type=noise&color=pink&duration=10
generate://synth?type=noise&color=blue&seed=42&duration=10
generate://synth?type=noise&color=violet&seed=42&duration=10

generate://xc?color=red&w=640&h=480
generate://xc?color=%23ff0000      # #ff0000 percent-encoded
generate://gradient?w=640&h=480&from=red&to=blue&direction=horizontal
generate://gradient?w=640&h=480&from=red&to=blue&type=radial
generate://pattern?type=checkerboard&w=640&h=480&size=32
generate://grating?w=640&h=480&freq=8&angle=0&phase=0
generate://grating?w=640&h=480&freq=16&angle=45&amplitude=0.7
generate://fractal?type=mandelbrot&w=640&h=480&cx=-0.5&cy=0&zoom=2&iter=256
generate://fractal?type=julia&w=640&h=480&cx=-0.7&cy=0.27&iter=256
generate://plasma?w=640&h=480&seed=42
generate://noise?type=perlin&w=640&h=480&scale=64&seed=42
generate://noise?type=simplex&w=640&h=480&scale=64&octaves=4&seed=42
generate://noise?type=value&w=640&h=480&scale=64&octaves=4&seed=42
generate://noise?type=worley&w=640&h=480&scale=48&seed=42
generate://noise?type=worley&dist=manhattan&k=2&points=2&w=640&h=480&scale=48&seed=42

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
| `grating:`             | `generate://grating`                                         |
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

Round 17 (2026-06-07): image catalogue gained `grating` — a sinusoidal
grating, the canonical single-tone spatial-frequency probe from Fourier
image analysis. Every pixel is set to
`0.5 + 0.5 · amplitude · cos(2π · (f_x · x + f_y · y) + phase_radians)`
with the spatial frequency vector derived from `freq=` (cycles across
the image width) and `angle=` (degrees clockwise from the +x axis):
`f_x = freq · cos(θ) / w`, `f_y = freq · sin(θ) / w`. Phase shift is
controlled by `phase=` in degrees; `amplitude=` clamps to `[0, 1]`
(0 → flat mid-grey, 1 → reaches full white and full black on the
peaks). Near-zero `cos(θ)` / `sin(θ)` (|x|<1e-6) snap to 0 so the
canonical horizontal (`angle=0`) and vertical (`angle=90`) gratings
are exactly axis-aligned without f32 round-off leakage on the
orthogonal axis. Output is greyscale RGBA8 with `R=G=B=byte`,
`A=255`, the same rendering convention as the in-tree `zoneplate`.
Distinct from `zoneplate` — the zone plate sweeps every spatial
frequency simultaneously via the radial chirp `cos(k·r²)` while the
grating isolates exactly one `(magnitude, direction)` pair on a flat
plane. Distinct from `pattern` — the grating is C∞-smooth while the
checker / stripes patterns are piecewise constant. Eleven new unit
tests cover (a) dimensions match the query, (b) `amplitude=0`
collapses to flat mid-grey, (c) `freq=0 phase=0` is flat peak white,
(d) `freq=0 phase=180` is flat black, (e) horizontal grating
(`angle=0`) is constant down each column, (f) vertical grating
(`angle=90`) is constant across each row, (g) `freq=1` on a 16-wide
image hits the cos(0)=1 / cos(π)=-1 / cos(π/2)≈0 landmarks at the
expected x positions, (h) `freq=2` produces two peak-white columns
with a matching trough between them, (i) `angle=45` breaks both
axis-symmetry invariants, (j) `amplitude=2` clamps and renders
byte-identical to `amplitude=1`, (k) alpha is opaque on every pixel,
plus a single-frame URI roundtrip in `tests/source_uri.rs` confirming
`generate://grating?w=4&h=4&freq=0&phase=0&amplitude=1` returns one
4×4 RGBA frame of all-white pixels (64 bytes) and an entry in
`tests/filter_zero_input.rs` confirming `image.grating` is a
zero-input one-output filter. Pure first-principles maths;
cos-of-linear-phase is textbook Fourier analysis with no external
reference required. Reaches the URI path (`generate://grating?…`),
the `grating:` shorthand prefix (with the trailing tail used verbatim
as the query string), and the `image.grating` filter through the
existing registration.

Round 16 (2026-06-05): synth catalogue gained `shepard` — a Shepard tone,
the classical octave-circular-pitch construct described in Roger
Shepard's 1964 *Journal of the Acoustical Society of America* paper
"Circularity in Judgments of Relative Pitch" (vol. 36 no. 12 p. 2346).
The output is the weighted sum of `voices` sine tones spaced exactly one
octave apart starting at `freq`, with each voice scaled by a Gaussian
envelope in log-frequency space centred on `center_freq` with width
`sigma` octaves: `w_k = exp(-(log2(f_k / center_freq) / sigma)²)`. The
sum is normalised by `Σ w_k`, so the worst-case peak (all voices
momentarily aligned) sits exactly at `amplitude` for every
`(freq, voices, center_freq, sigma)` combination and every sample rate.
Defaults are `freq=55` (lowest voice), `voices=8` (clamped to `[1, 12]`),
`sigma=1.5` octaves (clamped to `[0.1, 6.0]`), and
`center_freq = freq · 2^((voices−1)/2)` — the geometric mean of the
voice frequencies, i.e. the log-midpoint of the octave stack (for the
default 55 Hz × 8 voices that lands at ≈ 622 Hz). The Gaussian
log-envelope is the canonical shape Shepard uses to render the absolute-
pitch information ambiguous while preserving a clear chroma percept,
distinct from the in-tree `multitone` (which is a flat equal-weight sum
of an arbitrary frequency list — no log-envelope, no octave constraint,
no centre/sigma). Eight new tests cover (a) basic render shape +
amplitude bound on a default 8-voice render, (b) `voices=1` collapse to
sample-equivalent in-tree `sine` at the matching frequency (the weight
normalisation `scale = amplitude / w_0` cancels the single Gaussian),
(c) octave-spacing verified by single-bin DFT — magnitudes at `f0` and
`2·f0` both register meaningfully while an off-octave probe at `1.3·f0`
is much quieter, (d) Gaussian-envelope identity — shifting `center_freq`
across the stack reshapes the mix audibly (max abs sample difference
> 0.05), with both renders staying inside `±amplitude`, (e) `freq ≤ 0`
erroring out, (f) `center_freq ≤ 0` erroring out, (g) `voices=100`
clamping silently to 12 (matches the explicit voices=12 render bit-for-
bit), (h) the dispatcher's "unknown type" hint advertising `shepard`,
(i) the default-centre algebraic identity — a default render with no
explicit `center_freq` agrees sample-for-sample with one that passes the
log-midpoint formula explicitly. Plus a single-frame URI roundtrip in
`tests/source_uri.rs` confirms `generate://synth?type=shepard&voices=6&duration=0.05`
returns one `AudioFrame` of 400 mono S16 LE samples (800 bytes) with the
peak inside the `amplitude=0.8` S16 bound. Pure first-principles DSP;
sole reference is the 1964 Shepard JASA paper, a public academic source.
Reaches the URI path (`generate://synth?type=shepard&…`), the `synth:`
shorthand via the existing comma-arg parser, and the `audio.synth`
filter through the existing dispatcher (no new registration).

Round 14 (2026-06-03): image noise gained `value` (alias `lattice`) —
classical value noise, the textbook predecessor to gradient noise that
Ken Perlin's 1985 SIGGRAPH paper *An Image Synthesizer* introduced
before moving on to gradient noise. Each integer lattice point holds a
pseudo-random scalar in `[-1, 1]`; a sample at `(x, y)` smoothstep-
interpolates the four surrounding lattice values. The lattice values
themselves come from the existing seeded 512-entry permutation table
(`build_perm`) — `perm[(perm[ix & 0xFF] + iy) & 0xFF]` is a `u8`
hashed deterministically from `(ix, iy, seed)`, then remapped from
`[0, 255]` to `[-1, 1]` via `(h · 2 / 255) − 1`. The smoothstep is the
exact same quintic `t³·(t·(6t − 15) + 10)` `fade` curve `perlin2`
uses, so the surface is C²-continuous across every lattice boundary.
Output is bounded by `[-1, 1]` exactly because both the corner values
and the interpolation weights are bounded that way (a convex
combination of values in `[-1, 1]` stays in `[-1, 1]`), which matches
`perlin2` / `simplex2` so the shared multi-octave fBm accumulator,
palette mapping, `scale=` / `octaves=` / `seed=` parameters all work
unchanged — `value` is the third basis on the same accumulator
alongside `perlin` and `simplex`. Distinct from gradient noise:
value noise has axis-aligned blocky low-frequency character because
the lattice scalars (not gradients of a hidden field) carry the
signal, which is exactly why Perlin moved on from it. Ten new tests
cover (a) basic render shape, (b) `value` / `lattice` alias byte-
equivalence, (c) seed determinism + seed divergence, (d) categorical
distinctness from `perlin` and `simplex` at the same seed/scale
(different algorithm), (e) distinctness from `worley` too (third
independent basis), (f) the raw-sample `[-1, 1]` boundedness invariant
over a 200×200 grid plus a non-degeneracy `|v| > 0.3` floor,
(g) integer-lattice corner identity — `value2(perm, 3.0, 5.0)` must
equal the corner's own remapped scalar because `fade(0) = 0` zeros
the neighbour contributions, (h) palette-bounded output (≥ 8 distinct
colours in a 48×48 render), (i) the unknown-type error message now
advertising `value`. Pure first-principles maths; reference is
Perlin's 1985 SIGGRAPH paper, an already-cited public academic source
for this module. Reaches the URI path
(`generate://noise?type=value&…`), the `noise:value` shorthand (via
the existing `noise:<type>` prefix), and the `image.noise` filter
through the existing dispatcher (no new registration).

Round 13 (2026-06-02): image noise gained `worley` (alias `cellular`)
— Worley / cellular noise, a spatial-point-process texture distinct
from gradient noise. The plane is divided into integer cells of side
`scale` pixels; each cell holds `points ∈ [1, 4]` pseudo-randomly
placed feature points (`points=1` is the canonical Voronoi / "stone
wall" texture, higher values pack the plane more densely); for each
pixel the renderer scans the 3×3 neighbourhood of cells around the
pixel's home cell, gathers every feature-point distance, and palette-
maps the k-th closest distance (`k ∈ [1, 4]`, default 1 = the F1
distance; `k=2` is the so-called F2 distance, etc.). Three distance
metrics are exposed: `dist=euclidean|euc|l2` (default), `manhattan|l1`,
`chebyshev|linf|max` — Euclidean gives the smooth circular falloff,
Manhattan gives axis-rotated diamonds, Chebyshev gives axis-aligned
squares, all on the same Voronoi cell structure. Pseudo-random feature-
point placement uses the existing in-tree LCG keyed by
`(cell_x, cell_y, slot, seed)`, so `seed=` is bit-deterministic across
builds and across the gradient-noise modes the same module already
ships. Twelve new tests cover (a) basic render shape, (b) `worley` /
`cellular` alias byte-equivalence, (c) seed determinism + seed
divergence, (d) categorical distinctness from `perlin` and `simplex` at
the same seed (it's a fundamentally different algorithm), (e) the
three metrics each rendering and producing visibly different images,
(f) `k=1` vs `k=2` divergence, (g) `points=1` vs `points=3` divergence,
(h) the placement contract — each feature point stays inside its
declared cell over a sweep of negative + positive cell coordinates,
(i) palette-bounded output (no panic on the indexed access, ≥ 8
distinct colours in a 48×48 render), (j) Chebyshev render non-
degeneracy, (k) unknown-metric error path. Mathematical reference is
Steven Worley, *A Cellular Texture Basis Function*, SIGGRAPH 1996
proceedings — a public academic paper on cellular-noise basis
functions. Pure first-principles maths; no other reference consulted.

Round 12 (2026-06-01): audio synth gained `supersaw` (alias `saws`) —
a detuned-sawtooth stack that piles `voices` (default 7, clamped to
`[1, 32]`) sawtooth oscillators around a centre frequency `freq` Hz
and equal-weight averages them. `detune=` is the half-spread in cents
(1 cent = 1/100 of an equal-tempered semitone; default 12 cents) so
voices are placed symmetrically over `[-detune, +detune]` with the
middle voice landing exactly on `freq` for odd `voices`. The classic
"supersaw" timbre (popularised by the 1996 Roland JP-8000) emerges
from the slow chorus-like beating between near-but-not-quite-identical
sawtooths: 7 voices × 12 cents in either direction gives ~5 % maximum
frequency spread, audibly thick but tonally still anchored at `freq`.
Per-voice frequencies are `freq · 2^(c_k / 1200)` for the chosen
cent offsets. Output is the average of in-tree
[`sawtooth`](crate::audio::synth::sawtooth) calls so the worst-case
peak stays inside `[-amplitude, amplitude]` for every
`(freq, voices, detune)` and every sample rate. Nine new tests cover
(a) `voices=1` collapses to sample-equivalent in-tree `sawtooth`,
(b) `detune=0` with any `voices` count likewise collapses (the average
of identical voices), (c) bounded-amplitude invariant on a non-trivial
44.1 kHz × 4096-sample render, (d) audible divergence from the centre
saw at `voices=7, detune=12`, (e) `freq ≤ 0` erroring out,
(f) `type=supersaw` / `type=saws` alias equivalence, (g) listing in
the "unknown type" help, (h) `voices=100` clamping silently to 32,
(i) the algebraic property that odd voice counts put the middle voice
at 0 cents. Mathematical reference is Adam Szabo, *How to Emulate the
Super Saw* (KTH Royal Institute of Technology MSc thesis, 2010) — a
public academic spectral analysis of detuned-saw stacks. Pure
first-principles DSP otherwise; the in-tree `sawtooth` is reused
unchanged per voice.

Round 11 (2026-06-01): audio synth gained `pwm` (alias `pulse`) —
a pulse-width-modulated rectangular oscillator that generalises the
fixed-50%-duty `square` wave. `duty=` in `(0, 1)` is the fraction of
each period the signal sits at `+amplitude` (the remainder sits at
`−amplitude`); `duty=0.5` reproduces `square` sample-for-sample.
Optional `lfo=` (Hz) + `depth=` together drive the canonical
analogue-synth pulse-width-modulation effect: the duty threshold
sweeps sinusoidally between `duty − depth` and `duty + depth` at
`lfo` Hz, turning the static rectangle into a chorus-like / phasing
widening of the classical pulse. The duty clamp is
resolution-aware (`eps = max(1.5 / period_samples, 1e-3)`) so each
period always contains at least one positive and one negative sample
at every sample rate, depth is clamped so `duty ± depth` never
crosses the same edges, and the output only takes values in
`{+amplitude, −amplitude}` so it is exactly bounded by `amplitude`
for every `(freq, duty, lfo, depth)`. Eleven new tests cover the
duty=0.5 ↔ `square` identity, the binary `{±amp}` invariant, the
`duty → positive-fraction` linearity (≤2% error across five duty
settings), the duty=0/1 clamp (no silent DC), the LFO actually
steering the positive-fraction across the buffer (q1 vs q3 ≥ 0.15
apart), `freq ≤ 0` erroring out, the dispatcher `type=pwm` /
`type=pulse` aliasing, the new mode being advertised in the
"unknown type" help, and a pinned 16-sample fixture (freq=1 kHz,
duty=0.25 → two-on / six-off per period). Pure first-principles
DSP; references are textbook analogue-synth theory (Moore, *Elements
of Computer Music* 1990 ch.4 + the standard line-spectrum
Fourier-series `∝ sin(π · k · d) / (π · k)` for a duty-`d`
rectangular train).
Also: integration test `tests/source_uri.rs` now matches
`SourceOutput` exhaustively via a fall-through `_` arm — the
upstream enum became `#[non_exhaustive]`, which had broken
`cargo test` for the entire crate before this round's new test
could run.

Round 10 (2026-05-30): `generate://noise?type=simplex` is now a real
Ken-Perlin-2001 improved-gradient-noise generator instead of an alias
that silently produced byte-identical output to `type=perlin`. The 2-D
simplex tessellation tiles the plane with equilateral triangles: each
sample point is skewed by `F2 = (√3 − 1) / 2` into a sheared lattice
where the containing simplex is found by a single integer floor plus one
`x0 > y0` "which-half" comparison, the three corners are unskewed back by
`G2 = (3 − √3) / 6`, and each corner contributes a radially-attenuated
`max(0, 0.5 − r²)⁴ · (gradient · offset)` term (the falloff confines a
corner's influence to its own simplex, giving a C²-continuous surface
with no directional bias). The summed contributions are scaled by `70.0`
back toward `[−1, 1]`, matching `perlin2`'s output range so the shared
multi-octave fBm accumulator, palette mapping, `scale=` / `octaves=` /
`seed=` parameters, and the 512-entry seeded permutation table
(`build_perm`, Fisher-Yates with the in-tree LCG) all work unchanged for
both kinds. An in-tree test sweeps a 200×200 grid and asserts the raw
samples stay inside `[−1, 1]` while still exercising a meaningful slice
of the range (|v| > 0.3); another confirms simplex output now differs
byte-for-byte from Perlin at the same seed/scale (it used to be
identical). Same `seed=` is bit-deterministic across builds. Pure
first-principles maths transcribed from Ken Perlin's 2001 SIGGRAPH
note on improved noise.

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
Pure first-principles DSP. Reaches the URI path
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
rate. Pure first-principles DSP. Reaches the URI path
(`generate://synth?type=am&carrier=…&modulator=…`),
the `synth:` shorthand, and the `audio.synth` filter through the
existing dispatcher (no new registration).

Round 7 (2026-05-25): synth catalogue gained `formant` (alias `vowel`)
— a Klatt-style two-formant vowel synthesizer (after Klatt, 1980,
"Software for a cascade/parallel formant synthesizer", JASA
67(3):971-995 — the paper is the public reference). A
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
`(f1, f2)` and every sample rate. Pure first-principles DSP. Reaches
the URI path (`generate://synth?type=ringmod&f1=…&f2=…`), the `synth:`
shorthand,
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
Math-only piecewise-linear shaping. Reaches the URI path, the
`synth:` shorthand, and the
`audio.synth` filter through the existing dispatcher (no new
registration).

Round 4 (2026-05-23): synth catalogue gained `dtmf` — telephone
touch-tone dual-tone multi-frequency dialling. `digits=` is the key
sequence (`0`-`9`, `A`-`D`, `*`, `#`); each key is the sum of one
low-group (697/770/852/941 Hz) and one high-group (1209/1336/1477/1633
Hz) sine, both at half amplitude so an aligned peak stays bounded. Per-key
on/off timing comes from `tone=` / `gap=` (seconds); the overall
`duration=` is ignored — the length is derived from the dialled string.
Frequency layout follows the ITU-T Q.23 / Q.24 keypad. Math-only.

Round 3 (2026-05-20): synth catalogue grew chirp / FM / multitone
modes (linear + exponential frequency sweeps; classical 2-operator
frequency modulation; equal-weight tone sums). Video catalogue
gained `zoneplate` — `cos(k·r²)` radial chirp, optional
`motion=temporal|horizontal|vertical` to animate it without
changing structure. All three additions are math-only; useful for
codec PSNR / motion-search / spatial-frequency probes.

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
plasma / Perlin / simplex / value, `seed=0x12345678` for white / pink
/ brown noise. Perlin, simplex, and value all draw from the same
seeded 512-entry permutation table, so a given `seed=` is reproducible
across all three gradient / lattice modes.

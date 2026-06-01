# Changelog

All notable changes to oxideav-generator are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Audio synth gained `supersaw` (alias `saws`) — a detuned-sawtooth
  stack that piles `voices` sawtooth oscillators around a centre
  frequency `freq` Hz and equal-weight averages them. `voices` defaults
  to 7 (clamped to `[1, 32]`); `detune=` is the half-spread in cents
  (1 cent = 1/100 of an equal-tempered semitone; default 12 cents) so
  the voices are placed symmetrically over `[-detune, +detune]` with
  the middle voice landing exactly on `freq` for odd `voices`. Per-voice
  frequencies are `freq · 2^(c_k / 1200)` for the chosen cent offsets.
  Output is the equal-weight average of in-tree
  [`sawtooth`](crate::audio::synth::sawtooth) calls so the worst-case
  peak stays inside `[-amplitude, amplitude]` for every
  `(freq, voices, detune)` and every sample rate. The classic
  "supersaw" timbre (popularised by the 1996 Roland JP-8000) emerges
  from the slow chorus-like beating between the slightly-detuned
  sawtooths; `7 voices × 12 cents` gives ~5 % maximum frequency spread,
  audibly thick but tonally still anchored on `freq`. Tests cover (a)
  `voices=1` collapsing to in-tree `sawtooth` sample-for-sample, (b)
  any `voices` at `detune=0` likewise collapsing (`/voices` average of
  identical voices), (c) bounded-amplitude invariant on a non-trivial
  44.1 kHz × 4096-sample render, (d) audible divergence from the
  centre saw at `voices=7, detune=12`, (e) `freq ≤ 0` erroring out,
  (f) `type=supersaw` / `type=saws` aliasing, (g) listing in the
  "unknown type" help, (h) `voices=100` silently clamping to 32,
  (i) the algebraic identity that odd voice counts put the middle
  voice exactly at 0 cents. Mathematical reference is Adam Szabo,
  *How to Emulate the Super Saw* (KTH Royal Institute of Technology
  MSc thesis, 2010) — a public academic spectral analysis of
  detuned-saw stacks. Pure first-principles DSP otherwise; the
  in-tree `sawtooth` is reused unchanged per voice.

- Audio synth gained `pwm` (alias `pulse`) — a pulse-width-modulated
  rectangular oscillator that generalises the fixed-50%-duty `square`
  wave. `duty=` in `(0, 1)` is the fraction of each period the signal
  sits at `+amplitude` (the remainder sits at `−amplitude`), and a
  non-zero `lfo=` + `depth=` pair sweeps the duty threshold
  sinusoidally between `duty − depth` and `duty + depth` at `lfo` Hz
  — the canonical analogue-synth pulse-width-modulation effect that
  turns the static pulse into a chorus-like / phasing widening of the
  classical rectangle wave. The duty clamp is resolution-aware
  (`eps = max(1.5 / period_samples, 1e-3)`) so each period always
  contains at least one positive and one negative sample at every
  sample rate, depth is clamped so `duty ± depth` never crosses the
  same edges, and the output only takes values in
  `{+amplitude, −amplitude}` so it is exactly bounded by `amplitude`
  for every `(freq, duty, lfo, depth)`. Tests cover (a) duty=0.5 +
  zero LFO reproducing `square` sample-for-sample, (b) the binary
  `{±amp}` invariant, (c) `duty ∈ {0.1, 0.25, 0.5, 0.75, 0.9}`
  yielding the matching positive-sample fraction within ~2%, (d) the
  duty=0/1 clamps producing alternating polarities (no silent DC),
  (e) the LFO actually steering the positive-fraction across the
  buffer (q1 vs q3), (f) `freq ≤ 0` erroring out, (g) the dispatcher
  `type=pwm` / `type=pulse` aliasing, (h) the catalogue listing the
  new mode in the "unknown type" help, and (i) a pinned 16-sample
  fixture (freq=1 kHz, duty=0.25 → two-on / six-off per period) so
  future refactors can't silently change the wire format. Pure
  first-principles DSP; references are textbook analogue-synth theory
  (Moore, *Elements of Computer Music* 1990 ch.4 + the standard
  line-spectrum Fourier-series result `∝ sin(π · k · d) / (π · k)`
  for a duty-`d` rectangular train).

### Changed

- Integration test `tests/source_uri.rs` now matches `SourceOutput`
  exhaustively via a fall-through `_` arm — the upstream enum became
  `#[non_exhaustive]`, which had broken `cargo test` for the entire
  crate before the new test could even run. No behaviour change.

## [0.1.4](https://github.com/OxideAV/oxideav-generator/compare/v0.1.3...v0.1.4) - 2026-05-29

### Other

- round 10: real simplex noise for generate://noise?type=simplex
- round 9: synth noise gained `blue` and `violet` colours
- round 8: synth `am` — classical analogue amplitude modulation
- round 7: synth `formant` — Klatt-style two-formant vowel synthesizer
- round 6: synth `ringmod` — classical analogue ring modulation
- round 5: synth `adsr` — Attack-Decay-Sustain-Release enveloped tone
- add DTMF touch-tone (dual-tone multi-frequency) mode
- add chirp / FM / multitone modes + video zoneplate pattern

### Added

- `generate://noise?type=simplex` is now a real Ken-Perlin-2001
  improved-gradient-noise generator. Previously `type=simplex` was a
  straight alias that produced byte-identical output to `type=perlin`;
  it now runs the genuine 2-D simplex algorithm. The plane is tiled
  with equilateral triangles: each sample is skewed by
  `F2 = (√3 − 1) / 2` into a sheared lattice (so the containing simplex
  is found by one integer floor + one `x0 > y0` half-test), the three
  corners are unskewed back by `G2 = (3 − √3) / 6`, and each corner
  contributes a radially-attenuated `max(0, 0.5 − r²)⁴ ·
  (gradient · offset)` term — the falloff confines a corner's
  influence to its own simplex, giving a C²-continuous surface with no
  directional bias. The summed contributions are scaled by `70.0` back
  toward `[−1, 1]`, matching the existing `perlin2` range so the shared
  multi-octave fBm accumulator, palette mapping, `scale=` / `octaves=`
  / `seed=` parameters, and the seeded 512-entry permutation table all
  apply unchanged to both kinds. A new test sweeps a 200×200 grid and
  asserts samples stay inside `[−1, 1]` while still exercising a
  meaningful slice of the range; another asserts simplex output now
  differs byte-for-byte from Perlin at the same seed/scale. Same
  `seed=` is bit-deterministic across builds. Pure first-principles
  maths transcribed from Ken Perlin's 2001 SIGGRAPH note on improved
  noise.

### Changed

- The old `simplex_alias_routes_to_perlin` test (which only checked
  that the simplex alias rendered) was replaced by a `simplex_renders`
  sizing check plus dedicated determinism / seed-divergence /
  distinct-from-Perlin / bounded-range tests for the real generator.

- Audio synth's `noise` family gained two new colours that complete
  the symmetric high-pass side of the spectrum. `blue` (alias
  `azure`) is the discrete first difference of white noise,
  `y[n] = 0.5·(x[n] − x[n−1])`, whose frequency response
  `|H(e^{jω})|² = 2·(1 − cos ω)` is the discrete-derivative magnitude
  — zero at DC, monotonically rising to 4 at the Nyquist limit — so
  power spectral density grows roughly as `f²` over the audio band
  (+6 dB/octave, the explicit complement of brown's −6 dB/octave
  running-integral low-pass). `violet` (alias `purple`) is the
  second difference `y[n] = 0.25·(x[n] − 2·x[n−1] + x[n−2])`, the
  same filter applied twice so the response squares to
  `[2·(1 − cos ω)]² = 4·(1 − cos ω)²` — rising from 0 at DC to 16
  at Nyquist, +12 dB/octave PSD slope, the discrete second-
  derivative counterpart of brown's −12 dB/octave double-integral.
  The `0.5` / `0.25` scalings come from the worst-case input bounds
  (`|x − x_prev| ≤ 2`, `|x − 2·x_prev + x_prev2| ≤ 4` when each
  draw is in `[−1, 1]`) and guarantee every sample stays strictly
  inside `[−amplitude, amplitude]` for every `(n, seed, amplitude)`
  and every sample rate. Validated by a single-bin DFT — blue's
  3 kHz / 200 Hz magnitude ratio dominates white's by ≥5×, and
  violet's ratio is ≥1.5× steeper than blue's, both well clear of
  the asserted floors. The same seed produces bit-identical samples
  (`Determinism` section's contract); the dispatcher's `expected …`
  error message now lists all five colours; the prior
  `unknown_noise_color_errors` test (which used `purple` as its
  unknown-colour sentinel) now uses `chartreuse` instead, since
  `purple` is a documented alias for violet. Pure first-principles
  DSP; reaches the URI path
  (`generate://synth?type=noise&color=blue|violet|azure|purple`),
  the `synth:` shorthand, and the `audio.synth` filter through the
  existing dispatcher (no new registration).
- Audio synth gained an `am` mode — classical analogue amplitude
  modulation, `amplitude · 0.5 · (1 + m·sin(2π·fm·t)) · sin(2π·fc·t)`.
  Expanded via the prosthaphaeresis identity the spectrum is
  `0.5·sin(2π·fc·t) + 0.25·m·[cos(2π·(fc − fm)·t) − cos(2π·(fc + fm)·t)]`,
  i.e. an unsuppressed carrier at `fc` plus two sidebands at
  `fc ± fm` — explicitly the carrier-preserving counterpart of the
  existing `ringmod` mode (the in-tree DFT test compares the bin at
  `fc` for both modes at identical parameters and asserts AM's carrier
  dominates ringmod's by ≥10×). `index=` is the modulation index
  `m ∈ [0, 1]` (100 % modulation at `m=1`, pure half-amplitude carrier
  at `m=0`); the dispatcher clamps out-of-range values. The leading
  `0.5` keeps the worst-case envelope `(1 + m)·1 ≤ 2` at `m=1` inside
  `[-amplitude, amplitude]` for every `(fc, fm, index)` and every
  sample rate (sample-wise bounds verified at `m ∈ {0, 0.25, 0.5, 0.75,
  1}`). Pure first-principles DSP; reaches the URI path
  (`generate://synth?type=am&carrier=…&modulator=…&index=…`), the
  `synth:` shorthand, and the `audio.synth` filter through the
  existing dispatcher (no new registration). Default carrier/modulator
  ratio mirrors the `fm` mode (2:1), default index is `0.5`.
- Audio synth gained a `formant` (alias `vowel`) mode — a Klatt-style
  two-formant vowel synthesizer (after Klatt 1980, JASA 67(3):971-995;
  the paper is the public reference). Architecture: a
  glottal-pulse train at the fundamental `f0=` (an impulse every
  `Fs/f0` samples, lightly low-passed with `0.5·(x[n]+x[n-1])`) drives
  two parallel 2-pole resonators tuned to the formant centres `(F1,
  F2)`. Each resonator is the standard Klatt-normalised biquad
  `y[n] = (1−r²)·x[n] + 2·r·cos(ω)·y[n−1] − r²·y[n−2]` with `r =
  exp(−π·BW/Fs)` and `ω = 2π·F/Fs`, holding the magnitude response at
  unity at the formant centre with a −3 dB bandwidth of `bw=` Hz
  (default 80, a textbook-typical value). The two resonator outputs
  are summed and peak-normalised so every sample stays inside
  `[-amplitude, amplitude]`. `vowel=A|E|I|O|U` (case-insensitive)
  selects textbook-standard rounded adult-male formant centres
  consistent with the 1952 Peterson & Barney study reproduced in every
  introductory phonetics textbook since:
  `A→(730,1090)`, `E→(530,1840)`, `I→(270,2290)`, `O→(570,840)`,
  `U→(300,870)` Hz. Unknown vowels are an error rather than a silent
  default. Reaches the URI path
  (`generate://synth?type=formant&vowel=A&f0=220&duration=0.5`), the
  `synth:` shorthand, and the `audio.synth` filter through the
  existing dispatcher (no new registration). Validated via a small
  in-tree DFT: for each of the five vowels rendered at 220 Hz / 16
  kHz / 100 ms, the DFT magnitude at the f0-harmonic nearest each
  formant centre dominates an off-band probe at 3300 Hz by ≥3× (the
  measured ratios are well clear of the asserted floor).
- Audio synth gained a `ringmod` (`ring`) mode — classical analogue ring
  modulation: `amplitude · sin(2π·f1·t) · sin(2π·f2·t)`. By the
  prosthaphaeresis identity `sin(α)·sin(β) = ½·[cos(α−β) − cos(α+β)]`
  the spectrum collapses to the sum and difference tones `f1 ± f2` at
  half amplitude; the carrier components at `f1` and `f2` are fully
  suppressed, which is what distinguishes ring modulation from
  amplitude modulation. Worst case `|sin · sin| ≤ 1` so the output is
  bounded by `amplitude` for every `(f1, f2)` and every sample rate.
  Pure first-principles DSP; exposed through the URI path
  (`generate://synth?type=ringmod&f1=…&f2=…`), the `synth:` shorthand,
  and the `audio.synth` filter through the existing dispatcher (no new
  registration).
- Audio synth gained an `adsr` mode — an Attack-Decay-Sustain-Release
  amplitude envelope applied to a base oscillator. `wave=` selects the
  carrier (`sine` default, plus `square` / `triangle` / `sawtooth`);
  `attack=` / `decay=` / `release=` are segment durations in seconds and
  `sustain=` is the hold level in `[0, 1]`. The envelope is
  piecewise-linear (`0 → 1` attack, `1 → sustain` decay, flat sustain
  hold, `sustain → 0` release taken from the tail of the overall
  `duration=`, reaching exactly 0 at the final sample), with the release
  start clamped so it never begins before the attack ends. The carrier
  runs at full amplitude and the envelope stays in `[0, 1]`, so the
  output is bounded by `[-amplitude, amplitude]`. An unsupported `wave=`
  is an error. Pure first-principles DSP; reaches the URI path, the
  `synth:` shorthand, and the
  `audio.synth` filter through the existing dispatcher (no new
  registration).
- Audio synth gained a `dtmf` mode — telephone touch-tone dual-tone
  multi-frequency dialling. `digits=` is the key sequence (`0`-`9`,
  `A`-`D`, `*`, `#`; whitespace ignored); each key is the sum of one
  low-group (697/770/852/941 Hz) and one high-group
  (1209/1336/1477/1633 Hz) sine, both at half amplitude so an aligned
  peak stays inside `[-amplitude, amplitude]`. Per-key on/off timing is
  `tone=` / `gap=` (seconds); the overall `duration=` is ignored — the
  length is derived from the dialled string. An unrecognised key is an
  error rather than silently emitting nothing. Frequency layout follows
  the ITU-T Q.23 / Q.24 keypad; pure first-principles DSP. Exposed
  via the existing `synth:` shorthand and `audio.synth` filter (no
  new registration).
- Audio synth gained three modes:
  - `chirp` / `sweep` — linear or exponential frequency sweep
    between `f0` and `f1`; phase is integrated sample-by-sample so
    the waveform is C¹ continuous regardless of `(f0, f1,
    sample_rate)`. Exponential shape requires `f0 > 0` and
    `f1 > 0`.
  - `fm` — classical 2-operator frequency modulation
    `amplitude · sin(2π·fc·t + index·sin(2π·fm·t))`. Defaults
    pick a 2:1 carrier:modulator ratio and `index=5` for a
    bell-like timbre; `index=0` collapses to a pure carrier sine.
  - `multitone` / `tones` — equal-weight sum of sine tones from a
    comma-separated `freqs=440,1000,2200` list. Output is
    normalised by tone count so the worst-case peak stays inside
    `[-amplitude, amplitude]`. Useful for stereo intermodulation
    and image-rejection probes.
- Video catalogue gained `zoneplate` — `cos(k·r²)` radial chirp
  rendered to luma, with optional `motion=none|temporal|
  horizontal|vertical` to animate without changing the overall
  structure. The pattern's local spatial frequency rises linearly
  with distance from the centre, so it exercises every spatial
  frequency the renderer supports in a single image — aliasing,
  ringing and interpolation artefacts appear as moiré rings.
- `zoneplate:` CLI shorthand and `video.zoneplate` filter wired
  through the standard `register()` aggregation.

## [0.1.3](https://github.com/OxideAV/oxideav-generator/compare/v0.1.2...v0.1.3) - 2026-05-06

### Other

- drop stale REGISTRARS / with_all_features intra-doc links
- drop dead `linkme` dep
- drop committed Cargo.lock + relax oxideav-core to "0.1"
- auto-register via oxideav_core::register! macro (linkme distributed slice)
- add top-level register(&mut RuntimeContext) entry point ([#502](https://github.com/OxideAV/oxideav-generator/pull/502))

### Added — unified `register(&mut RuntimeContext)` entry point (#502)

- New top-level `oxideav_generator::register(&mut RuntimeContext)`
  aggregates the existing `register_source` (URI side) +
  `register_filters` (filter-graph side) helpers into the single
  umbrella-friendly entry point every sibling crate now exposes. The
  helpers stay available for callers that only want one half. No
  breaking API change.

## [0.1.2](https://github.com/OxideAV/oxideav-generator/compare/v0.1.1...v0.1.2) - 2026-05-04

### Other

- Delete Cargo.lock
- construct VectorFrame field-by-field for crates.io compat
- migrate to vector shape→raster pipeline ([#354](https://github.com/OxideAV/oxideav-generator/pull/354))
- apply rustfmt to label.rs + source.rs
- add label: text-to-image generator (scribe-backed)

### Changed — `label:` migrated to vector pipeline (#354)

- The `label:text=...` generator no longer uses the removed
  `oxideav_scribe::render_text` API (scribe shipped a vector-only
  refactor that drops its pixel pipeline). The label render now does
  the standard two-step:
  1. `oxideav_scribe::Shaper::shape_to_paths` to emit positioned
     `oxideav_core::Node` glyphs (with the `cache_key` envelope so
     repeat-glyph runs hit the rasterizer's bitmap cache);
  2. `oxideav_raster::Renderer::render` to walk the resulting
     `VectorFrame` and produce a packed RGBA `VideoFrame`.
- The `label` cargo feature now also pulls in `oxideav-raster` (~1
  extra crate, ~140 KB compressed). Public API surface is unchanged
  (`render(&BTreeMap<String, String>) -> Result<Rgba8Image>`) and the
  CLI shorthand `label:Hello world` keeps producing the same
  centred-glyph-on-canvas output it did before — this is an
  implementation refactor, not a behaviour change.

## [0.1.1](https://github.com/OxideAV/oxideav-generator/compare/v0.1.0...v0.1.1) - 2026-05-03

### Other

- fix copyright line (auto-generated by gh repo create was wrong)

### Changed

- URI source path migrated from `BytesSource` (WAV / PNG bytes consumed
  by the standard demuxer chain) to `FrameSource` — `generate://…`
  opening now returns `SourceOutput::Frames` directly. Frames go
  straight to the pipeline executor, skipping demux + decode entirely.
  No public-API impact on the filter surface or the CLI shorthand
  translator.
- `register_source()` now calls `SourceRegistry::register_frames()`;
  the opener function is renamed `open_generate_frames` and returns
  `Box<dyn FrameSource>`. (`open_generate` is removed; in-tree callers
  all migrate.)

### Fixed

- `generate://testsrc`, `generate://smptebars`,
  `generate://fractal_zoom`, `generate://gradient_animate` URIs no
  longer bail with `Unsupported`. Round 1's "no Y4M demuxer in tree"
  workaround is gone — frames flow natively through the typed source
  registry.

### Removed

- Internal hand-rolled WAV writer (`audio/wav.rs`) and PNG writer
  (`image/png.rs`) — the bytes-shaped URI path that needed them is
  gone. The single `f32_sample_to_i16` clipping helper survives at
  `audio::f32_sample_to_i16` for the `FrameSource` PCM materialiser.

## [0.1.0] — 2026-05-02

### Added

- Initial release.
- `generate://` URI source driver registered through `SourceRegistry`.
- Audio synth (sine / square / triangle / sawtooth / Karplus-Strong
  pluck / white-pink-brown noise / silence) emitting canonical 16-bit
  PCM WAV bytes that the `oxideav-basic` WAV demuxer consumes verbatim.
- Image generators (solid colour `xc`, linear / radial gradient,
  checkerboard / horizontal / vertical stripes, Mandelbrot + Julia
  fractals, plasma via diamond-square, fBm Perlin noise) emitting
  PNG bytes via an in-tree minimal PNG writer (uncompressed deflate).
- Video generators (classical broadcast-style `testsrc`, SMPTE 75% colour bars,
  animated Mandelbrot zoom, hue-rotating gradient) wired through the
  filter API; the URI source path for video returns a clear
  "unsupported until we add a Y4M demuxer" error.
- Zero-input filter wrappers for every generator, registered as
  `audio.synth`, `image.{xc,gradient,pattern,fractal,plasma,noise}`,
  `video.{testsrc,smptebars,fractal_zoom,gradient_animate}`.
- Colon-prefixed terse-CLI shorthand translator
  (`xc:red`, `gradient:red-blue`, `synth:5,sine,440`, …) under
  `oxideav_generator::shorthand::translate`.
- Hand-rolled CSS colour parser (named colours + `#RGB(A)` /
  `#RRGGBB(AA)` hex).

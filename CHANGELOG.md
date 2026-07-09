# Changelog

All notable changes to oxideav-generator are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Video catalogue gained `movingbox` (URI alias `box`) — a solid
  `bw × bh` foreground rectangle translating at exactly-known signed
  integer pixels-per-frame `(vx, vy)` over a solid background, with
  toroidal wrap. Closed form:
  `frame_f(x, y) = fg iff (x − x0 − f·vx) mod w < bw and
  (y − y0 − f·vy) mod h < bh` (euclidean remainder, so negative
  velocities / origins work). Where `scroll` probes *global* motion
  (every pixel moves), `movingbox` probes *local* motion: one small
  object over a static background — the case block motion search has
  to isolate. A motion estimator on `(f, f+1)` should return
  `(vx, vy)` for box blocks, `(0, 0)` for background blocks; every
  frame contains exactly `bw · bh` foreground pixels (no resampling,
  no sub-pixel phase). Params: `w`/`h`, `duration`, `fps`, `bw`/`bh`
  (clamped to frame), `x0`/`y0`, `vx`/`vy`, `fg`/`bg` colours. Eight
  unit tests (full-frame closed-form recomputation, exact fg pixel
  count under double-edge wrap, residual-free displacement between
  consecutive frames, static at zero velocity, colours, clamping,
  bad-colour error, determinism) plus URI roundtrip with exact box
  positions, `box` alias equivalence, zero-input `video.movingbox`
  filter test, and `movingbox:` shorthand rows.
- `type=sine` gained `phase=` (initial phase, degrees) and `chphase=`
  (per-channel phase offset, degrees). The closed form becomes
  `s_c[n] = amplitude · sin(2π·freq·n/rate + (phase + c·chphase)·π/180)`
  for channel `c` — the phase is a pure additive offset inside the
  argument (no accumulator), so `phase=90` is exactly the matching
  cosine and two channels rendered `Δφ` apart stay `Δφ` apart for the
  whole buffer. With `chphase=0` (default) stereo renders remain
  bit-identical channel replicas, and `phase=0` is bit-identical to
  the previous `sine` output, so no existing fixture moves. New public
  `sine_phase(freq, rate, n, amplitude, phase_rad)` alongside `sine`
  (now the zero-phase special case). Quadrature stereo
  (`channels=2&chphase=90`) is the canonical stereo /
  mid-side / inter-channel-correlation codec probe. Eight unit tests
  (independent closed-form recomputation, bit-exact zero-phase
  equivalence, cosine identity, degree parsing, chphase interleave
  layout, phase+chphase stacking, replication default, 360°
  periodicity).
- Audio catalogue gained `dc` and `impulse`. `type=dc` renders a
  constant signal `s[n] = level` (signed, clamped to `[-1, 1]`,
  defaulting to the `amplitude` knob) — the classic offset / clipping /
  silence-detector probe with all its power at 0 Hz. `type=impulse`
  (aliases `impulses` / `click`) renders a unipolar impulse train:
  `width` samples at `+amplitude` every `period` samples, first
  impulse at `n = 0`, closed form
  `s[n] = amplitude · [n mod period < width]`. `period=` is an exact
  integer sample count (explicit `period=` wins; otherwise derived
  from `freq=` impulses-per-second, default 1 Hz, as
  `round(rate / freq)`), so the train never accumulates float phase
  drift — the k-th impulse starts at sample `k · period` for any
  render length. `width` is clamped to `1..=period`; `width = period`
  degenerates to DC at `+amplitude`, and `width = 1` is the discrete
  Dirac comb with equal-magnitude spectral lines at every multiple of
  `rate / period` Hz. Eleven unit tests pin the closed forms
  (bit-exact per-sample equality, clamping, period-vs-freq precedence,
  1 Hz default, non-positive-freq rejection, unknown-type help text).
- Video catalogue gained `colorwheel` — a rotating polar hue wheel.
  For each pixel the vector from the frame centre `(dx, dy)` is
  resolved into polar coordinates: hue is the `atan2(dy, dx)` angle in
  `[0, 360)` degrees plus a per-frame additive phase `spin · t` (so the
  wheel rotates rigidly at `spin` degrees per second; `t` = frame
  presentation time), and saturation is the radius
  `sqrt(dx² + dy²)` normalised by `r_max` (half the smaller dimension),
  clamped to `[0, 1]` and scaled by the `saturation` rim parameter.
  Lightness is a fixed parameter (default 0.5). The centre is
  achromatic and the rim fully saturated; one frame sweeps the whole
  hue circle (a chroma probe) and the rotation is a smooth
  angular-motion probe. Reuses the in-tree `palette::hsl_to_rgb`
  converter. Query params: `w`/`h`, `duration`, `fps`, `spin` (deg/s,
  signed), `lightness`, `saturation`. Eight unit tests (frame count,
  achromatic centre, lightness-0 black, saturation-0 grey,
  opposite-angle distinct hues, `spin=0` static, `spin>0` motion,
  determinism) plus a URI roundtrip, a zero-input `video.colorwheel`
  filter test, and `colorwheel:` shorthand rows. Exposed on all three
  surfaces: `generate://colorwheel?…`, the `colorwheel:` shorthand, and
  the `video.colorwheel` filter.
- Video catalogue gained `scroll` — a constant-velocity scrolling
  pattern, the canonical motion-estimation ground-truth probe. A base
  frame is rendered once by an in-tree image generator
  (`pattern=checkerboard|hstripes|vstripes` + aliases / `grating` /
  `plasma`; remaining query keys are forwarded unchanged so the base
  frame is bit-identical to the matching still-image generator's
  output), then frame `n` is exactly the base frame translated by
  `(n·vx, n·vy)` pixels with toroidal wrap-around addressing:
  `frame_n(x, y) = base((x − n·vx) mod w, (y − n·vy) mod h)`.
  `vx` / `vy` are signed integer pixels-per-frame (defaults 1 / 0;
  parsed by a new shared `q_i32` query helper), so the true motion
  field is globally constant and known exactly — every output pixel is
  a bit-exact copy of a base-frame pixel (one wrapped row lookup plus
  two contiguous byte copies per output row; no resampling, no
  interpolation). Useful for validating codec motion search (the
  estimated vector field should be uniformly `(vx, vy)`), temporal
  prediction (frame `n` predicted from frame `n−1` with the true
  vector is residual-free), and wrap-period logic (when `vx` divides
  `w` the sequence is periodic with period `w / vx` frames). Eleven
  unit tests cover the per-pixel ground-truth translation property on
  a plasma base, static `vx=vy=0`, torus velocity algebra
  (`vx=−2 ≡ vx=14` and `vx=20 ≡ vx=4` on a 16-wide frame), full-period
  wrap back to the base frame, frame-0 bit-identity with direct
  `pattern` / `grating` renders, vy-only row shifting with seam wrap,
  `duration × fps` frame counts, unknown-pattern and fractional-velocity
  rejection; plus a URI roundtrip, a zero-input `video.scroll` filter
  test, and `scroll:` shorthand rows. Exposed on all three surfaces:
  `generate://scroll?…`, the `scroll:` shorthand (bare + query
  passthrough), and the `video.scroll` filter.
- Audio synth gained `vibrato` (alias `vib`) — classical musical vibrato,
  the phase-domain sister of the in-tree `tremolo`. Instantaneous
  frequency traces a cosine around the carrier,
  `f_inst(t) = freq · (1 + depth · cos(2π · lfo · t))`, so `depth=` is
  the FRACTIONAL frequency deviation (default `0.005` = ±0.5 %, a
  textbook "natural" sung-vowel vibrato width; classical string vibrato
  is closer to ±2 %). Integrating gives the closed-form phase
  `φ(t) = 2π·freq·t + (depth·freq / lfo)·sin(2π·lfo·t)`, so the
  modulation index in the FM sense is exactly `β = depth · freq / lfo`
  radians (e.g. 440 Hz × 0.005 / 5 Hz ⇒ β ≈ 0.44 rad). `lfo=0` collapses
  the modulation algebraically — the cosine freezes at 1, the
  instantaneous frequency becomes `freq · (1 + depth)`, and the
  implementation special-cases the divide so f32 division-by-zero never
  leaks through. Carrier `wave=` selects
  `sine | square | triangle | sawtooth` exactly like `tremolo`, with
  the non-sine carriers evaluated on the fractional phase coordinate
  `φ(t) / TAU mod 1.0`. Distinct from in-tree `fm` (audio-rate
  modulator + unbounded modulation index) and from in-tree `tremolo`
  (amplitude domain, same family, dual domain). Eleven unit tests cover
  (a) `depth=0` collapsing sample-for-sample to the unmodulated
  carrier, (b) high-depth/fast-LFO bound-respect on a square carrier,
  (c) `lfo=0` collapsing to a pitch-shifted carrier
  (`freq · (1 + depth)`), (d) zero-crossing-rate asymmetry between LFO
  peak (≈ 1500 Hz density) and trough (≈ 500 Hz density) at
  `f=1 kHz, depth=0.5, lfo=1 Hz`, (e) `type=vibrato` / `type=vib` alias
  equivalence, (f) dispatcher `depth=2` silently clamping to 1,
  (g) unknown carrier `wave=` surfacing a `vibrato`-tagged error,
  (h) the dispatcher's "unknown type" hint advertising `vibrato`,
  (i) carrier-wave shape parity (all four oscillators rendering
  distinguishable output, `saw` aliasing to `sawtooth`), (j) family
  separation — vibrato, tremolo, and `fm` produce three different
  buffers at matched parameters, (k) the closed-form modulation-index
  identity — `vibrato("sine", freq, lfo, depth, …)` is sample-for-
  sample equal to `fm(freq, lfo, depth·freq/lfo, …)`. Plus a single-
  frame URI roundtrip in `tests/source_uri.rs` confirming
  `generate://synth?type=vibrato&duration=0.05` returns one
  `AudioFrame` of 400 mono S16 LE samples (800 bytes) inside the
  amplitude=0.8 S16 bound. Reaches the URI path
  (`generate://synth?type=vibrato&…`), the `synth:` shorthand (via the
  existing comma-arg parser), and the `audio.synth` filter through the
  existing dispatcher (no new registration). Pure first-principles DSP;
  sole reference is John Backus, *The Acoustical Foundations of Music*
  (W. W. Norton, 1969 ch. 8 "Vibrato"), a public academic monograph on
  musical acoustics.

- Image catalogue gained `grating` — sinusoidal grating, the canonical
  single-tone spatial-frequency probe from Fourier image analysis.
  Every pixel is set to
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
  `A=255`, same rendering convention as the in-tree zone plate.
  Distinct from `zoneplate` (radial chirp `cos(k·r²)` — every spatial
  frequency simultaneously): the grating isolates one
  `(magnitude, direction)` pair. Distinct from `pattern` (axis-aligned
  step / checker): the grating is C∞-smooth, the pattern is piecewise-
  constant. Eleven unit tests cover (a) dimensions match the query,
  (b) `amplitude=0` collapses to flat mid-grey, (c) `freq=0 phase=0`
  is flat peak white, (d) `freq=0 phase=180` is flat black,
  (e) horizontal grating (`angle=0`) is constant down each column,
  (f) vertical grating (`angle=90`) is constant across each row,
  (g) `freq=1` on a 16-wide image hits the cos(0)=1 / cos(π)=-1 /
  cos(π/2)=0 landmarks, (h) `freq=2` produces two white peaks with a
  matching trough, (i) `angle=45` breaks both axis symmetries,
  (j) `amplitude=2` clamps and renders byte-identical to
  `amplitude=1`, (k) alpha is opaque, plus a single-frame URI
  roundtrip in `tests/source_uri.rs` confirming
  `generate://grating?w=4&h=4&freq=0&phase=0&amplitude=1` returns one
  4×4 RGBA frame of all-white pixels (64 bytes) and a
  `tests/filter_zero_input.rs` entry confirming `image.grating` is a
  zero-input one-output filter. Reaches the URI path
  (`generate://grating?…`), the `grating:` shorthand prefix (with the
  trailing tail used verbatim as the query string), and the
  `image.grating` filter through the existing registration. Pure
  first-principles maths; the cos-of-linear-phase grating is textbook
  Fourier analysis.

- Audio synth gained `shepard` — a Shepard tone, the classical
  octave-circular-pitch construct described in Roger Shepard's 1964
  *Journal of the Acoustical Society of America* paper "Circularity in
  Judgments of Relative Pitch" (vol. 36 no. 12 p. 2346). The output is
  the weighted sum of `voices` sine tones spaced one octave apart
  starting at `freq`, each scaled by a Gaussian envelope in log-
  frequency space centred on `center_freq` with width `sigma` octaves
  (`w_k = exp(-(log2(f_k / center_freq) / sigma)²)`), then normalised
  by `Σ w_k` so the worst-case aligned peak sits at `amplitude`.
  Defaults: `freq=55` (lowest voice), `voices=8` (clamped to
  `[1, 12]`), `sigma=1.5` octaves (clamped to `[0.1, 6.0]`), and
  `center_freq = freq · 2^((voices−1)/2)` — the geometric mean of the
  voice frequencies, i.e. the log-midpoint of the octave stack (≈ 622
  Hz for the default 55 Hz × 8 voices). The Gaussian log-envelope is
  the canonical Shepard shape: bottom and top voices stay quiet while
  the middle of the stack carries the energy, the absolute frequency
  range stays bounded across the whole stack, and a sweep of `freq`
  produces a pitch percept that rises while the spectrum stays
  enveloped. Distinct from the in-tree `multitone` (flat equal-weight
  sum of an arbitrary frequency list — no log-envelope, no octave
  constraint, no centre/sigma). Eight unit tests cover the basic
  render shape + bounded amplitude, the `voices=1` collapse to plain
  `sine` at the same frequency (the weight normalisation cancels the
  single Gaussian term), octave-spacing via a single-bin DFT (the
  fundamental and octave bins both register meaningfully while a
  1.3·f0 probe is much quieter), `center_freq` reshaping the mix
  while keeping output bounded, the `freq ≤ 0` / `center_freq ≤ 0`
  error paths, `voices=100` clamping silently to 12, the dispatcher's
  "unknown type" hint advertising `shepard`, and the default-centre
  algebraic identity that the implicit log-midpoint equals an
  explicit `center_freq = freq · 2^((voices−1)/2)`. Plus a single-
  frame URI roundtrip in `tests/source_uri.rs` confirms
  `generate://synth?type=shepard&voices=6&duration=0.05` returns one
  `AudioFrame` of 400 mono S16 LE samples (800 bytes) with peak
  inside the `amplitude=0.8` S16 bound. Pure first-principles DSP;
  sole reference is the 1964 Shepard JASA paper. Reaches the URI
  path (`generate://synth?type=shepard&…`), the `synth:` shorthand
  via the existing comma-arg parser, and the `audio.synth` filter
  through the existing dispatcher (no new registration).

- Audio synth gained `tremolo` (alias `trem`) — a sub-audio amplitude
  envelope laid over an arbitrary carrier wave. The carrier is selected
  via `wave=sine|square|triangle|sawtooth` (mirroring the `adsr`
  carrier list); each sample is then scaled by the unipolar cosine
  envelope `e(t) = 1 − depth · 0.5 · (1 − cos(2π · lfo · t))`, which
  sits exactly in `[1 − depth, 1] ⊆ [0, 1]` so the output is bounded
  by `amplitude` for every `(wave, freq, lfo, depth)`. Defaults are
  `wave=sine`, `freq=440`, `lfo=5`, `depth=0.5` — five-cycles-per-
  second amplitude swell, the classical guitar-amp / Leslie tremolo
  speed. Distinct from the existing `am`: tremolo's envelope is
  unipolar (strict attenuation, never crosses zero) and runs at sub-
  audio LFO rates (0–20 Hz typical), while `am` uses a bipolar
  audio-rate sinusoidal modulator with prosthaphaeresis sidebands at
  `fc ± fm` and the leading 0.5 normalisation; tremolo's spectrum
  stays centred on `fc` with low-frequency sidebands at `fc ± lfo`
  that read perceptually as periodic loudness variation rather than
  a new timbre, and the carrier can be any of the four in-tree
  oscillators rather than just a sine. Eleven new tests cover (a) the
  `depth=0` collapse to the pure carrier sample-for-sample, (b) the
  amplitude bound on a non-trivial 44.1 kHz × 4096-sample square-
  carrier render (square is the worst case because every sample
  already sits at the rail), (c) the envelope-range identity that
  positive samples span exactly `[amp·(1 − d), amp]` over an integer
  number of LFO periods, (d) RMS-energy quartile divergence between
  the LFO peak and trough (proves the LFO is actually steering the
  gain), (e) the `lfo=0` algebraic collapse to the pure carrier (the
  cosine freezes at 1, leaving the envelope identically 1), (f)
  categorical divergence from `am` at matched depth (different
  spectrum — same headline parameter), (g) `type=tremolo` / `type=trem`
  alias byte-equivalence with default-bounded output, (h) dispatcher
  depth-clamp to [0, 1] (out-of-range value matches the explicit-
  clamped render bit-for-bit), (i) the unknown-wave error path
  surfaces both `tremolo` and the offending wave name, (j) the
  catalogue listing tremolo in the "unknown type" help, and (k)
  carrier-shape parity — each of `sine` / `square` / `triangle` /
  `sawtooth` (with `saw` aliasing `sawtooth`) produces a
  distinguishable buffer at the same LFO config, so the wave selector
  is honoured end-to-end. Pure first-principles DSP; mathematical
  reference is the standard amplitude-modulation result that a non-
  negative low-frequency envelope on a band-limited carrier shifts
  spectral energy by ±lfo without injecting an AM-style suppressed-
  carrier component (textbook material in Moore, *Elements of
  Computer Music*, 1990, chapter 4 on classic analogue effects).
  Reaches the URI path (`generate://synth?type=tremolo&…`), the
  `synth:` shorthand via the existing comma-arg parser, and the
  `audio.synth` filter through the existing dispatcher (no new
  registration).

- Image noise gained `value` (alias `lattice`) — classical value noise,
  the textbook predecessor to gradient noise that Ken Perlin's 1985
  SIGGRAPH paper *An Image Synthesizer* introduced before moving on to
  gradient noise. Each integer lattice point holds a pseudo-random
  scalar in `[-1, 1]`; a sample at `(x, y)` smoothstep-interpolates
  the four surrounding lattice values. The lattice values come from
  the existing seeded 512-entry permutation table (`build_perm`) —
  `perm[(perm[ix & 0xFF] + iy) & 0xFF]` is a `u8` deterministically
  hashed from `(ix, iy, seed)`, then remapped from `[0, 255]` to
  `[-1, 1]` via `(h · 2 / 255) − 1`. The smoothstep is the same quintic
  `t³·(t·(6t − 15) + 10)` `fade` curve `perlin2` uses, so the surface
  is C²-continuous across every lattice boundary. Output is bounded by
  `[-1, 1]` exactly because both the corner values and the
  interpolation weights are bounded that way (a convex combination of
  values in `[-1, 1]` stays in `[-1, 1]`), which matches
  `perlin2` / `simplex2` so the shared multi-octave fBm accumulator,
  palette mapping, `scale=` / `octaves=` / `seed=` parameters all work
  unchanged — `value` is the third basis on the same accumulator
  alongside `perlin` and `simplex`. Distinct from gradient noise:
  value noise has axis-aligned blocky low-frequency character because
  the lattice scalars (not gradients of a hidden field) carry the
  signal, which is exactly why Perlin moved on from it. Ten new tests
  cover basic render shape, `value` / `lattice` alias byte-
  equivalence, seed determinism + seed divergence, categorical
  distinctness from `perlin` and `simplex` at the same seed/scale
  (different algorithm), distinctness from `worley` too (third
  independent basis), the raw-sample `[-1, 1]` boundedness invariant
  over a 200×200 grid plus a non-degeneracy `|v| > 0.3` floor,
  integer-lattice corner identity (`value2(perm, 3.0, 5.0)` must equal
  the corner's own remapped scalar because `fade(0) = 0` zeros the
  neighbour contributions), palette-bounded output (≥ 8 distinct
  colours in a 48×48 render), and the unknown-type error message now
  advertising `value`. Pure first-principles maths; reference is
  Perlin's 1985 SIGGRAPH paper. Reaches the URI path
  (`generate://noise?type=value&…`), the `noise:value` shorthand (via
  the existing `noise:<type>` prefix), and the `image.noise` filter
  through the existing dispatcher (no new registration).

- Image noise gained `worley` (alias `cellular`) — Worley / cellular
  noise, a spatial-point-process texture distinct from the existing
  Perlin / simplex gradient-noise modes. The plane is divided into
  integer cells of side `scale` pixels; each cell holds `points ∈ [1, 4]`
  pseudo-randomly placed feature points (`points=1` is the canonical
  Voronoi / "stone wall" texture); each pixel scans the 3×3
  neighbourhood of cells around its home cell, gathers every feature-
  point distance, and palette-maps the k-th closest distance (`k ∈
  [1, 4]`, default 1 — the F1 distance; `k=2` is the F2 distance). The
  distance metric is selectable via `dist=euclidean|euc|l2` (default),
  `manhattan|l1`, `chebyshev|linf|max` — same Voronoi cell structure,
  three different falloff shapes (circle, diamond, square). Pseudo-
  random feature-point placement uses the same in-tree LCG the rest of
  the module already uses, keyed by `(cell_x, cell_y, slot, seed)`, so
  the same `seed=` is bit-deterministic across builds and the same
  seed semantics already documented for `perlin` / `simplex` apply
  here. Twelve new tests cover the alias byte-equivalence
  (`worley` ≡ `cellular`), seed determinism, seed divergence,
  categorical distinctness from both gradient-noise modes at the same
  seed, the three-metric render-and-differ matrix, k=1 vs k=2
  divergence, points=1 vs points=3 divergence, the
  feature-point-inside-cell placement contract over a sweep of negative
  + positive cell coordinates, palette-bounded output (no panic on the
  indexed access; ≥ 8 distinct colours in a 48×48 render), and the
  unknown-metric error path. Mathematical reference is Steven Worley,
  *A Cellular Texture Basis Function*, SIGGRAPH 1996 proceedings — a
  public academic paper on cellular-noise basis functions. Pure
  first-principles maths.

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

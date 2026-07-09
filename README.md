# oxideav-generator

[![CI](https://github.com/OxideAV/oxideav-generator/actions/workflows/ci.yml/badge.svg)](https://github.com/OxideAV/oxideav-generator/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/oxideav-generator.svg)](https://crates.io/crates/oxideav-generator) [![docs.rs](https://docs.rs/oxideav-generator/badge.svg)](https://docs.rs/oxideav-generator) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Pure-Rust synthetic media generator for the oxideav framework. Provides
audio synth (sine / square / triangle / sawtooth / supersaw
(detuned-sawtooth stack) / pulse-width-modulated rectangle /
Karplus-Strong pluck / linear + exponential chirp / FM / AM /
sub-audio tremolo (unipolar-cosine LFO over any carrier) /
sub-audio vibrato (closed-form integrated-phase FM over any carrier) /
ring modulation / DTMF touch-tones / ADSR-enveloped tone / Klatt-style
two-formant vowel synthesizer / Shepard tone (octave-spaced
Gaussian-weighted sine stack) / multi-tone /
white-pink-brown-blue-violet noise / silence / DC (constant offset) /
impulse train (drift-free integer period); sine takes `phase=` +
per-channel `chphase=` offsets for stereo-correlation probes),
image basics (solid colour, linear / radial gradient,
checkerboard, horizontal / vertical stripes, sinusoidal grating —
single-frequency cos at a chosen orientation, per-channel quantised
`ramp` at configurable 1–8-bit depth — the banding / dithering /
bit-depth-conversion probe), procedural imagery
(Mandelbrot + Julia fractals, plasma, Perlin + simplex gradient
noise, value / lattice noise, Worley cellular noise), and video
(classical broadcast `testsrc`, SMPTE colour bars, animated Mandelbrot
zoom, hue-rotating gradient, zone-plate `cos(k·r²)` spatial-frequency
probe, constant-velocity toroidal `scroll` — bit-exact ground-truth
motion-estimation probe, rotating `colorwheel` — polar hue from the
`atan2` angle with radial saturation, a chroma + angular-motion probe,
`movingbox` — a solid rectangle translating at exactly-known signed
integer pixels-per-frame over a solid background, the local-motion
ground-truth probe, seeded temporal `snow` noise — every pixel a
stateless hash of `(seed, frame, x, y)`, the worst-case-entropy
rate-control stress input).

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

Runtime-dependent only on `oxideav-core` and `serde_json` — no `image`,
no `png`, no `wav` crate, no `rand`. LCG / Perlin / diamond-square are
all hand-rolled in tree. The default-on `label` feature pulls in
`oxideav-scribe` + `oxideav-raster` for a text-to-image generator
(`label:` / `generate://label?text=…`); opt out with
`--no-default-features` to drop the bundled font (~340 KB) and that dep
tree when text rendering is not needed.

## URI catalogue

```
generate://synth?type=sine&freq=440&duration=5
generate://synth?type=sine&freq=440&phase=90&duration=5          # cosine
generate://synth?type=sine&freq=440&channels=2&chphase=90&duration=5   # quadrature stereo
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
generate://synth?type=vibrato&wave=sine&freq=440&lfo=5&depth=0.005&duration=2
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
generate://synth?type=dc&level=-0.25&duration=1
generate://synth?type=impulse&freq=4&duration=2
generate://synth?type=impulse&period=100&width=3&duration=1

generate://xc?color=red&w=640&h=480
generate://xc?color=%23ff0000      # #ff0000 percent-encoded
generate://gradient?w=640&h=480&from=red&to=blue&direction=horizontal
generate://gradient?w=640&h=480&from=red&to=blue&type=radial
generate://pattern?type=checkerboard&w=640&h=480&size=32
generate://grating?w=640&h=480&freq=8&angle=0&phase=0
generate://grating?w=640&h=480&freq=16&angle=45&amplitude=0.7
generate://ramp?w=256&h=64&bits=8                    # identity ramp: value(x) = x
generate://ramp?bits=2&channel=r&direction=vertical  # 4-level red-only banding
generate://fractal?type=mandelbrot&w=640&h=480&cx=-0.5&cy=0&zoom=2&iter=256
generate://fractal?type=julia&w=640&h=480&cx=-0.7&cy=0.27&iter=256
generate://plasma?w=640&h=480&seed=42
generate://noise?type=perlin&w=640&h=480&scale=64&seed=42
generate://noise?type=simplex&w=640&h=480&scale=64&octaves=4&seed=42
generate://noise?type=value&w=640&h=480&scale=64&octaves=4&seed=42
generate://noise?type=worley&w=640&h=480&scale=48&seed=42
generate://noise?type=worley&dist=manhattan&k=2&points=2&w=640&h=480&scale=48&seed=42
generate://label?text=Hello%20world&color=black&bg=white&padding=4   # needs `label` feature

generate://testsrc?w=640&h=480&duration=5&fps=30
generate://smptebars?w=640&h=480&duration=5&fps=30
generate://zoneplate?w=640&h=480&duration=5&fps=30&k=0.05&motion=temporal
generate://scroll?pattern=checkerboard&size=32&vx=2&vy=1&w=640&h=480&duration=5&fps=30
generate://scroll?pattern=plasma&seed=7&vx=-3&w=640&h=480&duration=5&fps=30
generate://colorwheel?w=640&h=480&duration=5&fps=30&spin=60
generate://colorwheel?spin=-90&lightness=0.5&saturation=1&w=640&h=480
generate://movingbox?w=640&h=480&bw=32&bh=32&vx=3&vy=-2&duration=5&fps=30
generate://movingbox?bw=16&bh=16&x0=100&y0=50&fg=red&bg=gray&vx=2
generate://snow?w=640&h=480&duration=5&fps=30&seed=42
generate://snow?mode=rgb&seed=7&w=320&h=240
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
| `scroll:`              | `generate://scroll`                                          |
| `colorwheel:`          | `generate://colorwheel`                                      |
| `movingbox:`           | `generate://movingbox`                                       |
| `snow:`                | `generate://snow`                                            |
| `ramp:`                | `generate://ramp`                                            |
| `noise:perlin`         | `generate://noise?type=perlin`                               |
| `label:Hello world`    | `generate://label?text=Hello%20world` (needs `label` feature)|

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

Feature-complete and stable. All catalogue entries above reach the
source-driver (`generate://` URI), the CLI shorthand, and the
zero-input `StreamFilter` paths, each covered by unit tests plus a URI
round-trip and a zero-input-filter test. Audio, image, and video URIs
all produce frames natively (no intermediate encode/decode round-trip).
Every generator is built from first principles — DSP identities,
textbook Fourier/noise maths, and the in-tree LCG / Perlin permutation
table — with no external media-encoding dependencies.

See `CHANGELOG.md` for the per-release history.

## CSS colour parser

Hand-rolled. Accepts a curated subset of the CSS/HTML4 named colours
plus `#RGB`, `#RGBA`, `#RRGGBB`, and `#RRGGBBAA`.

## Determinism

All randomness is seeded — every generator that takes a `seed=` query
parameter is bit-deterministic across builds. The whole catalogue is
under a byte-determinism contract enforced by
`tests/catalogue_determinism.rs`: every generator kind (28 synth
types, 15 image forms, 11 video forms), opened twice with identical
parameters, must produce byte-identical audio PCM / video plane data. Defaults: `seed=42` for
plasma / Perlin / simplex / value, `seed=0x12345678` for white / pink
/ brown noise. Perlin, simplex, and value all draw from the same
seeded 512-entry permutation table, so a given `seed=` is reproducible
across all three gradient / lattice modes.

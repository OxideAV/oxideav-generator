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
image basics (solid colour, linear / radial / conic gradients with
stop lists, spread methods and sRGB / linear / Oklab / CIELAB
interpolation, a two-colour pattern set, test charts, seeded random
fields in seven distributions — all at 8-bit, 16-bit or float, grey or
RGB with or without alpha, optionally dithered, sinusoidal grating —
single-frequency cos at a chosen orientation, per-channel quantised
`ramp` at configurable 1–8-bit depth — the banding / dithering /
bit-depth-conversion probe, identity Hald CLUT `hald` at level 2–16),
procedural imagery
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

Runtime-dependent only on `oxideav-core`, `oxideav-pixfmt` (colour
models) and `serde_json` — no `image`, no `png`, no `wav` crate, no
`rand`. LCG / Perlin / diamond-square are
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
generate://gradient?from=red&to=blue&direction=NorthWest            # IM compass point
generate://gradient?from=red&to=blue&angle=30                       # degrees clockwise from North
generate://gradient?from=red&to=blue&vector=10,10,300,200&bbox=320x240%2B10%2B10
generate://gradient?from=black&to=white&colorspace=linear           # linear-light interpolation
generate://radial-gradient?from=white&to=black&extent=Ellipse
generate://radial-gradient?from=white&to=black&center=100,80&radii=60,40
generate://radial-gradient?center=40,30&extent=farthest-corner
generate://gradient?stops=red,white%2030%25,blue&angle=30          # multi-stop ("white 30%")
generate://gradient?from=red&to=blue&colorspace=oklab               # perceptual interpolation
generate://gradient?from=red&to=blue&colorspace=oklch&hue=longer    # polar, the long way round
generate://gradient?vector=0,0,64,0&spread=reflect                  # pad | repeat | reflect
generate://conic-gradient?stops=red,yellow,lime,cyan,blue,magenta,red
generate://conic-gradient?from=black&to=white&center=100,80&angle=90
generate://radial-gradient?radii=200,80&angle=30                    # rotated ellipse
generate://gradient?from=black&to=white&depth=16&channels=gray      # 16-bit grey ramp
generate://gradient?from=%23101010&to=%23202020&dither=noise        # no 8-bit banding
generate://pattern?type=checkerboard&w=640&h=480&size=32
generate://pattern?type=bricks&size=24&line=2&color1=firebrick&color2=silver
generate://pattern?type=gray25                                      # ordered 25 % fill
generate://chart                                                    # captioned composite test chart
generate://chart?type=bars&level=100&depth=16
generate://chart?type=zoneplate&w=512&h=512
generate://random?type=gaussian&amount=0.1&seed=7                   # per-pixel noise
generate://random?type=poisson&amount=64&color=gray&mono=1
generate://noise?type=perlin&from=navy&to=white&depth=16&octaves=6&gain=0.6
generate://grating?w=640&h=480&freq=8&angle=0&phase=0
generate://grating?w=640&h=480&freq=16&angle=45&amplitude=0.7
generate://ramp?w=256&h=64&bits=8                    # identity ramp: value(x) = x
generate://ramp?bits=2&channel=r&direction=vertical  # 4-level red-only banding
generate://hald?level=8                              # 512x512 identity Hald CLUT
generate://fractal?type=mandelbrot&w=640&h=480&cx=-0.5&cy=0&zoom=2&iter=256
generate://fractal?type=julia&w=640&h=480&cx=-0.7&cy=0.27&iter=256
generate://plasma?w=640&h=480&seed=42
generate://plasma?from=red&to=blue&seed=42                          # IM plasma:red-blue
generate://plasma?type=fractal&seed=42                              # IM plasma:fractal
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
| `radial-gradient:red-blue` | `generate://gradient?type=radial&from=red&to=blue`       |
| `gradient:red-blue,angle=45` | `generate://gradient?from=red&to=blue&angle=45`        |
| `plasma:`              | `generate://plasma`                                          |
| `plasma:red-blue`      | `generate://plasma?from=red&to=blue`                         |
| `plasma:fractal`       | `generate://plasma?type=fractal`                             |
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
| `hald:8`               | `generate://hald?level=8`                                    |
| `noise:perlin`         | `generate://noise?type=perlin`                               |
| `noise:gaussian`       | `generate://random?type=gaussian`                            |
| `random:poisson`       | `generate://random?type=poisson`                             |
| `canvas:red`           | `generate://xc?color=red`                                    |
| `conic-gradient:red-blue` | `generate://gradient?type=conic&from=red&to=blue`         |
| `gradient:red-white-blue` | `generate://gradient?stops=red,white,blue`                |
| `pattern:bricks,size=16` | `generate://pattern?type=bricks&size=16`                   |
| `chart:bars`           | `generate://chart?type=bars`                                 |
| `label:Hello world`    | `generate://label?text=Hello%20world` (needs `label` feature)|

`probe` / `transcode` / `remux` / `run` accept the canonical
`generate://` URI form only — they don't expand shorthands.

## Still images: depth, layout, dithering, threads

Every still-image generator is a `Field` — a straight-alpha RGBA
colour at each pixel — that `image::canvas::render` delivers in any of
twelve formats:

| `depth=`  | `channels=gray` | `graya`  | `rgb`     | `rgba` (default) |
| --------- | --------------- | -------- | --------- | ---------------- |
| `8`       | `Gray8`         | `Ya8`    | `Rgb24`   | `Rgba`           |
| `16`      | `Gray16Le`      | `Ya16Le` | `Rgb48Le` | `Rgba64Le`       |
| `float`   | f32, direct API only (the framework has no float pixel format) ||||

16-bit samples are `round(65535·v)`; 8-bit samples are that value
rounded to 8 bits, which equals `round(255·v)` — the 8-bit image is
the rounded float image. Gradients, solids, patterns, charts, random
fields and ramp-coloured noise are evaluated at full precision in
every depth; two-colour and fractal plasma keep the fractional part of
their displacement over an 8-bit base; generators defined on 8-bit
levels (palette noise, fractals, `ramp`, `hald`, `label`) are widened
exactly. Colours written as 16-bit hex (`#RRRRGGGGBBBB`) keep all 16
bits.

`dither=ordered` (8 × 8 dispersed-dot) or `dither=noise` (triangular,
seeded by `dither_seed=`) spreads the rounding error of the integer
depths so shallow ramps do not band; a flat area keeps its mean and a
value already on an output level is not touched.

`threads=N` splits the rows into bands (`RenderConfig::with_threads`,
or `with_execution_context` for a framework budget). The default is
serial and the output is byte-identical at every budget.

```rust
use oxideav_generator::image::canvas::{self, Channels, Dither, RenderConfig, SampleDepth};
use oxideav_generator::image::gradient::{
    GradientColorspace, GradientKind, GradientOptions, GradientStop,
};

let field = GradientOptions::new(1920, 1080)
    .with_stops(GradientStop::parse_list("navy, white 40%, orange")?)
    .with_angle(30.0)
    .with_colorspace(GradientColorspace::Oklab)
    .field(GradientKind::Linear)?;
let cfg = RenderConfig::new()
    .with_depth(SampleDepth::U16)
    .with_channels(Channels::Rgb)
    .with_dither(Dither::Noise)
    .with_threads(4);
let image = canvas::render(&field, &cfg);              // or canvas::render_band(&field, &cfg, y0, rows)
assert_eq!(image.as_u16().unwrap().len(), 1920 * 1080 * 3);
# Ok::<(), oxideav_core::Error>(())
```

### Gradients

- **Kinds**: `linear` (compass `direction`, `angle`, or explicit
  `vector`), `radial` (`center`, `radii`, `extent` = Circle / Ellipse /
  Diagonal / Maximum / Minimum / closest-side / farthest-side /
  closest-corner / farthest-corner — the last four as circles or, with
  `shape=ellipse`, as ellipses — and `angle` rotating the ellipse),
  `conic` (`center`, start `angle`, clockwise from North).
- **Stops**: two colours (`from` / `to`) or `stops=c1, c2 30%, c3`;
  missing offsets are spaced evenly, equal offsets make a hard edge.
- **Spread**: `pad`, `repeat`, `reflect`.
- **Interpolation space**: `srgb` (default), `linear`, `oklab`,
  `oklch`, `lab`, `lch`; polar spaces take `hue=shorter | longer |
  increasing | decreasing`. Alpha is premultiplied in every space.
- `bbox=WxH+X+Y` confines the gradient.

The conic angle is computed with `+ − × ÷ √` only, so a conic gradient
is bit-identical on every platform and exact on the eight principal
directions.

### Random fields

`generate://random?type=…` draws every sample from a stateless hash of
`(seed, x, y, channel)`: `random` (uniform over the range), `uniform`,
`gaussian`, `impulse` (salt and pepper), `laplacian`, `poisson`,
`multiplicative` (speckle), around a base `color`, with strength
`amount` and `mono=1` for grey noise. The logarithm and cosine behind
the shaped distributions are arithmetic-only, so a seed gives the same
field on every platform, in any row order, under any thread budget.
Float output keeps the tails (it is not clamped).

### Patterns and charts

`pattern` types: `checkerboard`, `horizontal`, `vertical`, `right45`,
`left45`, `crosshatch`, `crosshatch45`, `bricks`, `verticalbricks`,
`dots`, `circles`, `horizontalsaw`, `verticalsaw`, and the ordered
fills `gray0` … `gray100` — each an integer predicate of the pixel
coordinates (`size=`, `line=`, `color1=`, `color2=`), exact at every
depth. These are this crate's own geometries.

`chart` types: `bars` (eight colour bars at `level=` percent), `steps`
(grey staircase), `ramp`, `zoneplate` (reaching the Nyquist frequency
at the edge of the longer side), `bursts` (line widths 1 / 2 / 4 / 8)
and the default `composite` of all of them, captioned when the `label`
feature is on (`labels=0` to turn the captions off).

## Pseudo-image specifications

`pseudo::PseudoImage` turns a `name:arguments` string into a rendered
image — the entry point for callers that receive specifications such
as `gradient:red-blue` or `plasma:fractal` together with a size, a set
of defines and a seed:

```rust
use oxideav_generator::pseudo::PseudoImage;

let image = PseudoImage::parse("radial-gradient:white-black")?
    .with_size(640, 480)
    .with_define("gradient:extent", "Ellipse")?
    .with_define("gradient:center", "200,120")?
    .render()?;

let clouds = PseudoImage::parse("plasma:fractal")?
    .with_size(640, 480)
    .with_seed(1234)            // same seed, same pixels
    .render()?;
# let _ = (image, clouds);
# Ok::<(), oxideav_core::Error>(())
```

Names: `xc:` / `canvas:`, `gradient:`, `radial-gradient:`,
`conic-gradient:`, `plasma:`, `hald:`, `pattern:`, `noise:`,
`random:`, `chart:`, `mandelbrot:`, `julia:`, `ramp:`, `grating:`,
`label:`. Defines: `gradient:angle`, `gradient:vector`,
`gradient:direction`, `gradient:extent`, `gradient:bounding-box`,
`gradient:center`, `gradient:radii`, plus any bare query key
(`stops`, `spread`, `colorspace`, `hue`, `size`, `amount`, …).
`with_depth` / `with_channels` / `with_dither` / `with_threads` select
the output, and `to_uri()` gives the equivalent `generate://` URI.

## Wiring

```rust
use oxideav_core::RuntimeContext;

let mut ctx = RuntimeContext::new();
oxideav_source::register(&mut ctx);                      // file://
oxideav_generator::register(&mut ctx);                   // generate:// + audio.synth, image.xc, ...
// or the halves individually:
// oxideav_generator::register_source(&mut ctx.sources); // generate:// only
// oxideav_generator::register_filters(&mut ctx);        // filter factories only
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

## Benchmarks

`cargo bench --bench framefill` (criterion; no fixtures, all inputs
synthesised in-bench). Apple-silicon reference numbers, per 320×240
frame unless noted:

| Bench                          | Time      |
| ------------------------------ | --------- |
| video colorwheel               | ~690 µs   |
| video zoneplate                | ~222 µs   |
| video testsrc                  | ~45 µs    |
| video snow (mono / rgb)        | ~18 / 27 µs |
| video movingbox                | ~5.7 µs   |
| video scroll (checkerboard)    | ~51 µs    |
| image perlin noise             | ~1.94 ms  |
| image checkerboard             | ~42 µs    |
| image ramp 8-bit               | ~43 µs    |
| image grating                  | ~189 µs   |
| audio sine, 1 s @ 48 kHz       | ~101 µs   |
| audio supersaw-7, 1 s @ 48 kHz | ~2.22 ms  |
| audio pink noise, 1 s @ 48 kHz | ~51 µs    |

### Still-image throughput

`cargo bench --bench mpps` renders 1920 × 1080 stills and prints
megapixels per second (`-- stages` prints a per-stage breakdown). The
table compares the 0.1.6 release with this tree: both built standalone
with the default bench profile, the two binaries run alternately five
times, medians of seven renders per run, 8-bit RGBA, **one thread**.
The machine (Apple silicon) was heavily shared while measuring (load
average about 50), so the absolute rates are pessimistic and ratios
within about ±15 % of 1 are noise. Output is byte-identical to 0.1.6
for every row (`tests/pinned_bytes.rs`).

| Still (serial, 8-bit RGBA)         | 0.1.6 MP/s | now MP/s | ratio |
| ---------------------------------- | ---------: | -------: | ----: |
| solid colour                       |      15961 |    20832 |  1.3× |
| checkerboard                       |       1377 |     2661 |  1.9× |
| linear gradient, South             |        177 |     9648 |   55× |
| linear gradient, East              |        177 |     6189 |   35× |
| linear gradient, angle = 30°       |        177 |      325 |  1.8× |
| … linear-light interpolation       |       28.1 |      110 |  3.9× |
| radial gradient                    |        111 |      255 |  2.3× |
| … linear-light interpolation       |       27.3 |     81.4 |  3.0× |
| plasma (palette)                   |        162 |      187 |  1.2× |
| plasma (two colours)               |       38.1 |     52.4 |  1.4× |
| plasma (fractal)                   |       49.9 |     51.8 |  1.0× |
| Perlin noise, 4 octaves            |       27.9 |     34.2 |  1.2× |
| simplex noise, 4 octaves           |       17.7 |     16.8 |  1.0× |
| value noise, 4 octaves             |       42.6 |     64.7 |  1.5× |
| Worley noise                       |       43.1 |     67.9 |  1.6× |

Simplex noise is not separable and was left as it is.

Where the time went, and what changed (ns per pixel before the work,
from `-- stages`): an angled sRGB gradient spent 1.0 on the gradient
parameter, 3.9 on the colour and 2.3 on packing; linear-light spent 20
of 26 in three power functions; Perlin noise 29 of 33 in the lattice
sums.

- **Rows that repeat are copied.** A row identical to an earlier one
  (axis-aligned gradients, periodic patterns, flat canvases, chart
  bands) is a memory copy; a row of one colour is filled by doubling.
- **Two-colour gradients run in flat batches**: parameter, clamp and
  the three mixes are separate loops over 256 pixels that the compiler
  vectorises, each value produced by the same expression as before.
- **The sRGB transfer function is a table at integer depths.** The map
  from linear light to the 16-bit code is a step function; its 65 535
  step positions are located to the exact `f64` with the reference
  function, so a lookup returns what the power function would have —
  verified at every step and bucket edge.
- **Lattice noise is separable per octave**: the terms that depend
  only on the column are tabulated once per image and those that
  depend only on the row once per row. Worley noise builds the feature
  points of the three cell rows once per span and ranks candidates by
  squared distance.
- **Plasma only computes the part of the power-of-two grid that can
  reach the canvas** (the rest only advances the random sequence), and
  the three channel fields run side by side under a thread budget.

New generators, same run: 16-bit and float angled gradients 345 and
509 MP/s, five-stop gradient 126, Oklab 36, OkLCh 27, conic 52, bricks
/ dots 3800, composite chart 377, zone plate 67, uniform-range noise
186, Gaussian 19, Poisson 29. With `threads=4`: angled gradient 1214
MP/s, conic 258, Gaussian noise 89, Perlin noise 194.

## CSS colour parser

Hand-rolled. Accepts the 148 CSS Color 4 named colours (plus
`transparent` / `none`), the X11 `gray0`…`gray100` percent greys,
`rgb()` / `rgba()` / `srgb()` / `srgba()` / `gray()` / `hsl()` /
`hsla()` (numbers or percentages), and `#RGB`, `#RGBA`, `#RRGGBB`,
`#RRGGBBAA`, `#RRRRGGGGBBBB`, `#RRRRGGGGBBBBAAAA`. Bare `gray` / `grey`
follow CSS (128); ImageMagick's own table uses 126 / 190.

## ImageMagick pseudo-image parity

`gradient` / `radial-gradient` accept the ImageMagick 7
`-define gradient:` keys as query parameters (`direction`, `angle`,
`vector`, `extent`, `bbox`/`bounding-box`, `center`, `radii`) and
match `magick … -depth 8` output within one level per channel
(`tests/magick_parity.rs`, run when `magick` is on `PATH`). The one
deliberate difference: ImageMagick 7.1.2 copies the left neighbour into
the gradient vector's start pixel, while this crate renders the defined
value. `extent` also takes `closest-side` / `farthest-side` /
`closest-corner` / `farthest-corner`, circles measured from the radial
centre (ImageMagick has no such extents; these are additions).
`plasma:c1-c2` / `plasma:fractal` match ImageMagick's
statistics (gradient direction, noise spread, smoothness), not its
pixels, because the plasma recursion and random source are not
documented. `hald:N` is byte-identical to `magick hald:N -depth 8`
(levels 2–16; a level below 2 selects 8, fractions truncate).

## Determinism

All randomness is seeded — every generator that takes a `seed=` query
parameter is bit-deterministic across builds. The whole catalogue is
under a byte-determinism contract enforced by
`tests/catalogue_determinism.rs`: every generator kind (28 synth
types, 43 image forms, 11 video forms), opened twice with identical
parameters, must produce byte-identical audio PCM / video plane data. Defaults: `seed=42` for
plasma / Perlin / simplex / value, `seed=0x12345678` for white / pink
/ brown noise. Perlin, simplex, and value all draw from the same
seeded 512-entry permutation table, so a given `seed=` is reproducible
across all three gradient / lattice modes.

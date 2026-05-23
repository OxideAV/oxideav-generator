# Changelog

All notable changes to oxideav-generator are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

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
  is an error. Pure first-principles DSP, no spec or external-library
  dependency; reaches the URI path, the `synth:` shorthand, and the
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
  the ITU-T Q.23 / Q.24 keypad; pure first-principles DSP, no spec or
  external-library dependency. Exposed via the existing `synth:`
  shorthand and `audio.synth` filter (no new registration).
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
- Video generators (ffmpeg-style `testsrc`, SMPTE 75% colour bars,
  animated Mandelbrot zoom, hue-rotating gradient) wired through the
  filter API; the URI source path for video returns a clear
  "unsupported until we add a Y4M demuxer" error.
- Zero-input filter wrappers for every generator, registered as
  `audio.synth`, `image.{xc,gradient,pattern,fractal,plasma,noise}`,
  `video.{testsrc,smptebars,fractal_zoom,gradient_animate}`.
- ImageMagick / sox style CLI shorthand translator
  (`xc:red`, `gradient:red-blue`, `synth:5,sine,440`, …) under
  `oxideav_generator::shorthand::translate`.
- Hand-rolled CSS colour parser (named colours + `#RGB(A)` /
  `#RRGGBB(AA)` hex).

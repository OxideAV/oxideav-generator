# Changelog

All notable changes to oxideav-generator are documented here.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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

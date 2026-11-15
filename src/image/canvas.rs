//! Output side of the still-image generators: sample depth, channel
//! layout, dithering, and the row-band driver that turns a
//! [`Field`] (a colour defined at every pixel) into an [`Image`].
//!
//! # Model
//!
//! A generator is a [`Field`]: for a pixel `(x, y)` it yields a
//! straight-alpha `[r, g, b, a]` colour as `f64`, nominally in
//! `[0, 1]`, sRGB-encoded. The driver ([`render`] / [`render_band`])
//! asks the field for whole rows and packs them into the requested
//! [`SampleDepth`] and [`Channels`]:
//!
//! - **float** — the value itself, as `f32`, unclamped;
//! - **16-bit** — `q16 = round(65535 · clamp(v, 0, 1))`;
//! - **8-bit** — `(q16 + 128) div 257`, i.e. the 16-bit sample
//!   rounded to 8 bits. This equals `round(255 · v)` for every `v`, so
//!   the 8-bit output is the rounded float output and the top byte
//!   relation between the 16-bit and 8-bit renders is exact.
//!
//! Channel reduction: `Rgb` drops alpha; `Gray` / `GrayAlpha` use the
//! red channel when `r = g = b` and otherwise the weighted sum
//! `0.2126 r + 0.7152 g + 0.0722 b` of the encoded components.
//!
//! # Dithering
//!
//! Integer outputs can be dithered to break up banding (smooth ramps
//! at 8 bits show visible steps). The dither adds a zero-mean offset
//! `d`, in output-level units, before rounding:
//!
//! - [`Dither::Ordered`] — the 8 × 8 dispersed-dot threshold map
//!   `M₈` built by the recurrence `M₂ₙ = [4Mₙ, 4Mₙ+2; 4Mₙ+3, 4Mₙ+1]`,
//!   `d = (M₈[y mod 8][x mod 8] + ½) / 64 − ½`.
//! - [`Dither::Noise`] — triangular-density noise `d = u₁ − u₂` with
//!   `u₁`, `u₂` uniform in `[0, 1)`, drawn from a stateless hash of
//!   `(x, y)`; a flat grey no longer shows a repeating texture. Within
//!   half a level of black or white the offset narrows to the
//!   rectangular `u₁ − ½`, which the range ends cannot clip.
//!
//! Both keep the mean of a flat area equal to its true value, and a
//! value that already sits on an output level is left on it (there is
//! no rounding error to spread). The same offset is applied to every channel of a
//! pixel, so neutral greys stay neutral. Dithering never touches float
//! output.
//!
//! # Threads
//!
//! The driver is serial unless [`RenderConfig::with_threads`] grants a
//! budget; rows are then split into contiguous bands, one worker per
//! band. Every pixel is a pure function of its coordinates, so the
//! output is byte-identical at any budget.

use oxideav_core::PixelFormat;

use super::Rgba8Image;

/// Storage type of one sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SampleDepth {
    /// Unsigned 8-bit, `0..=255`.
    #[default]
    U8,
    /// Unsigned 16-bit, `0..=65535`.
    U16,
    /// 32-bit float, nominal range `0.0..=1.0`.
    F32,
}

impl SampleDepth {
    /// `8` / `u8`, `16` / `u16`, `float` / `f32` / `32`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "8" | "u8" => Self::U8,
            "16" | "u16" => Self::U16,
            "float" | "f32" | "32" | "32f" | "hdr" => Self::F32,
            _ => return None,
        })
    }
}

/// Channel layout of the output, samples interleaved in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Channels {
    /// One luminance sample.
    Gray,
    /// Luminance, alpha.
    GrayAlpha,
    /// Red, green, blue.
    Rgb,
    /// Red, green, blue, straight alpha.
    #[default]
    Rgba,
}

impl Channels {
    /// Samples per pixel.
    pub fn count(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::GrayAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    /// `gray` / `y`, `graya` / `ya` / `ga`, `rgb`, `rgba`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "gray" | "grey" | "y" | "g" | "1" => Self::Gray,
            "graya" | "greya" | "gray-alpha" | "grayalpha" | "ya" | "ga" | "2" => Self::GrayAlpha,
            "rgb" | "3" => Self::Rgb,
            "rgba" | "4" => Self::Rgba,
            _ => return None,
        })
    }
}

/// Dither applied before rounding to an integer depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Dither {
    /// Round to nearest.
    #[default]
    None,
    /// 8 × 8 dispersed-dot ordered dither.
    Ordered,
    /// Triangular-density hash noise, one level peak to peak either
    /// side.
    Noise,
}

impl Dither {
    /// `none` / `off`, `ordered` / `bayer`, `noise` / `tpdf` /
    /// `random`. `on` / `true` / `1` select [`Dither::Noise`].
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "false" | "0" | "" => Self::None,
            "ordered" | "bayer" => Self::Ordered,
            "noise" | "tpdf" | "random" | "on" | "true" | "1" => Self::Noise,
            _ => return None,
        })
    }
}

/// How a [`Field`] is turned into samples. Build with
/// [`RenderConfig::new`] and the `with_*` setters; the default is
/// 8-bit RGBA, no dither, serial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenderConfig {
    pub depth: SampleDepth,
    pub channels: Channels,
    pub dither: Dither,
    /// Seed of the [`Dither::Noise`] hash.
    pub dither_seed: u32,
    /// Worker budget; `1` (the default) is serial.
    pub threads: usize,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderConfig {
    /// 8-bit RGBA, no dither, serial.
    pub fn new() -> Self {
        Self {
            depth: SampleDepth::U8,
            channels: Channels::Rgba,
            dither: Dither::None,
            dither_seed: 0,
            threads: 1,
        }
    }

    pub fn with_depth(mut self, depth: SampleDepth) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_channels(mut self, channels: Channels) -> Self {
        self.channels = channels;
        self
    }

    pub fn with_dither(mut self, dither: Dither) -> Self {
        self.dither = dither;
        self
    }

    pub fn with_dither_seed(mut self, seed: u32) -> Self {
        self.dither_seed = seed;
        self
    }

    /// Grant up to `threads` workers (`0` is treated as `1`). The
    /// output does not depend on the budget.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    /// Take the worker budget from a framework execution context.
    pub fn with_execution_context(self, ctx: &oxideav_core::ExecutionContext) -> Self {
        self.with_threads(ctx.threads)
    }

    /// Read `depth=`, `channels=`, `dither=`, `dither_seed=` and
    /// `threads=` from a `generate://` query; absent keys keep the
    /// defaults.
    pub fn from_query(
        query: &std::collections::BTreeMap<String, String>,
    ) -> oxideav_core::Result<Self> {
        use oxideav_core::Error;
        let mut cfg = Self::new();
        if let Some(d) = query.get("depth") {
            cfg.depth = SampleDepth::parse(d)
                .ok_or_else(|| Error::invalid(format!("depth {d:?} is not one of 8|16|float")))?;
        }
        if let Some(c) = query.get("channels") {
            cfg.channels = Channels::parse(c).ok_or_else(|| {
                Error::invalid(format!("channels {c:?} is not one of gray|graya|rgb|rgba"))
            })?;
        }
        if let Some(d) = query.get("dither") {
            cfg.dither = Dither::parse(d).ok_or_else(|| {
                Error::invalid(format!("dither {d:?} is not one of none|ordered|noise"))
            })?;
        }
        cfg.dither_seed = crate::source::q_u32(query, "dither_seed", 0)?;
        cfg.threads = crate::source::q_u32(query, "threads", 1)?.clamp(1, 256) as usize;
        Ok(cfg)
    }
}

/// Interleaved sample storage of an [`Image`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Samples {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

/// A rendered still image: `width × height` pixels, row-major, samples
/// interleaved per [`Channels`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub channels: Channels,
    pub samples: Samples,
}

impl Image {
    /// Sample depth of the storage.
    pub fn depth(&self) -> SampleDepth {
        match self.samples {
            Samples::U8(_) => SampleDepth::U8,
            Samples::U16(_) => SampleDepth::U16,
            Samples::F32(_) => SampleDepth::F32,
        }
    }

    /// The 8-bit samples, when the depth is [`SampleDepth::U8`].
    pub fn as_u8(&self) -> Option<&[u8]> {
        match &self.samples {
            Samples::U8(v) => Some(v),
            _ => None,
        }
    }

    /// The 16-bit samples, when the depth is [`SampleDepth::U16`].
    pub fn as_u16(&self) -> Option<&[u16]> {
        match &self.samples {
            Samples::U16(v) => Some(v),
            _ => None,
        }
    }

    /// The float samples, when the depth is [`SampleDepth::F32`].
    pub fn as_f32(&self) -> Option<&[f32]> {
        match &self.samples {
            Samples::F32(v) => Some(v),
            _ => None,
        }
    }

    /// Stored sample `c` of pixel `(x, y)` as a unit-range float
    /// (`u8 / 255`, `u16 / 65535`, or the float itself).
    pub fn sample(&self, x: u32, y: u32, c: usize) -> f64 {
        let i = ((y as usize) * (self.width as usize) + x as usize) * self.channels.count() + c;
        match &self.samples {
            Samples::U8(v) => f64::from(v[i]) / 255.0,
            Samples::U16(v) => f64::from(v[i]) / 65535.0,
            Samples::F32(v) => f64::from(v[i]),
        }
    }

    /// Pixel `(x, y)` expanded to `[r, g, b, a]` unit-range floats
    /// (grey replicated, a missing alpha reads as 1).
    pub fn pixel(&self, x: u32, y: u32) -> [f64; 4] {
        let s = |c| self.sample(x, y, c);
        match self.channels {
            Channels::Gray => [s(0), s(0), s(0), 1.0],
            Channels::GrayAlpha => [s(0), s(0), s(0), s(1)],
            Channels::Rgb => [s(0), s(1), s(2), 1.0],
            Channels::Rgba => [s(0), s(1), s(2), s(3)],
        }
    }

    /// Convert to 8-bit RGBA (deeper samples are rounded, grey is
    /// replicated, a missing alpha becomes 255).
    pub fn to_rgba8(&self) -> Rgba8Image {
        if let (Channels::Rgba, Samples::U8(v)) = (self.channels, &self.samples) {
            return Rgba8Image {
                width: self.width,
                height: self.height,
                pixels: v.clone(),
            };
        }
        let mut out = Rgba8Image::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                let p = self.pixel(x, y);
                out.put(
                    x,
                    y,
                    [
                        unit_to_u8(p[0]),
                        unit_to_u8(p[1]),
                        unit_to_u8(p[2]),
                        unit_to_u8(p[3]),
                    ],
                );
            }
        }
        out
    }

    /// Consume an 8-bit RGBA image without copying; any other format
    /// goes through [`Image::to_rgba8`].
    pub fn into_rgba8(self) -> Rgba8Image {
        match (self.channels, self.samples) {
            (Channels::Rgba, Samples::U8(pixels)) => Rgba8Image {
                width: self.width,
                height: self.height,
                pixels,
            },
            (channels, samples) => Image {
                width: self.width,
                height: self.height,
                channels,
                samples,
            }
            .to_rgba8(),
        }
    }

    /// The framework pixel format of this image's byte form
    /// ([`Image::to_le_bytes`]): `Gray8` / `Ya8` / `Rgb24` / `Rgba` at
    /// 8 bits, `Gray16Le` / `Ya16Le` / `Rgb48Le` / `Rgba64Le` at 16.
    /// Float images have no framework pixel format (`None`).
    pub fn pixel_format(&self) -> Option<PixelFormat> {
        pixel_format_of(self.depth(), self.channels)
    }

    /// Bytes per row of [`Image::to_le_bytes`].
    pub fn stride(&self) -> usize {
        let bytes = match self.samples {
            Samples::U8(_) => 1,
            Samples::U16(_) => 2,
            Samples::F32(_) => 4,
        };
        self.width as usize * self.channels.count() * bytes
    }

    /// Samples as little-endian bytes, row-major, no padding.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match &self.samples {
            Samples::U8(v) => v.clone(),
            Samples::U16(v) => v.iter().flat_map(|s| s.to_le_bytes()).collect(),
            Samples::F32(v) => v.iter().flat_map(|s| s.to_le_bytes()).collect(),
        }
    }
}

/// Framework pixel format for a depth / layout pair, `None` for float.
pub fn pixel_format_of(depth: SampleDepth, channels: Channels) -> Option<PixelFormat> {
    Some(match (depth, channels) {
        (SampleDepth::U8, Channels::Gray) => PixelFormat::Gray8,
        (SampleDepth::U8, Channels::GrayAlpha) => PixelFormat::Ya8,
        (SampleDepth::U8, Channels::Rgb) => PixelFormat::Rgb24,
        (SampleDepth::U8, Channels::Rgba) => PixelFormat::Rgba,
        (SampleDepth::U16, Channels::Gray) => PixelFormat::Gray16Le,
        (SampleDepth::U16, Channels::GrayAlpha) => PixelFormat::Ya16Le,
        (SampleDepth::U16, Channels::Rgb) => PixelFormat::Rgb48Le,
        (SampleDepth::U16, Channels::Rgba) => PixelFormat::Rgba64Le,
        (SampleDepth::F32, _) => return None,
    })
}

/// A colour defined at every pixel of a `width × height` canvas — the
/// common shape of every still-image generator.
///
/// Implementations must be pure: `fill_row` for a given `(y, x0)` and
/// length always writes the same values, and the value of a pixel does
/// not depend on which other pixels are requested with it.
pub trait Field: Sync {
    /// Canvas width in pixels.
    fn width(&self) -> u32;
    /// Canvas height in pixels.
    fn height(&self) -> u32;

    /// Write the straight-alpha `[r, g, b, a]` colours of pixels
    /// `(x0, y) .. (x0 + out.len(), y)`.
    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]);

    /// `Some(colour)` when every pixel of row `y` has that colour; the
    /// driver then fills the row without calling [`Field::fill_row`].
    fn uniform_row(&self, _y: u32) -> Option<[f64; 4]> {
        None
    }

    /// `Some(colour)` when the whole canvas has that one colour; the
    /// driver then writes the packed pixel straight into fresh storage.
    fn uniform(&self) -> Option<[f64; 4]> {
        None
    }

    /// `Some(y′)`, `y′ < y`, when row `y` is pixel-for-pixel identical
    /// to row `y′` (a pattern periodic in `y`, a colour that depends on
    /// `x` only, a flat canvas). When row `y′` was rendered in the same
    /// band the driver copies it instead of evaluating row `y`.
    fn row_alias(&self, _y: u32) -> Option<u32> {
        None
    }

    /// Optional integer fast path: write the 16-bit quantised colours
    /// `round(65535 · clamp(v, 0, 1))` of the same pixels
    /// [`Field::fill_row`] would produce and return `true`, or return
    /// `false` to let the driver quantise the float row. Used for
    /// undithered 8- and 16-bit output only; the values must equal the
    /// float path's exactly.
    fn fill_row_q16(&self, _y: u32, _x0: u32, _out: &mut [[u16; 4]]) -> bool {
        false
    }
}

/// Render the whole field.
pub fn render(field: &dyn Field, cfg: &RenderConfig) -> Image {
    render_band(field, cfg, 0, field.height())
}

/// Render rows `y0 .. y0 + rows` of the field (clipped to the canvas)
/// as an image `rows` pixels tall. Bands rendered separately and
/// stacked are identical to one whole render, dithering included.
pub fn render_band(field: &dyn Field, cfg: &RenderConfig, y0: u32, rows: u32) -> Image {
    let w = field.width();
    let y0 = y0.min(field.height());
    let rows = rows.min(field.height() - y0);
    let n = w as usize * rows as usize * cfg.channels.count();
    let samples = match cfg.depth {
        SampleDepth::U8 => Samples::U8(samples_of::<u8>(field, cfg, y0, rows, n)),
        SampleDepth::U16 => Samples::U16(samples_of::<u16>(field, cfg, y0, rows, n)),
        SampleDepth::F32 => Samples::F32(samples_of::<f32>(field, cfg, y0, rows, n)),
    };
    Image {
        width: w,
        height: rows,
        channels: cfg.channels,
        samples,
    }
}

/// Allocate and fill the `n` samples of rows `y0 .. y0 + rows`.
fn samples_of<T: Sample>(
    field: &dyn Field,
    cfg: &RenderConfig,
    y0: u32,
    rows: u32,
    n: usize,
) -> Vec<T> {
    let row_len = field.width() as usize * cfg.channels.count();
    // A flat canvas: pack the pixel once, lay out one row, and append
    // it row after row — a single pass over fresh memory. (A dither
    // would vary the pixels, so it takes the general path.)
    if n > 0
        && (cfg.dither == Dither::None || !T::INTEGER)
        && let Some(c) = field.uniform()
    {
        let mut pixel = [T::default(); 4];
        let count = cfg.channels.count();
        pack_row(
            &[c],
            &mut pixel[..count],
            cfg.channels,
            Dither::None,
            0,
            0,
            0,
        );
        let mut row = Vec::with_capacity(row_len);
        for _ in 0..field.width() {
            row.extend_from_slice(&pixel[..count]);
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..rows {
            v.extend_from_slice(&row);
        }
        return v;
    }
    let mut v = vec![T::default(); n];
    drive(field, cfg, y0, rows, &mut v);
    v
}

/// One output sample type.
trait Sample: Copy + Default + Send + Sync + 'static {
    /// `true` for the integer depths (dither applies, q16 path usable).
    const INTEGER: bool;
    /// Quantise a unit-range value (no dither).
    fn from_unit(v: f64) -> Self;
    /// Quantise with a dither offset in output-level units.
    fn from_unit_dithered(v: f64, d: DitherOffset) -> Self;
    /// From a 16-bit quantised value.
    fn from_q16(q: u16) -> Self;
}

/// `round(65535 · clamp(v, 0, 1))`.
#[inline]
pub(crate) fn unit_to_q16(v: f64) -> u16 {
    unit_to_q16_wide(v) as u16
}

#[inline]
fn unit_to_q16_wide(v: f64) -> u32 {
    // NaN clamps to NaN and casts to 0.
    (v.clamp(0.0, 1.0) * 65535.0).round() as u32
}

#[inline]
fn unit_to_u8(v: f64) -> u8 {
    ((unit_to_q16_wide(v) + 128) / 257) as u8
}

impl Sample for u8 {
    const INTEGER: bool = true;
    #[inline]
    fn from_unit(v: f64) -> Self {
        unit_to_u8(v)
    }
    #[inline]
    fn from_unit_dithered(v: f64, d: DitherOffset) -> Self {
        d.apply(v.clamp(0.0, 1.0) * 255.0, 255.0) as u8
    }
    #[inline]
    fn from_q16(q: u16) -> Self {
        ((u32::from(q) + 128) / 257) as u8
    }
}

impl Sample for u16 {
    const INTEGER: bool = true;
    #[inline]
    fn from_unit(v: f64) -> Self {
        unit_to_q16(v)
    }
    #[inline]
    fn from_unit_dithered(v: f64, d: DitherOffset) -> Self {
        d.apply(v.clamp(0.0, 1.0) * 65535.0, 65535.0) as u16
    }
    #[inline]
    fn from_q16(q: u16) -> Self {
        q
    }
}

impl Sample for f32 {
    const INTEGER: bool = false;
    #[inline]
    fn from_unit(v: f64) -> Self {
        v as f32
    }
    #[inline]
    fn from_unit_dithered(v: f64, _d: DitherOffset) -> Self {
        v as f32
    }
    #[inline]
    fn from_q16(q: u16) -> Self {
        f32::from(q) / 65535.0
    }
}

/// Luminance of an encoded colour: the common value of an achromatic
/// colour, else the Rec. 709-weighted sum.
#[inline]
fn luma(p: &[f64; 4]) -> f64 {
    if p[0] == p[1] && p[1] == p[2] {
        p[0]
    } else {
        0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
    }
}

/// The 8 × 8 dispersed-dot index matrix, by three applications of
/// `M₂ₙ = [4Mₙ, 4Mₙ+2; 4Mₙ+3, 4Mₙ+1]` from `M₁ = [0]`.
const fn dispersed_dot_8() -> [[u8; 8]; 8] {
    let mut m = [[0u8; 8]; 8];
    let mut n = 1usize;
    while n < 8 {
        let mut y = 0;
        while y < n {
            let mut x = 0;
            while x < n {
                let v = 4 * m[y][x];
                m[y][x] = v;
                m[y][x + n] = v + 2;
                m[y + n][x] = v + 3;
                m[y + n][x + n] = v + 1;
                x += 1;
            }
            y += 1;
        }
        n *= 2;
    }
    m
}

static DISPERSED_DOT_8: [[u8; 8]; 8] = dispersed_dot_8();

/// Entry `M₈[y mod 8][x mod 8]` (0–63) of the 8 × 8 dispersed-dot
/// index matrix.
#[inline]
pub fn dispersed_dot(x: u32, y: u32) -> u8 {
    DISPERSED_DOT_8[(y & 7) as usize][(x & 7) as usize]
}

/// The dither offsets of one pixel, in output-level units.
#[derive(Clone, Copy)]
struct DitherOffset {
    /// Offset used in the interior of the range.
    wide: f64,
    /// Offset in `(−½, ½)` used within half a level of either end of
    /// the range, where a wider offset would be clipped and shift the
    /// mean; it also leaves exact black and exact white untouched.
    narrow: f64,
}

impl DitherOffset {
    /// Round `x` (in levels, `0..=max`) with the offset applied.
    #[inline]
    fn apply(self, x: f64, max: f64) -> f64 {
        // A value that already sits on a level has no rounding error
        // to spread.
        let nearest = x.round();
        if (x - nearest).abs() < 1e-9 {
            return nearest;
        }
        let d = if x < 0.5 || x > max - 0.5 {
            self.narrow
        } else {
            self.wide
        };
        (x + d).round().clamp(0.0, max)
    }
}

/// Dither offsets for pixel `(x, y)`.
#[inline]
fn dither_offset(dither: Dither, seed: u32, x: u32, y: u32) -> DitherOffset {
    match dither {
        Dither::None => DitherOffset {
            wide: 0.0,
            narrow: 0.0,
        },
        Dither::Ordered => {
            let d =
                (f64::from(DISPERSED_DOT_8[(y & 7) as usize][(x & 7) as usize]) + 0.5) / 64.0 - 0.5;
            DitherOffset { wide: d, narrow: d }
        }
        Dither::Noise => {
            let a = f64::from(crate::video::snow::mix(seed, 0x0D17_4E55, x, y));
            let b = f64::from(crate::video::snow::mix(seed, 0x51ED_270B, x, y));
            DitherOffset {
                wide: (a - b) / 4_294_967_296.0,
                narrow: (a + 0.5) / 4_294_967_296.0 - 0.5,
            }
        }
    }
}

/// Pack one row of colours into interleaved samples.
fn pack_row<T: Sample>(
    row: &[[f64; 4]],
    out: &mut [T],
    channels: Channels,
    dither: Dither,
    seed: u32,
    x0: u32,
    y: u32,
) {
    let n = channels.count();
    if dither == Dither::None || !T::INTEGER {
        match channels {
            Channels::Rgba => {
                for (p, o) in row.iter().zip(out.as_chunks_mut::<4>().0) {
                    o[0] = T::from_unit(p[0]);
                    o[1] = T::from_unit(p[1]);
                    o[2] = T::from_unit(p[2]);
                    o[3] = T::from_unit(p[3]);
                }
            }
            Channels::Rgb => {
                for (p, o) in row.iter().zip(out.as_chunks_mut::<3>().0) {
                    o[0] = T::from_unit(p[0]);
                    o[1] = T::from_unit(p[1]);
                    o[2] = T::from_unit(p[2]);
                }
            }
            Channels::Gray => {
                for (p, o) in row.iter().zip(out.iter_mut()) {
                    *o = T::from_unit(luma(p));
                }
            }
            Channels::GrayAlpha => {
                for (p, o) in row.iter().zip(out.as_chunks_mut::<2>().0) {
                    o[0] = T::from_unit(luma(p));
                    o[1] = T::from_unit(p[3]);
                }
            }
        }
        return;
    }
    for (i, (p, o)) in row.iter().zip(out.chunks_exact_mut(n)).enumerate() {
        let d = dither_offset(dither, seed, x0 + i as u32, y);
        match channels {
            Channels::Rgba => {
                o[0] = T::from_unit_dithered(p[0], d);
                o[1] = T::from_unit_dithered(p[1], d);
                o[2] = T::from_unit_dithered(p[2], d);
                o[3] = T::from_unit_dithered(p[3], d);
            }
            Channels::Rgb => {
                o[0] = T::from_unit_dithered(p[0], d);
                o[1] = T::from_unit_dithered(p[1], d);
                o[2] = T::from_unit_dithered(p[2], d);
            }
            Channels::Gray => o[0] = T::from_unit_dithered(luma(p), d),
            Channels::GrayAlpha => {
                o[0] = T::from_unit_dithered(luma(p), d);
                o[1] = T::from_unit_dithered(p[3], d);
            }
        }
    }
}

/// Pack one row of 16-bit quantised RGBA into interleaved RGB / RGBA
/// samples (the caller never takes this path for the grey layouts).
fn pack_row_q16<T: Sample>(row: &[[u16; 4]], out: &mut [T], channels: Channels) {
    if channels == Channels::Rgba {
        for (p, o) in row.iter().zip(out.as_chunks_mut::<4>().0) {
            *o = p.map(T::from_q16);
        }
    } else {
        debug_assert_eq!(channels, Channels::Rgb);
        for (p, o) in row.iter().zip(out.as_chunks_mut::<3>().0) {
            *o = [T::from_q16(p[0]), T::from_q16(p[1]), T::from_q16(p[2])];
        }
    }
}

/// Fill `out` (rows `y0 .. y0 + rows`) from the field, splitting the
/// rows into bands when a thread budget is granted.
fn drive<T: Sample>(field: &dyn Field, cfg: &RenderConfig, y0: u32, rows: u32, out: &mut [T]) {
    let w = field.width() as usize;
    let row_len = w * cfg.channels.count();
    if row_len == 0 || rows == 0 {
        return;
    }

    let threads = cfg.threads.max(1).min(rows as usize);
    if threads == 1 {
        drive_rows(field, cfg, y0, out);
        return;
    }
    let band = (rows as usize).div_ceil(threads);
    std::thread::scope(|scope| {
        for (i, chunk) in out.chunks_mut(band * row_len).enumerate() {
            let start = y0 + (i * band) as u32;
            scope.spawn(move || drive_rows(field, cfg, start, chunk));
        }
    });
}

/// Serial worker: fill whole rows starting at `y0`.
fn drive_rows<T: Sample>(field: &dyn Field, cfg: &RenderConfig, y0: u32, out: &mut [T]) {
    let w = field.width() as usize;
    let n = cfg.channels.count();
    let row_len = w * n;
    let dithered = T::INTEGER && cfg.dither != Dither::None;
    // The integer fast path cannot reduce a chromatic pixel to grey
    // (that needs the float weights), so it serves RGB layouts only.
    let try_q16 = T::INTEGER && !dithered && matches!(cfg.channels, Channels::Rgb | Channels::Rgba);
    let mut scratch = vec![[0f64; 4]; w];
    let mut scratch_q16 = if try_q16 {
        vec![[0u16; 4]; w]
    } else {
        Vec::new()
    };
    for i in 0..out.len() / row_len {
        let y = y0 + i as u32;
        let (done, rest) = out.split_at_mut(i * row_len);
        let row_out = &mut rest[..row_len];
        if !dithered
            && let Some(src) = field.row_alias(y)
            && src >= y0
            && src < y
        {
            let from = (src - y0) as usize * row_len;
            row_out.copy_from_slice(&done[from..from + row_len]);
            continue;
        }
        if !dithered && let Some(c) = field.uniform_row(y) {
            pack_row(&[c], &mut row_out[..n], cfg.channels, Dither::None, 0, 0, y);
            // Replicate the pixel by doubling the filled prefix.
            let mut filled = n;
            while filled < row_len {
                let take = filled.min(row_len - filled);
                row_out.copy_within(..take, filled);
                filled += take;
            }
            continue;
        }
        if try_q16 && field.fill_row_q16(y, 0, &mut scratch_q16) {
            pack_row_q16(&scratch_q16, row_out, cfg.channels);
            continue;
        }
        if let Some(c) = field.uniform_row(y) {
            scratch.fill(c);
        } else {
            field.fill_row(y, 0, &mut scratch);
        }
        pack_row(
            &scratch,
            row_out,
            cfg.channels,
            cfg.dither,
            cfg.dither_seed,
            0,
            y,
        );
    }
}

/// A single colour everywhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidField {
    pub width: u32,
    pub height: u32,
    /// Straight-alpha sRGB-encoded `[r, g, b, a]`, unit range.
    pub color: [f64; 4],
}

impl Field for SolidField {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn fill_row(&self, _y: u32, _x0: u32, out: &mut [[f64; 4]]) {
        out.fill(self.color);
    }
    fn uniform_row(&self, _y: u32) -> Option<[f64; 4]> {
        Some(self.color)
    }
    fn uniform(&self) -> Option<[f64; 4]> {
        Some(self.color)
    }
    fn row_alias(&self, y: u32) -> Option<u32> {
        y.checked_sub(1)
    }
}

/// `v / 255` for every byte, tabulated.
static U8_UNIT: [f64; 256] = {
    let mut t = [0.0; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = i as f64 / 255.0;
        i += 1;
    }
    t
};

/// An 8-bit level as a unit-range value, `v / 255`.
#[inline]
pub fn u8_to_unit(v: u8) -> f64 {
    U8_UNIT[v as usize]
}

/// An existing 8-bit RGBA raster viewed as a [`Field`], so generators
/// that only produce 8-bit pixels can still be delivered in any depth
/// and layout (their precision stays 8 bits).
pub struct Rgba8Field<'a>(pub &'a Rgba8Image);

impl Field for Rgba8Field<'_> {
    fn width(&self) -> u32 {
        self.0.width
    }
    fn height(&self) -> u32 {
        self.0.height
    }
    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
        let start = ((y as usize) * (self.0.width as usize) + x0 as usize) * 4;
        let src = &self.0.pixels[start..start + out.len() * 4];
        for (o, p) in out.iter_mut().zip(src.as_chunks::<4>().0) {
            *o = p.map(u8_to_unit);
        }
    }
}

/// Re-deliver an 8-bit RGBA raster in another depth / layout.
pub fn convert_rgba8(img: Rgba8Image, cfg: &RenderConfig) -> Image {
    if cfg.depth == SampleDepth::U8 && cfg.channels == Channels::Rgba && cfg.dither == Dither::None
    {
        return Image {
            width: img.width,
            height: img.height,
            channels: Channels::Rgba,
            samples: Samples::U8(img.pixels),
        };
    }
    render(&Rgba8Field(&img), cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `r = x / (w−1)`, `g = y / (h−1)`, `b = 0.25`, `a = 1`.
    struct Ramp(u32, u32);
    impl Field for Ramp {
        fn width(&self) -> u32 {
            self.0
        }
        fn height(&self) -> u32 {
            self.1
        }
        fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
            for (i, o) in out.iter_mut().enumerate() {
                *o = [
                    f64::from(x0 + i as u32) / f64::from(self.0 - 1),
                    f64::from(y) / f64::from(self.1 - 1),
                    0.25,
                    1.0,
                ];
            }
        }
    }

    #[test]
    fn dispersed_dot_matrix_is_a_permutation_with_the_documented_corner() {
        let m = DISPERSED_DOT_8;
        // M₂ = [0 2; 3 1] scaled by 16 at the 4-spaced corners.
        assert_eq!(m[0][0], 0);
        assert_eq!(m[0][4], 2);
        assert_eq!(m[4][0], 3);
        assert_eq!(m[4][4], 1);
        let mut seen = [false; 64];
        for row in m {
            for v in row {
                assert!(!seen[v as usize]);
                seen[v as usize] = true;
            }
        }
    }

    #[test]
    fn eight_bit_is_the_sixteen_bit_output_rounded() {
        let f = Ramp(97, 13);
        let a = render(&f, &RenderConfig::new());
        let b = render(&f, &RenderConfig::new().with_depth(SampleDepth::U16));
        let (a, b) = (a.as_u8().unwrap(), b.as_u16().unwrap());
        for (x, y) in a.iter().zip(b) {
            assert_eq!(u32::from(*x), (u32::from(*y) + 128) / 257);
        }
    }

    #[test]
    fn eight_bit_is_the_float_output_rounded() {
        let f = Ramp(256, 7);
        let a = render(&f, &RenderConfig::new());
        let c = render(&f, &RenderConfig::new().with_depth(SampleDepth::F32));
        for (x, v) in a.as_u8().unwrap().iter().zip(c.as_f32().unwrap()) {
            // x / 255 lands on every level exactly; g and b are away
            // from rounding ties except where f32 rounding is exact.
            let scaled = f64::from(*v) * 255.0;
            if (scaled - scaled.floor() - 0.5).abs() > 1e-4 {
                assert_eq!(f64::from(*x), scaled.round());
            }
        }
    }

    #[test]
    fn layouts_agree_with_rgba() {
        let f = Ramp(9, 5);
        let rgba = render(&f, &RenderConfig::new());
        let rgb = render(&f, &RenderConfig::new().with_channels(Channels::Rgb));
        let ya = render(&f, &RenderConfig::new().with_channels(Channels::GrayAlpha));
        let y = render(&f, &RenderConfig::new().with_channels(Channels::Gray));
        assert_eq!(rgb.pixel_format(), Some(PixelFormat::Rgb24));
        assert_eq!(ya.pixel_format(), Some(PixelFormat::Ya8));
        for py in 0..5 {
            for px in 0..9 {
                let p = rgba.pixel(px, py);
                assert_eq!(rgb.pixel(px, py)[..3], p[..3]);
                assert_eq!(y.pixel(px, py)[0], ya.pixel(px, py)[0]);
                let want =
                    0.2126 * (f64::from(px) / 8.0) + 0.7152 * (f64::from(py) / 4.0) + 0.0722 * 0.25;
                assert!((y.pixel(px, py)[0] - want).abs() <= 0.5 / 255.0 + 1e-12);
            }
        }
    }

    #[test]
    fn thread_budget_does_not_change_the_output() {
        let f = Ramp(31, 23);
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F32] {
            for dither in [Dither::None, Dither::Ordered, Dither::Noise] {
                let cfg = RenderConfig::new().with_depth(depth).with_dither(dither);
                let serial = render(&f, &cfg);
                for t in [2, 3, 7, 64] {
                    assert_eq!(serial, render(&f, &cfg.with_threads(t)));
                }
            }
        }
    }

    #[test]
    fn bands_stack_to_the_whole_image() {
        let f = Ramp(11, 10);
        let cfg = RenderConfig::new().with_dither(Dither::Noise);
        let whole = render(&f, &cfg);
        let mut stacked = Vec::new();
        for (y0, rows) in [(0, 3), (3, 6), (9, 5)] {
            stacked.extend_from_slice(render_band(&f, &cfg, y0, rows).as_u8().unwrap());
        }
        assert_eq!(whole.as_u8().unwrap(), &stacked[..]);
    }

    #[test]
    fn dither_preserves_the_mean_of_a_between_levels_grey() {
        // 100.25 / 255: undithered this is a flat 100; dithered, a
        // quarter of the pixels step up so the mean survives.
        struct Flat;
        impl Field for Flat {
            fn width(&self) -> u32 {
                64
            }
            fn height(&self) -> u32 {
                64
            }
            fn fill_row(&self, _y: u32, _x0: u32, out: &mut [[f64; 4]]) {
                out.fill([100.25 / 255.0, 100.25 / 255.0, 100.25 / 255.0, 1.0]);
            }
        }
        let cfg = RenderConfig::new().with_channels(Channels::Gray);
        let flat = render(&Flat, &cfg);
        assert!(flat.as_u8().unwrap().iter().all(|v| *v == 100));
        for dither in [Dither::Ordered, Dither::Noise] {
            let img = render(&Flat, &cfg.with_dither(dither));
            let v = img.as_u8().unwrap();
            let mean = v.iter().map(|s| f64::from(*s)).sum::<f64>() / v.len() as f64;
            assert!((mean - 100.25).abs() < 0.03, "{dither:?}: mean {mean}");
            assert!(v.iter().all(|s| (99..=101).contains(s)));
        }
    }

    #[test]
    fn rgba8_round_trips_through_every_depth() {
        let mut src = Rgba8Image::new(16, 16);
        for (i, b) in src.pixels.iter_mut().enumerate() {
            *b = (i * 37 % 256) as u8;
        }
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F32] {
            let img = convert_rgba8(src.clone(), &RenderConfig::new().with_depth(depth));
            assert_eq!(img.to_rgba8().pixels, src.pixels, "{depth:?}");
        }
        let deep = convert_rgba8(
            src.clone(),
            &RenderConfig::new().with_depth(SampleDepth::U16),
        );
        for (a, b) in deep.as_u16().unwrap().iter().zip(&src.pixels) {
            assert_eq!(*a, u16::from(*b) * 257);
        }
    }
}

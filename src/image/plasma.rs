//! Plasma clouds — seeded recursive midpoint displacement
//! (diamond-square), in three flavours:
//!
//! - **palette** (the historical default, `generate://plasma` with no
//!   colours): one height field mapped through the shared viridis-ish
//!   palette.
//! - **gradient** (`from=` / `to=`, the ImageMagick `plasma:c1-c2`
//!   pseudo-image): a two-stop linear gradient between the colours —
//!   built by the [`super::gradient`] module and therefore honouring
//!   every `gradient:` define (`direction`, `angle`, `vector`, …) —
//!   with an independent seeded displacement field added to each of
//!   R, G and B. `plasma:red` is red → black; ImageMagick's bare
//!   `plasma:` is `plasma:white-black`.
//! - **fractal** (`type=fractal`, the ImageMagick `plasma:fractal`
//!   pseudo-image): the base is a bilinear blend of four seeded random
//!   corner colours instead of a gradient, with a stronger
//!   displacement field.
//!
//! The ImageMagick page defining plasma's recursion, attenuation and
//! random source is not staged under `docs/`, and the `magick` binary
//! only exposes seeded random output, so there is no pixel-level
//! parity to be had — the same `-seed` produces different pixels in
//! the two implementations. What the gradient / fractal modes match is
//! the *statistical shape* of `magick -seed N plasma:…` output (checked
//! by `tests/magick_parity.rs`): the base gradient's direction and
//! means, a per-channel displacement standard deviation around 25
//! levels that does not grow with the canvas size, independent noise
//! per channel, and smooth neighbour differences (a few levels).
//! Displaced values are clamped to `0..=255` at the end, matching the
//! clamp an HDRI pipeline applies when it exports 8-bit pixels.
//!
//! The displacement field is the diamond-square recursion with a
//! `(−amplitude, +amplitude)` uniform offset at the coarsest level,
//! multiplied by `roughness` per subdivision; `amplitude` and
//! `roughness` are tunable (`amplitude=`, `roughness=`), the defaults
//! reproduce ImageMagick's statistics.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::Rgba8Image;
use super::gradient::{self, GradientOptions};
use super::palette::default_palette;
use crate::source::{q_f64, q_u32};

/// Which plasma flavour to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PlasmaKind {
    /// Height field through the shared default palette (historical
    /// behaviour of `generate://plasma`).
    #[default]
    Palette,
    /// Two-colour gradient base plus per-channel displacement
    /// (`plasma:c1-c2`).
    Gradient,
    /// Four random corner colours plus per-channel displacement
    /// (`plasma:fractal`).
    Fractal,
}

impl PlasmaKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "palette" => Self::Palette,
            "gradient" | "color" | "colour" => Self::Gradient,
            "fractal" => Self::Fractal,
            _ => return None,
        })
    }
}

/// Plasma parameters. Build with [`PlasmaOptions::new`] + `with_*`, or
/// from a query via [`PlasmaOptions::from_query`], then [`render_opts`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PlasmaOptions {
    pub kind: PlasmaKind,
    pub seed: u32,
    /// Palette mode: per-level amplitude decay of the height field
    /// (`0 < roughness ≤ 1`). Gradient / fractal modes: per-level decay
    /// of the displacement field.
    pub roughness: f32,
    /// Gradient / fractal modes: coarsest-level displacement half-range,
    /// in 8-bit levels.
    pub amplitude: f32,
    /// Base gradient (size, colours, and every `gradient:` define).
    /// `from` / `to` are the two plasma colours.
    pub gradient: GradientOptions,
}

/// Default coarsest-level displacement half-range for gradient mode —
/// tuned so a `grey-grey` plasma's per-channel standard deviation
/// lands around 25 levels, the figure `magick -seed N plasma:grey-grey`
/// produces at any canvas size from 64 to 256 pixels.
pub const DEFAULT_AMPLITUDE: f32 = 48.0;
/// Default per-level decay of the displacement field.
pub const DEFAULT_ROUGHNESS: f32 = 0.82;
/// Fractal mode scales [`DEFAULT_AMPLITUDE`] by this (ImageMagick's
/// `plasma:fractal` noise is roughly 1.5× as strong as the colour
/// form's).
pub const FRACTAL_AMPLITUDE_SCALE: f32 = 1.5;

impl PlasmaOptions {
    /// `w × h`, palette mode, seed 42.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            kind: PlasmaKind::Palette,
            seed: 42,
            roughness: DEFAULT_ROUGHNESS,
            amplitude: DEFAULT_AMPLITUDE,
            gradient: GradientOptions::new(width.max(2), height.max(2)),
        }
    }

    pub fn with_kind(mut self, kind: PlasmaKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_seed(mut self, seed: u32) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_roughness(mut self, roughness: f32) -> Self {
        self.roughness = roughness;
        self
    }

    pub fn with_amplitude(mut self, amplitude: f32) -> Self {
        self.amplitude = amplitude;
        self
    }

    /// Two plasma colours; switches to [`PlasmaKind::Gradient`] unless
    /// the kind was set to `Fractal`.
    pub fn with_colors(mut self, from: [u8; 4], to: [u8; 4]) -> Self {
        self.gradient = self.gradient.with_colors(from, to);
        if self.kind == PlasmaKind::Palette {
            self.kind = PlasmaKind::Gradient;
        }
        self
    }

    /// Replace the whole base-gradient description (direction, angle,
    /// vector, … — see [`GradientOptions`]). Its size wins.
    pub fn with_gradient(mut self, gradient: GradientOptions) -> Self {
        self.gradient = gradient;
        self
    }

    /// `generate://plasma?…`: `w`, `h`, `seed`, `roughness`,
    /// `amplitude`, `type=palette|gradient|fractal`, `from`, `to`, and
    /// every `gradient:` define accepted by
    /// [`GradientOptions::from_query`]. Giving `from` or `to` without
    /// a `type` selects gradient mode; `plasma:red` (only `from`) runs
    /// to black like ImageMagick.
    pub fn from_query(query: &BTreeMap<String, String>) -> Result<Self> {
        let w = q_u32(query, "w", 640)?.max(2);
        let h = q_u32(query, "h", 480)?.max(2);
        let has_colors = query.contains_key("from") || query.contains_key("to");
        let kind = match query.get("type") {
            Some(t) => PlasmaKind::parse(t).ok_or_else(|| {
                Error::invalid(format!(
                    "plasma: unknown type {t:?} (expected palette|gradient|fractal)"
                ))
            })?,
            None if has_colors => PlasmaKind::Gradient,
            None => PlasmaKind::Palette,
        };
        let roughness = q_f64(query, "roughness", f64::from(DEFAULT_ROUGHNESS))? as f32;
        if !(roughness > 0.0 && roughness <= 1.0) {
            return Err(Error::invalid(format!(
                "plasma: roughness must be in (0, 1], got {roughness}"
            )));
        }
        let amplitude = q_f64(query, "amplitude", f64::from(DEFAULT_AMPLITUDE))? as f32;
        if amplitude.is_nan() || amplitude < 0.0 {
            return Err(Error::invalid(format!(
                "plasma: amplitude must be ≥ 0, got {amplitude}"
            )));
        }
        // The base gradient shares the query (direction / angle /
        // vector / extent / bbox / colorspace), with plasma's own colour
        // defaults (white → black like ImageMagick's bare `plasma:`).
        let mut gq = query.clone();
        gq.insert("w".into(), w.to_string());
        gq.insert("h".into(), h.to_string());
        gq.remove("type");
        if !gq.contains_key("from") {
            gq.insert("from".into(), "white".into());
        }
        if !gq.contains_key("to") {
            gq.insert("to".into(), "black".into());
        }
        let gradient = GradientOptions::from_query(&gq)?;
        Ok(Self {
            kind,
            seed: q_u32(query, "seed", 42)?,
            roughness,
            amplitude,
            gradient,
        })
    }
}

/// `generate://plasma?…` entry point.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let opts = PlasmaOptions::from_query(query)?;
    // Historical palette mode keeps its original roughness default
    // (0.7) when the caller did not pick one.
    if opts.kind == PlasmaKind::Palette && !query.contains_key("roughness") {
        return render_opts(&opts.with_roughness(0.7));
    }
    render_opts(&opts)
}

/// Render from an explicit option record.
pub fn render_opts(opts: &PlasmaOptions) -> Result<Rgba8Image> {
    let w = opts.gradient.width.max(2);
    let h = opts.gradient.height.max(2);
    match opts.kind {
        PlasmaKind::Palette => {
            let height_field = diamond_square(w as usize, h as usize, opts.seed, opts.roughness);
            let palette = default_palette();
            let mut img = Rgba8Image::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let v = height_field[(y as usize) * (w as usize) + (x as usize)];
                    let idx = ((v * 255.0).clamp(0.0, 255.0)) as usize;
                    img.put(x, y, palette[idx.min(255)]);
                }
            }
            Ok(img)
        }
        PlasmaKind::Gradient => {
            let base = GradientOptions {
                width: w,
                height: h,
                ..opts.gradient.clone()
            };
            let img = gradient::render_linear(&base)?;
            Ok(displace(img, opts.seed, opts.amplitude, opts.roughness))
        }
        PlasmaKind::Fractal => {
            let img = random_corner_base(w, h, opts.seed, opts.gradient.from[3]);
            Ok(displace(
                img,
                opts.seed ^ 0x5A5A_5A5A,
                opts.amplitude * FRACTAL_AMPLITUDE_SCALE,
                opts.roughness,
            ))
        }
    }
}

/// Add one independent signed displacement field per colour channel
/// (alpha is left alone) and clamp.
fn displace(mut img: Rgba8Image, seed: u32, amplitude: f32, roughness: f32) -> Rgba8Image {
    let w = img.width as usize;
    let h = img.height as usize;
    let fields: [Vec<f32>; 3] = [
        displacement_field(w, h, seed.wrapping_add(0x1000_0001), amplitude, roughness),
        displacement_field(w, h, seed.wrapping_add(0x2000_0002), amplitude, roughness),
        displacement_field(w, h, seed.wrapping_add(0x3000_0003), amplitude, roughness),
    ];
    for i in 0..w * h {
        for (c, field) in fields.iter().enumerate() {
            let v = img.pixels[i * 4 + c] as f32 + field[i];
            img.pixels[i * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
    img
}

/// Bilinear blend of four seeded random opaque colours at the corners
/// (`alpha` applied uniformly).
fn random_corner_base(w: u32, h: u32, seed: u32, alpha: u8) -> Rgba8Image {
    let mut rng = Lcg::new(seed ^ 0xC0FF_EE00);
    let mut corner = [[0f32; 3]; 4];
    for c in corner.iter_mut() {
        for ch in c.iter_mut() {
            *ch = rng.next_unit() * 255.0;
        }
    }
    let mut img = Rgba8Image::new(w, h);
    let fx = |x: u32| {
        if w > 1 {
            x as f32 / (w - 1) as f32
        } else {
            0.0
        }
    };
    let fy = |y: u32| {
        if h > 1 {
            y as f32 / (h - 1) as f32
        } else {
            0.0
        }
    };
    for y in 0..h {
        let v = fy(y);
        for x in 0..w {
            let u = fx(x);
            let mut px = [0u8; 4];
            for ch in 0..3 {
                let top = corner[0][ch] + (corner[1][ch] - corner[0][ch]) * u;
                let bottom = corner[2][ch] + (corner[3][ch] - corner[2][ch]) * u;
                px[ch] = (top + (bottom - top) * v).round().clamp(0.0, 255.0) as u8;
            }
            px[3] = alpha;
            img.put(x, y, px);
        }
    }
    img
}

/// Signed diamond-square displacement field: corners at 0, each new
/// point is the neighbour average plus a uniform offset in
/// `(−amp_k, amp_k)` with `amp_k = amplitude · roughness^k` at
/// subdivision level `k`. Not clamped; mean ≈ 0. Row-major `w × h`
/// slice of the covering power-of-two-plus-one grid.
pub fn displacement_field(
    w: usize,
    h: usize,
    seed: u32,
    amplitude: f32,
    roughness: f32,
) -> Vec<f32> {
    let max_dim = w.max(h).max(2);
    let mut size = 2usize;
    while size + 1 < max_dim {
        size *= 2;
    }
    let n = size + 1;
    let mut grid = vec![0f32; n * n];
    let mut rng = Lcg::new(seed);
    let mut step = size;
    let mut amp = amplitude;
    while step > 1 {
        let half = step / 2;
        let mut y = half;
        while y < n {
            let mut x = half;
            while x < n {
                let avg = (grid[(y - half) * n + (x - half)]
                    + grid[(y - half) * n + (x + half)]
                    + grid[(y + half) * n + (x - half)]
                    + grid[(y + half) * n + (x + half)])
                    / 4.0;
                grid[y * n + x] = avg + (rng.next_unit() * 2.0 - 1.0) * amp;
                x += step;
            }
            y += step;
        }
        let mut y = 0usize;
        while y < n {
            let x_off = if (y / half).is_multiple_of(2) {
                half
            } else {
                0
            };
            let mut x = x_off;
            while x < n {
                let mut sum = 0.0f32;
                let mut count = 0.0f32;
                if y >= half {
                    sum += grid[(y - half) * n + x];
                    count += 1.0;
                }
                if y + half < n {
                    sum += grid[(y + half) * n + x];
                    count += 1.0;
                }
                if x >= half {
                    sum += grid[y * n + (x - half)];
                    count += 1.0;
                }
                if x + half < n {
                    sum += grid[y * n + (x + half)];
                    count += 1.0;
                }
                grid[y * n + x] = sum / count + (rng.next_unit() * 2.0 - 1.0) * amp;
                x += step;
            }
            y += half;
        }
        step = half;
        amp *= roughness;
    }
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            out.push(grid[y * n + x]);
        }
    }
    out
}

/// Diamond-square algorithm. Allocates a power-of-two-plus-one square
/// big enough to cover `(w, h)`, fills it via midpoint displacement,
/// and slices the requested rectangle out of the corner.
///
/// Returned values are normalised to `[0, 1]` row-major.
fn diamond_square(w: usize, h: usize, seed: u32, roughness: f32) -> Vec<f32> {
    let max_dim = w.max(h);
    let mut size = 2usize;
    while size + 1 < max_dim {
        size *= 2;
    }
    let n = size + 1;
    let mut grid = vec![0f32; n * n];
    let mut rng = Lcg::new(seed);

    // Seed the four corners.
    grid[0] = rng.next_f32();
    grid[size] = rng.next_f32();
    grid[size * n] = rng.next_f32();
    grid[size * n + size] = rng.next_f32();

    let mut step = size;
    let mut amp = 1.0f32;
    while step > 1 {
        let half = step / 2;
        // Diamond step
        let mut y = half;
        while y < n {
            let mut x = half;
            while x < n {
                let avg = (grid[(y - half) * n + (x - half)]
                    + grid[(y - half) * n + (x + half)]
                    + grid[(y + half) * n + (x - half)]
                    + grid[(y + half) * n + (x + half)])
                    / 4.0;
                grid[y * n + x] = (avg + (rng.next_f32() - 0.5) * amp).clamp(0.0, 1.0);
                x += step;
            }
            y += step;
        }
        // Square step
        let mut y = 0usize;
        while y < n {
            let x_off = if (y / half).is_multiple_of(2) {
                half
            } else {
                0
            };
            let mut x = x_off;
            while x < n {
                let mut sum = 0.0f32;
                let mut count = 0.0f32;
                if y >= half {
                    sum += grid[(y - half) * n + x];
                    count += 1.0;
                }
                if y + half < n {
                    sum += grid[(y + half) * n + x];
                    count += 1.0;
                }
                if x >= half {
                    sum += grid[y * n + (x - half)];
                    count += 1.0;
                }
                if x + half < n {
                    sum += grid[y * n + (x + half)];
                    count += 1.0;
                }
                let avg = sum / count;
                grid[y * n + x] = (avg + (rng.next_f32() - 0.5) * amp).clamp(0.0, 1.0);
                x += step;
            }
            y += half;
        }
        step = half;
        amp *= roughness;
    }

    // Slice the requested rectangle out of the top-left corner.
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            out.push(grid[y * n + x]);
        }
    }
    out
}

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u32) -> Self {
        Self {
            state: (seed as u64)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0xCAFE_F00D_DEAD_BEEF),
        }
    }
    fn step(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
    /// Historical draw used by the palette height field: bits 33..63
    /// over `u32::MAX`, so the range is `[0, 0.5]`. Kept as-is so
    /// `generate://plasma` (and `scroll?pattern=plasma`) stay
    /// byte-identical across releases.
    fn next_f32(&mut self) -> f32 {
        ((self.step() >> 33) as u32 as f32) / (u32::MAX as f32)
    }
    /// Uniform in `[0, 1)` from the top 32 bits — used by the
    /// displacement field and the fractal corner colours.
    fn next_unit(&mut self) -> f32 {
        ((self.step() >> 32) as f64 / 4_294_967_296.0) as f32
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

    /// (mean, standard deviation) of one channel.
    fn stats(img: &Rgba8Image, ch: usize) -> (f64, f64) {
        let n = (img.width * img.height) as f64;
        let vals = img.pixels.as_chunks::<4>().0.iter().map(|p| p[ch] as f64);
        let mean = vals.clone().sum::<f64>() / n;
        let var = vals.map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
        (mean, var.sqrt())
    }

    fn band_mean(img: &Rgba8Image, ch: usize, x0: u32, y0: u32, w: u32, h: u32) -> f64 {
        let mut sum = 0.0;
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                sum += img.get(x, y)[ch] as f64;
            }
        }
        sum / (w * h) as f64
    }

    #[test]
    fn plasma_default_seed_deterministic() {
        let a = render(&map(&[("w", "32"), ("h", "32"), ("seed", "7")])).unwrap();
        let b = render(&map(&[("w", "32"), ("h", "32"), ("seed", "7")])).unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn plasma_different_seeds_differ() {
        let a = render(&map(&[("w", "32"), ("h", "32"), ("seed", "1")])).unwrap();
        let b = render(&map(&[("w", "32"), ("h", "32"), ("seed", "999")])).unwrap();
        assert_ne!(a.pixels, b.pixels);
    }

    #[test]
    fn palette_mode_is_unchanged_by_the_option_record() {
        // The historical query path (palette through the viridis map)
        // must be byte-identical to the explicit record.
        let q = render(&map(&[("w", "40"), ("h", "24"), ("seed", "3")])).unwrap();
        let o = render_opts(&PlasmaOptions::new(40, 24).with_seed(3).with_roughness(0.7)).unwrap();
        assert_eq!(q.pixels, o.pixels);
        // Every palette pixel is a palette entry.
        let pal = default_palette();
        for p in q.pixels.as_chunks::<4>().0.iter() {
            assert!(pal.contains(&[p[0], p[1], p[2], p[3]]));
        }
    }

    #[test]
    fn palette_mode_bytes_are_pinned_across_releases() {
        // FNV-1a of the RGBA bytes, recorded from the 0.1.4 code path.
        let fnv = |img: &Rgba8Image| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &b in &img.pixels {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h
        };
        let cases: [(&[(&str, &str)], u64); 3] = [
            (
                &[("w", "37"), ("h", "23"), ("seed", "7")],
                0xe7f1_2ec5_cf30_cfa2,
            ),
            (&[("w", "64"), ("h", "64")], 0xb621_4a23_78bb_bff2),
            (
                &[
                    ("w", "16"),
                    ("h", "40"),
                    ("seed", "3"),
                    ("roughness", "0.5"),
                ],
                0xe785_e90c_c58c_9610,
            ),
        ];
        for (q, want) in cases {
            assert_eq!(fnv(&render(&map(q)).unwrap()), want, "{q:?}");
        }
    }

    #[test]
    fn colour_mode_keeps_the_base_gradient_direction() {
        // red → blue, South: the top band is red-dominant, the bottom
        // band blue-dominant, and green stays near zero.
        let img = render(&map(&[
            ("from", "red"),
            ("to", "blue"),
            ("w", "64"),
            ("h", "64"),
            ("seed", "1"),
        ]))
        .unwrap();
        let top_r = band_mean(&img, 0, 0, 0, 64, 8);
        let bot_r = band_mean(&img, 0, 0, 56, 64, 8);
        let top_b = band_mean(&img, 2, 0, 0, 64, 8);
        let bot_b = band_mean(&img, 2, 0, 56, 64, 8);
        assert!(top_r > 200.0 && bot_r < 60.0, "red {top_r} → {bot_r}");
        assert!(top_b < 60.0 && bot_b > 200.0, "blue {top_b} → {bot_b}");
        let (g_mean, _) = stats(&img, 1);
        assert!(g_mean < 25.0, "green mean {g_mean}");
        // Alpha untouched.
        assert!(img.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // `direction=East` turns it sideways.
        let img = render(&map(&[
            ("from", "red"),
            ("to", "blue"),
            ("direction", "East"),
            ("w", "64"),
            ("h", "64"),
            ("seed", "1"),
        ]))
        .unwrap();
        assert!(band_mean(&img, 0, 0, 0, 8, 64) > 200.0);
        assert!(band_mean(&img, 0, 56, 0, 8, 64) < 60.0);
    }

    #[test]
    fn colour_mode_noise_statistics_are_scale_invariant() {
        // grey-grey: flat base (gray50 = 127) + displacement; the
        // per-channel sd sits in the mid-20s at 64 and 256 px, like
        // `magick -seed N plasma:grey-grey`, channels differ, and the
        // mean stays near the base.
        for size in [64u32, 256] {
            let img = render(&map(&[
                ("from", "gray50"),
                ("to", "gray50"),
                ("w", &size.to_string()),
                ("h", &size.to_string()),
                ("seed", "1"),
            ]))
            .unwrap();
            for ch in 0..3 {
                let (mean, sd) = stats(&img, ch);
                assert!((mean - 127.0).abs() < 20.0, "{size}: ch{ch} mean {mean}");
                assert!((15.0..=40.0).contains(&sd), "{size}: ch{ch} sd {sd}");
            }
            // Channels carry independent fields.
            let r: Vec<u8> = img.pixels.as_chunks::<4>().0.iter().map(|p| p[0]).collect();
            let g: Vec<u8> = img.pixels.as_chunks::<4>().0.iter().map(|p| p[1]).collect();
            assert_ne!(r, g);
        }
    }

    #[test]
    fn fractal_mode_is_random_colour_not_gradient() {
        let a = render(&map(&[
            ("type", "fractal"),
            ("w", "64"),
            ("h", "64"),
            ("seed", "1"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "fractal"),
            ("w", "64"),
            ("h", "64"),
            ("seed", "2"),
        ]))
        .unwrap();
        assert_ne!(a.pixels, b.pixels);
        // Not greyscale, not a palette image: channels have distinct means.
        let means: Vec<f64> = (0..3).map(|c| stats(&a, c).0).collect();
        assert!(
            means.iter().any(|m| (m - means[0]).abs() > 5.0),
            "{means:?}"
        );
        // Spread is wider than the colour form's.
        let sd: f64 = (0..3).map(|c| stats(&a, c).1).sum::<f64>() / 3.0;
        assert!(sd > 20.0, "fractal sd {sd}");
        assert!(a.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn only_from_runs_to_black() {
        let img = render(&map(&[
            ("from", "red"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "5"),
        ]))
        .unwrap();
        assert!(band_mean(&img, 0, 0, 0, 32, 4) > 180.0);
        assert!(band_mean(&img, 0, 0, 28, 32, 4) < 70.0);
        // Blue is only the clamped positive half of the noise.
        assert!(stats(&img, 2).0 < 40.0);
    }

    #[test]
    fn options_record_roundtrip_and_errors() {
        let o = PlasmaOptions::from_query(&map(&[
            ("type", "fractal"),
            ("seed", "9"),
            ("roughness", "0.5"),
            ("amplitude", "10"),
            ("w", "8"),
            ("h", "8"),
        ]))
        .unwrap();
        assert_eq!(o.kind, PlasmaKind::Fractal);
        assert_eq!(o.seed, 9);
        assert_eq!(o.roughness, 0.5);
        assert_eq!(o.amplitude, 10.0);
        assert_eq!(o.gradient.width, 8);
        assert!(render(&map(&[("type", "lava")])).is_err());
        assert!(render(&map(&[("roughness", "0")])).is_err());
        assert!(render(&map(&[("amplitude", "-1")])).is_err());
        assert!(render(&map(&[("from", "notacolour")])).is_err());
        // Builder: colours flip palette → gradient.
        let o = PlasmaOptions::new(8, 8).with_colors([255, 0, 0, 255], [0, 0, 255, 255]);
        assert_eq!(o.kind, PlasmaKind::Gradient);
        let o = PlasmaOptions::new(8, 8)
            .with_kind(PlasmaKind::Fractal)
            .with_colors([255, 0, 0, 255], [0, 0, 255, 255]);
        assert_eq!(o.kind, PlasmaKind::Fractal);
    }

    #[test]
    fn displacement_field_is_zero_mean_and_zero_at_corners() {
        let f = displacement_field(65, 65, 11, 50.0, 0.6);
        assert_eq!(f[0], 0.0);
        assert_eq!(f[64], 0.0);
        assert_eq!(f[64 * 65], 0.0);
        assert_eq!(f[64 * 65 + 64], 0.0);
        let mean = f.iter().map(|&v| v as f64).sum::<f64>() / f.len() as f64;
        assert!(mean.abs() < 15.0, "mean {mean}");
        assert!(f.iter().any(|&v| v > 5.0) && f.iter().any(|&v| v < -5.0));
        // Zero amplitude → identically zero.
        assert!(
            displacement_field(16, 16, 1, 0.0, 0.6)
                .iter()
                .all(|&v| v == 0.0)
        );
    }
}

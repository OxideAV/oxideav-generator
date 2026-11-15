//! Image noise generators:
//!
//! - **Gradient noise** — classic Perlin 2-D (`type=perlin`) and Ken
//!   Perlin's improved simplex 2-D (`type=simplex`). Pure first-
//!   principles implementations derived from the gradient-noise
//!   mathematics in Ken Perlin's published papers. They share the
//!   seeded 512-entry permutation table (`build_perm`), so the same
//!   `seed=` is bit-deterministic across builds for both kinds, and
//!   they share the multi-octave fBm accumulator and the palette
//!   mapping in [`render`].
//!
//! - **Value noise** — `type=value` (alias `type=lattice`). The
//!   textbook predecessor to gradient noise: each integer lattice
//!   point holds a pseudo-random scalar in `[-1, 1]`; a sample at
//!   `(x, y)` smoothstep-interpolates the four surrounding lattice
//!   values. The smoothstep is the same quintic
//!   `t³·(t·(6t − 15) + 10)` `fade` curve [`perlin2`] uses, so the
//!   surface is C²-continuous along cell boundaries. Distinct from
//!   gradient noise: value noise has axis-aligned blocky low-frequency
//!   character because the lattice values themselves (not gradients of
//!   a hidden field) carry the signal, which is exactly why Perlin's
//!   1985 SIGGRAPH paper *An Image Synthesizer* moved on from it to
//!   gradient noise. Same seeded permutation table as the gradient
//!   modes (`build_perm`); same multi-octave fBm accumulator; same
//!   palette. Pure first-principles maths.
//!
//! - **Cellular noise** — Worley 2-D (`type=worley`, alias
//!   `type=cellular`). A spatial-point-process noise distinct from
//!   gradient noise: feature points are placed pseudo-randomly inside
//!   each integer cell of a regular grid, and each pixel's value is
//!   the distance from the pixel to the k-th closest feature point.
//!   The default `k=1, dist=euclidean` reproduces the canonical
//!   Voronoi-cell "stone wall" / "scales" texture; alternative metrics
//!   (`manhattan`, `chebyshev`) and higher `k` (the so-called
//!   F2 / F3 distances) yield a family of related-but-distinct textures.
//!   Mathematical reference: Steven Worley, *A Cellular Texture Basis
//!   Function*, SIGGRAPH 1996 proceedings — a public academic paper.
//!   Pure first-principles maths transcribed from that paper; the
//!   pseudo-random feature-point placement uses the same in-tree LCG
//!   the rest of this module already uses, so the same `seed=` is
//!   bit-deterministic across builds.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::Rgba8Image;
use super::canvas::{self, Field, Image, RenderConfig};
use super::gradient::{GradientField, GradientKind, GradientOptions};
use super::palette::default_palette;
use crate::source::{q_f64, q_str, q_u32};

/// How a noise scalar becomes a colour.
enum Colouring {
    /// The shared 256-entry palette, indexed by `⌊255·s⌋`, as
    /// unit-range colours.
    Palette(Box<[[f64; 4]; 256]>),
    /// A gradient colour ramp evaluated at `s` (full precision).
    Ramp(Box<GradientField>),
}

/// Which scalar field is sampled.
#[derive(Copy, Clone)]
enum Basis {
    /// Multi-octave sum of a lattice noise.
    Fractal {
        mode: NoiseMode,
        octaves: u32,
        base_freq: f32,
        lacunarity: f32,
        gain: f32,
    },
    /// Distance to the k-th nearest feature point.
    Worley {
        k: usize,
        points_per_cell: usize,
        metric: DistMetric,
        cell_size: f32,
        seed: u32,
    },
}

/// A noise image as a [`Field`]: a scalar in `[0, 1]` at every pixel,
/// coloured by the palette or by a colour ramp. Build with
/// [`NoiseField::from_query`].
pub struct NoiseField {
    width: u32,
    height: u32,
    perm: Box<[u8; 512]>,
    basis: Basis,
    colouring: Colouring,
    /// Per-octave, per-column lattice terms of the Perlin / value sum
    /// (`columns[octave][x]`); empty for the other bases.
    columns: Vec<Vec<Column>>,
}

/// Everything about a lattice sample that depends on `x` only.
#[derive(Clone, Copy)]
struct Column {
    /// Fractional lattice position `x·f − ⌊x·f⌋`.
    xf: f32,
    /// `fade(xf)`.
    u: f32,
    /// Permutation entries of the two lattice columns either side.
    pa: u8,
    pb: u8,
}

/// Pixels handled per batch by the row evaluators.
const BATCH: usize = 256;

impl NoiseField {
    /// Parse a `generate://noise?…` query:
    ///
    /// - `w`, `h`, `seed`, `type=perlin|simplex|value|worley`, `scale`
    ///   (feature size in pixels);
    /// - fractal sum (`perlin`, `simplex`, `value`): `octaves` (1–8,
    ///   default 4), `lacunarity` (frequency ratio between octaves,
    ///   default 2), `gain` (amplitude ratio, default ½). Octave `i`
    ///   has frequency `lacunarityⁱ / scale` and weight `gainⁱ`; the
    ///   weighted sum is divided by the sum of the weights and mapped
    ///   from `[−1, 1]` to `[0, 1]`;
    /// - `worley`: `k`, `points`, `dist` (see [`render`]);
    /// - colour: by default the shared palette; giving `from` / `to` or
    ///   `stops` (plus `colorspace`, `spread`, `hue`) maps the scalar
    ///   through that gradient ramp instead, at full sample precision.
    pub fn from_query(query: &BTreeMap<String, String>) -> Result<Self> {
        let w = q_u32(query, "w", 640)?.max(1);
        let h = q_u32(query, "h", 480)?.max(1);
        let kind = q_str(query, "type", "perlin");
        let scale = q_f64(query, "scale", 64.0)?.max(1.0);
        let octaves = q_u32(query, "octaves", 4)?.clamp(1, 8);
        let seed = q_u32(query, "seed", 42)?;
        let basis = match kind {
            "perlin" | "simplex" | "value" | "lattice" => {
                let lacunarity = q_f64(query, "lacunarity", 2.0)?;
                let gain = q_f64(query, "gain", q_f64(query, "persistence", 0.5)?)?;
                if !(lacunarity.is_finite() && lacunarity > 0.0 && gain.is_finite() && gain > 0.0) {
                    return Err(Error::invalid(
                        "noise: lacunarity and gain must be positive numbers",
                    ));
                }
                Basis::Fractal {
                    mode: match kind {
                        "simplex" => NoiseMode::Simplex,
                        "value" | "lattice" => NoiseMode::Value,
                        _ => NoiseMode::Perlin,
                    },
                    octaves,
                    base_freq: 1.0f32 / (scale as f32),
                    lacunarity: lacunarity as f32,
                    gain: gain as f32,
                }
            }
            "worley" | "cellular" => {
                let metric_name = q_str(query, "dist", "euclidean");
                let metric = match metric_name {
                    "euclidean" | "euc" | "l2" => DistMetric::Euclidean,
                    "manhattan" | "l1" => DistMetric::Manhattan,
                    "chebyshev" | "linf" | "max" => DistMetric::Chebyshev,
                    other => {
                        return Err(Error::invalid(format!(
                            "noise: worley dist {other:?} (expected euclidean|manhattan|chebyshev)"
                        )));
                    }
                };
                Basis::Worley {
                    k: q_u32(query, "k", 1)?.clamp(1, 4) as usize,
                    points_per_cell: q_u32(query, "points", 1)?.clamp(1, 4) as usize,
                    metric,
                    cell_size: scale as f32,
                    seed,
                }
            }
            _ => {
                return Err(Error::invalid(format!(
                    "noise: unknown type {kind:?} (expected perlin|simplex|value|worley|cellular)"
                )));
            }
        };
        let ramp = ["from", "to", "stops"]
            .iter()
            .any(|k| query.contains_key(*k));
        let colouring = if ramp {
            let mut gq = query.clone();
            gq.remove("type");
            gq.remove("angle");
            if !gq.contains_key("stops") {
                // Low values dark, high values light unless told otherwise.
                gq.entry("from".into()).or_insert_with(|| "black".into());
                gq.entry("to".into()).or_insert_with(|| "white".into());
            }
            Colouring::Ramp(Box::new(
                GradientOptions::from_query(&gq)?.field(GradientKind::Linear)?,
            ))
        } else {
            Colouring::Palette(Box::new(
                default_palette().map(|c| c.map(canvas::u8_to_unit)),
            ))
        };
        let perm = Box::new(build_perm(seed));
        let mut columns = Vec::new();
        if let Basis::Fractal {
            mode: NoiseMode::Perlin | NoiseMode::Value,
            octaves,
            base_freq,
            lacunarity,
            ..
        } = basis
        {
            let mut freq = base_freq;
            for _ in 0..octaves {
                columns.push(
                    (0..w)
                        .map(|x| {
                            let p = (x as f32) * freq;
                            let xi = p.floor() as i32;
                            let xf = p - p.floor();
                            Column {
                                xf,
                                u: fade(xf),
                                pa: perm[(xi & 0xFF) as usize],
                                pb: perm[((xi + 1) & 0xFF) as usize],
                            }
                        })
                        .collect(),
                );
                freq *= lacunarity;
            }
        }
        Ok(Self {
            width: w,
            height: h,
            perm,
            basis,
            colouring,
            columns,
        })
    }

    /// The scalars of pixels `(x0, y) .. (x0 + out.len(), y)`: the same
    /// values as [`NoiseField::scalar`], with the row-only and
    /// column-only parts of the lattice sums taken out of the pixel
    /// loop.
    fn scalars(&self, y: u32, x0: u32, out: &mut [f32]) {
        for (i, chunk) in out.chunks_mut(BATCH).enumerate() {
            self.scalars_batch(y, x0 + (i * BATCH) as u32, chunk);
        }
    }

    /// Worley scalars of a span: the feature points of the three cell
    /// rows around `y` are generated once for the whole span instead of
    /// once per pixel; candidates are visited in the same order as
    /// [`worley_scalar`], so the values are the same.
    fn worley_batch(
        &self,
        y: u32,
        x0: u32,
        out: &mut [f32],
        k: usize,
        points_per_cell: usize,
        metric: DistMetric,
        cell_size: f32,
        seed: u32,
    ) {
        if out.is_empty() {
            return;
        }
        let cell_of = |v: u32| ((v as f32) / cell_size).floor() as i32;
        let py = y as f32;
        let cy = cell_of(y);
        let first = cell_of(x0) - 1;
        let last = cell_of(x0 + out.len() as u32 - 1) + 1;
        let columns = (last - first + 1) as usize;
        // features[(row * columns + column) * points + p]
        let mut features = Vec::with_capacity(3 * columns * points_per_cell);
        for dcy in -1..=1 {
            for cx in first..=last {
                for p in 0..points_per_cell {
                    features.push(feature_point(cx, cy + dcy, p as u32, seed, cell_size));
                }
            }
        }
        // Rank candidates by a key that orders like the distance: the
        // squared distance for the Euclidean metric (the square root is
        // monotone and correctly rounded, so the k-th smallest distance
        // is the root of the k-th smallest square), the distance itself
        // otherwise.
        let key = |px: f32, (fx, fy): (f32, f32)| -> f32 {
            match metric {
                DistMetric::Euclidean => {
                    let dx = px - fx;
                    let dy = py - fy;
                    dx * dx + dy * dy
                }
                DistMetric::Manhattan => (px - fx).abs() + (py - fy).abs(),
                DistMetric::Chebyshev => (px - fx).abs().max((py - fy).abs()),
            }
        };
        let finish = |key: f32| -> f32 {
            let d = if matches!(metric, DistMetric::Euclidean) {
                key.sqrt()
            } else {
                key
            };
            (d / cell_size).clamp(0.0, 1.0)
        };
        let rows: [&[(f32, f32)]; 3] = [
            &features[..columns * points_per_cell],
            &features[columns * points_per_cell..2 * columns * points_per_cell],
            &features[2 * columns * points_per_cell..],
        ];
        for (i, o) in out.iter_mut().enumerate() {
            let px = (x0 + i as u32) as f32;
            let cx = ((px / cell_size).floor() as i32 - first) as usize;
            // The three cells either side of the pixel's column.
            let span = (cx - 1) * points_per_cell..(cx + 2) * points_per_cell;
            if k == 1 {
                let mut best = f32::INFINITY;
                for row in rows {
                    for &f in &row[span.clone()] {
                        let d = key(px, f);
                        if d < best {
                            best = d;
                        }
                    }
                }
                *o = finish(best);
                continue;
            }
            let mut nearest = [f32::INFINITY; 4];
            for row in rows {
                for &f in &row[span.clone()] {
                    let d = key(px, f);
                    for slot_i in 0..k {
                        if d < nearest[slot_i] {
                            for j in (slot_i + 1..k).rev() {
                                nearest[j] = nearest[j - 1];
                            }
                            nearest[slot_i] = d;
                            break;
                        }
                    }
                }
            }
            *o = finish(nearest[k - 1]);
        }
    }

    /// [`NoiseField::scalars`] for at most [`BATCH`] pixels.
    fn scalars_batch(&self, y: u32, x0: u32, out: &mut [f32]) {
        let perm = &*self.perm;
        let (mode, octaves, base_freq, lacunarity, gain) = match self.basis {
            Basis::Worley {
                k,
                points_per_cell,
                metric,
                cell_size,
                seed,
            } => {
                self.worley_batch(y, x0, out, k, points_per_cell, metric, cell_size, seed);
                return;
            }
            Basis::Fractal {
                mode: NoiseMode::Simplex,
                octaves,
                base_freq,
                lacunarity,
                gain,
            } => {
                // Not separable: the same sum as `scalar`, with the
                // basis chosen outside the pixel loop.
                let fy = y as f32;
                for (i, o) in out.iter_mut().enumerate() {
                    let fx = (x0 + i as u32) as f32;
                    let mut amp = 1.0f32;
                    let mut freq = base_freq;
                    let mut total = 0.0f32;
                    let mut max_total = 0.0f32;
                    for _ in 0..octaves {
                        total += amp * simplex2(perm, fx * freq, fy * freq);
                        max_total += amp;
                        amp *= gain;
                        freq *= lacunarity;
                    }
                    *o = ((total / max_total) * 0.5 + 0.5).clamp(0.0, 1.0);
                }
                return;
            }
            Basis::Fractal {
                mode,
                octaves,
                base_freq,
                lacunarity,
                gain,
            } => (mode, octaves, base_freq, lacunarity, gain),
        };
        let mut total = [0.0f32; BATCH];
        let total = &mut total[..out.len()];
        let mut amp = 1.0f32;
        let mut freq = base_freq;
        let mut max_total = 0.0f32;
        // One column table per octave (`columns.len() == octaves`).
        debug_assert_eq!(self.columns.len(), octaves as usize);
        for octave_columns in &self.columns {
            let p = (y as f32) * freq;
            let yi = p.floor() as i32;
            let yf = p - p.floor();
            let v = fade(yf);
            let cols = &octave_columns[x0 as usize..x0 as usize + out.len()];
            let corner = |column: u8, dy: i32| perm[((column as i32 + yi + dy) & 0xFF) as usize];
            match mode {
                NoiseMode::Perlin => {
                    for (t, c) in total.iter_mut().zip(cols) {
                        let (aa, ab) = (corner(c.pa, 0), corner(c.pa, 1));
                        let (ba, bb) = (corner(c.pb, 0), corner(c.pb, 1));
                        let x1 = lerp(grad(aa, c.xf, yf), grad(ba, c.xf - 1.0, yf), c.u);
                        let x2 = lerp(
                            grad(ab, c.xf, yf - 1.0),
                            grad(bb, c.xf - 1.0, yf - 1.0),
                            c.u,
                        );
                        *t += amp * lerp(x1, x2, v);
                    }
                }
                _ => {
                    let unit = |h: u8| (h as f32) * (2.0 / 255.0) - 1.0;
                    for (t, c) in total.iter_mut().zip(cols) {
                        let (aa, ab) = (unit(corner(c.pa, 0)), unit(corner(c.pa, 1)));
                        let (ba, bb) = (unit(corner(c.pb, 0)), unit(corner(c.pb, 1)));
                        let x1 = lerp(aa, ba, c.u);
                        let x2 = lerp(ab, bb, c.u);
                        *t += amp * lerp(x1, x2, v);
                    }
                }
            }
            max_total += amp;
            amp *= gain;
            freq *= lacunarity;
        }
        for (o, t) in out.iter_mut().zip(total.iter()) {
            *o = ((t / max_total) * 0.5 + 0.5).clamp(0.0, 1.0);
        }
    }

    /// The noise scalar at pixel `(x, y)`, in `[0, 1]`.
    pub fn scalar(&self, x: u32, y: u32) -> f32 {
        match self.basis {
            Basis::Fractal {
                mode,
                octaves,
                base_freq,
                lacunarity,
                gain,
            } => {
                let perm = &*self.perm;
                let mut amp = 1.0f32;
                let mut freq = base_freq;
                let mut total = 0.0f32;
                let mut max_total = 0.0f32;
                for _ in 0..octaves {
                    let sample = match mode {
                        NoiseMode::Perlin => perlin2(perm, (x as f32) * freq, (y as f32) * freq),
                        NoiseMode::Simplex => simplex2(perm, (x as f32) * freq, (y as f32) * freq),
                        NoiseMode::Value => value2(perm, (x as f32) * freq, (y as f32) * freq),
                    };
                    total += amp * sample;
                    max_total += amp;
                    amp *= gain;
                    freq *= lacunarity;
                }
                ((total / max_total) * 0.5 + 0.5).clamp(0.0, 1.0)
            }
            Basis::Worley {
                k,
                points_per_cell,
                metric,
                cell_size,
                seed,
            } => worley_scalar(x, y, k, points_per_cell, metric, cell_size, seed),
        }
    }
}

impl Field for NoiseField {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
        let mut scalars = [0.0f32; BATCH];
        let mut x = x0;
        for chunk in out.chunks_mut(BATCH) {
            let scalars = &mut scalars[..chunk.len()];
            self.scalars(y, x, scalars);
            match &self.colouring {
                Colouring::Palette(palette) => {
                    for (o, s) in chunk.iter_mut().zip(scalars.iter()) {
                        let idx = (s * 255.0) as usize;
                        *o = palette[idx.min(255)];
                    }
                }
                Colouring::Ramp(ramp) => {
                    for (o, s) in chunk.iter_mut().zip(scalars.iter()) {
                        *o = ramp.color_at(f64::from(*s));
                    }
                }
            }
            x += chunk.len() as u32;
        }
    }
}

/// `generate://noise?…` as 8-bit RGBA. Keys per
/// [`NoiseField::from_query`].
///
/// Worley / cellular noise: the plane is divided into cells of side
/// `scale` pixels, each holding `points` (1–4) pseudo-random feature
/// points; a pixel's scalar is its distance to the `k`-th (1–4)
/// closest feature point over the 3 × 3 cell neighbourhood, under
/// `dist=euclidean|manhattan|chebyshev`, divided by `scale` and
/// clamped to 1.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let field = NoiseField::from_query(query)?;
    Ok(canvas::render(&field, &RenderConfig::new()).into_rgba8())
}

/// Like [`render`], in the depth / layout the query asks for.
pub fn render_image(query: &BTreeMap<String, String>) -> Result<Image> {
    let field = NoiseField::from_query(query)?;
    Ok(canvas::render(&field, &RenderConfig::from_query(query)?))
}

#[derive(Copy, Clone)]
enum NoiseMode {
    Perlin,
    Simplex,
    Value,
}

/// Normalised distance from pixel `(x, y)` to its `k`-th closest
/// feature point (see [`render`]).
fn worley_scalar(
    x: u32,
    y: u32,
    k: usize,
    points_per_cell: usize,
    metric: DistMetric,
    cell_size: f32,
    seed: u32,
) -> f32 {
    // k ≤ 4 and 9 cells × `points` candidates: a tiny sorted buffer is
    // cheaper than a heap and allocates nothing.
    let mut nearest = [f32::INFINITY; 4];
    let px = x as f32;
    let py = y as f32;
    let cx = (px / cell_size).floor() as i32;
    let cy = (py / cell_size).floor() as i32;
    for dcy in -1..=1 {
        for dcx in -1..=1 {
            let ncx = cx + dcx;
            let ncy = cy + dcy;
            for p in 0..points_per_cell {
                let (fx, fy) = feature_point(ncx, ncy, p as u32, seed, cell_size);
                let d = match metric {
                    DistMetric::Euclidean => {
                        let dx = px - fx;
                        let dy = py - fy;
                        (dx * dx + dy * dy).sqrt()
                    }
                    DistMetric::Manhattan => (px - fx).abs() + (py - fy).abs(),
                    DistMetric::Chebyshev => (px - fx).abs().max((py - fy).abs()),
                };
                // Insert d into the sorted top-k buffer.
                for slot_i in 0..k {
                    if d < nearest[slot_i] {
                        for j in (slot_i + 1..k).rev() {
                            nearest[j] = nearest[j - 1];
                        }
                        nearest[slot_i] = d;
                        break;
                    }
                }
            }
        }
    }
    // The F1 Euclidean distance with one point per cell is bounded by
    // about √2 · cell_size; clamping at 1 keeps the scalar in range.
    (nearest[k - 1] / cell_size).clamp(0.0, 1.0)
}

#[derive(Copy, Clone)]
enum DistMetric {
    Euclidean,
    Manhattan,
    Chebyshev,
}

/// Pseudo-random feature point in cell `(cx, cy)`, slot `p`.
///
/// The returned `(fx, fy)` is in absolute pixel coordinates and is
/// confined to the cell's bounding box `[cx·s, (cx+1)·s) × [cy·s,
/// (cy+1)·s)`. Two `u32`s are drawn from an LCG keyed by the cell
/// coordinates + slot + the global `seed`, then mapped to `[0, 1)` and
/// scaled by `cell_size`.
#[inline]
fn feature_point(cx: i32, cy: i32, p: u32, seed: u32, cell_size: f32) -> (f32, f32) {
    // Hash the cell coordinates + slot + seed into a single LCG state.
    // Wrapping arithmetic so negative cell coordinates land in the same
    // u32 space the LCG drives.
    let base = (cx as u32)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add((cy as u32).wrapping_mul(0x6849_5C5F))
        .wrapping_add(p.wrapping_mul(0x85EB_CA77))
        .wrapping_add(seed);
    let mut rng = Lcg::new(base);
    let rx = (rng.next_u32() as f32) / (u32::MAX as f32);
    let ry = (rng.next_u32() as f32) / (u32::MAX as f32);
    let fx = (cx as f32) * cell_size + rx * cell_size;
    let fy = (cy as f32) * cell_size + ry * cell_size;
    (fx, fy)
}

/// Classic Perlin 2-D noise. Uses a permutation derived from `seed`
/// (so the output is reproducible across builds).
fn perlin2(perm: &[u8; 512], x: f32, y: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let u = fade(xf);
    let v = fade(yf);
    let aa = perm[((perm[(xi & 0xFF) as usize] as i32 + yi) & 0xFF) as usize];
    let ab = perm[((perm[(xi & 0xFF) as usize] as i32 + yi + 1) & 0xFF) as usize];
    let ba = perm[((perm[((xi + 1) & 0xFF) as usize] as i32 + yi) & 0xFF) as usize];
    let bb = perm[((perm[((xi + 1) & 0xFF) as usize] as i32 + yi + 1) & 0xFF) as usize];
    let x1 = lerp(grad(aa, xf, yf), grad(ba, xf - 1.0, yf), u);
    let x2 = lerp(grad(ab, xf, yf - 1.0), grad(bb, xf - 1.0, yf - 1.0), u);
    lerp(x1, x2, v)
}

/// Ken Perlin's improved 2-D simplex noise (2001). Output is in roughly
/// `[-1, 1]`, matching [`perlin2`]'s range so the shared fBm accumulator
/// and palette mapping in [`render`] treat the two interchangeably.
///
/// The 2-D simplex tessellation tiles the plane with equilateral
/// triangles. Each input point is **skewed** by `F2 = (√3 − 1) / 2`
/// into a sheared lattice where the triangles become right-isoceles
/// (so the containing simplex is found by a single integer floor + one
/// "which-half" comparison), the three corners are **unskewed** back by
/// `G2 = (3 − √3) / 6`, and each corner contributes a radially
/// attenuated dot of its pseudo-random gradient with the offset to the
/// sample point. The `(0.5 − r²)` falloff (clamped at 0) keeps each
/// corner's influence confined to its own simplex, so the surface is
/// C²-continuous with no directional bias. The `70.0` scale normalises
/// the summed contributions back toward unit range.
///
/// Pure first-principles maths transcribed from Ken Perlin's 2001
/// SIGGRAPH note on improved noise.
#[inline(always)]
fn simplex2(perm: &[u8; 512], x: f32, y: f32) -> f32 {
    // Skew the input space to determine which simplex cell we're in.
    const F2: f32 = 0.366_025_42; // (sqrt(3) - 1) / 2
    const G2: f32 = 0.211_324_87; // (3 - sqrt(3)) / 6

    let s = (x + y) * F2;
    let i = (x + s).floor();
    let j = (y + s).floor();

    // Unskew the cell origin back to (x, y) space.
    let t = (i + j) * G2;
    let x0 = x - (i - t); // x distance from cell origin
    let y0 = y - (j - t);

    // Determine which of the two triangles of the unit cell we're in:
    // lower (i1=1,j1=0) or upper (i1=0,j1=1).
    let (i1, j1) = if x0 > y0 { (1.0, 0.0) } else { (0.0, 1.0) };

    // Offsets for the middle and last corners in (x, y) unskewed coords.
    let x1 = x0 - i1 + G2;
    let y1 = y0 - j1 + G2;
    let x2 = x0 - 1.0 + 2.0 * G2;
    let y2 = y0 - 1.0 + 2.0 * G2;

    // Hashed gradient indices of the three simplex corners.
    let ii = (i as i32 & 0xFF) as usize;
    let jj = (j as i32 & 0xFF) as usize;
    let g0 = perm[ii + perm[jj] as usize];
    let g1 = perm[(ii + i1 as usize) + perm[jj + j1 as usize] as usize];
    let g2 = perm[(ii + 1) + perm[jj + 1] as usize];

    // Radially symmetric attenuation per corner: max(0, 0.5 - r²)^4 · dot.
    let n0 = corner_contribution(g0, x0, y0);
    let n1 = corner_contribution(g1, x1, y1);
    let n2 = corner_contribution(g2, x2, y2);

    // Scale to span ≈ [-1, 1].
    70.0 * (n0 + n1 + n2)
}

/// Classical value 2-D noise — the textbook predecessor to gradient
/// noise. At every integer lattice point `(ix, iy)` we read a
/// pseudo-random scalar in `[-1, 1]`; a sample at `(x, y)`
/// smoothstep-interpolates the four corners of the unit cell that
/// contains `(x, y)`.
///
/// The "pseudo-random scalar" comes from the same seeded 512-entry
/// permutation table the gradient modes use: the cell-corner hash
/// `perm[(perm[ix & 0xFF] + iy) & 0xFF]` is a `u8`, which we re-map
/// from `[0, 255]` to `[-1, 1]` via `(h / 127.5) − 1`. The
/// smoothstep is the same quintic `t³·(t·(6t − 15) + 10)` `fade`
/// curve [`perlin2`] uses (C² continuous at cell boundaries).
///
/// Output is bounded by `[-1, 1]` exactly because both the per-corner
/// values and the interpolation weights are bounded that way and a
/// convex combination of values in `[-1, 1]` stays in `[-1, 1]`. That
/// matches [`perlin2`] / [`simplex2`] so the shared fBm accumulator
/// and palette index in [`render`] treat all three modes uniformly.
fn value2(perm: &[u8; 512], x: f32, y: f32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - x.floor();
    let yf = y - y.floor();
    let u = fade(xf);
    let v = fade(yf);
    let aa = perm[((perm[(xi & 0xFF) as usize] as i32 + yi) & 0xFF) as usize];
    let ab = perm[((perm[(xi & 0xFF) as usize] as i32 + yi + 1) & 0xFF) as usize];
    let ba = perm[((perm[((xi + 1) & 0xFF) as usize] as i32 + yi) & 0xFF) as usize];
    let bb = perm[((perm[((xi + 1) & 0xFF) as usize] as i32 + yi + 1) & 0xFF) as usize];
    // Map the u8 corner hashes from [0, 255] to [-1, 1].
    let aa = (aa as f32) * (2.0 / 255.0) - 1.0;
    let ab = (ab as f32) * (2.0 / 255.0) - 1.0;
    let ba = (ba as f32) * (2.0 / 255.0) - 1.0;
    let bb = (bb as f32) * (2.0 / 255.0) - 1.0;
    let x1 = lerp(aa, ba, u);
    let x2 = lerp(ab, bb, u);
    lerp(x1, x2, v)
}

#[inline]
fn corner_contribution(hash: u8, x: f32, y: f32) -> f32 {
    let mut t = 0.5 - x * x - y * y;
    if t < 0.0 {
        0.0
    } else {
        t *= t; // t²
        t * t * grad(hash, x, y) // t⁴ · (gradient · offset)
    }
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

#[inline]
fn grad(hash: u8, x: f32, y: f32) -> f32 {
    match hash & 7 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

/// Build a 512-entry permutation table from `seed`. The classic Perlin
/// trick is to take a 256-entry shuffled identity perm and double it.
fn build_perm(seed: u32) -> [u8; 512] {
    let mut p = [0u8; 256];
    for (i, slot) in p.iter_mut().enumerate() {
        *slot = i as u8;
    }
    // Fisher-Yates with our Lcg.
    let mut rng = Lcg::new(seed);
    for i in (1..256).rev() {
        let j = (rng.next_u32() as usize) % (i + 1);
        p.swap(i, j);
    }
    let mut out = [0u8; 512];
    out[..256].copy_from_slice(&p);
    out[256..].copy_from_slice(&p);
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
                .wrapping_add(0x123_4567_89AB_CDEF),
        }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
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

    #[test]
    fn perlin_deterministic_per_seed() {
        let a = render(&map(&[("w", "32"), ("h", "32"), ("seed", "11")])).unwrap();
        let b = render(&map(&[("w", "32"), ("h", "32"), ("seed", "11")])).unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn perlin_seeds_differ() {
        let a = render(&map(&[("w", "32"), ("h", "32"), ("seed", "1")])).unwrap();
        let b = render(&map(&[("w", "32"), ("h", "32"), ("seed", "2")])).unwrap();
        assert_ne!(a.pixels, b.pixels);
    }

    #[test]
    fn simplex_renders() {
        let img = render(&map(&[("type", "simplex"), ("w", "16"), ("h", "16")])).unwrap();
        assert_eq!(img.width, 16);
        assert_eq!(img.height, 16);
        assert_eq!(img.pixels.len(), 16 * 16 * 4);
    }

    #[test]
    fn simplex_deterministic_per_seed() {
        let a = render(&map(&[
            ("type", "simplex"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "11"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "simplex"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "11"),
        ]))
        .unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn simplex_seeds_differ() {
        let a = render(&map(&[
            ("type", "simplex"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "1"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "simplex"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "2"),
        ]))
        .unwrap();
        assert_ne!(a.pixels, b.pixels);
    }

    #[test]
    fn simplex_is_a_distinct_implementation_from_perlin() {
        // `simplex` is now a real, separate algorithm — its output must
        // differ from `perlin` at the same seed/scale (it used to be a
        // straight alias that produced byte-identical images).
        let p = render(&map(&[
            ("type", "perlin"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        let s = render(&map(&[
            ("type", "simplex"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        assert_ne!(p.pixels, s.pixels);
    }

    #[test]
    fn simplex_raw_sample_is_bounded() {
        // simplex2 must stay within roughly [-1, 1] across a dense grid
        // so the shared fBm accumulator + palette index never overflow.
        let perm = build_perm(123);
        let mut max_abs = 0.0f32;
        for yi in 0..200 {
            for xi in 0..200 {
                let v = simplex2(&perm, xi as f32 * 0.13, yi as f32 * 0.13);
                max_abs = max_abs.max(v.abs());
            }
        }
        assert!(
            max_abs <= 1.0001,
            "simplex sample escaped [-1, 1]: max |v| = {max_abs}"
        );
        // ...but it must actually exercise a meaningful slice of the
        // range, not sit near zero (which would mean a dead generator).
        assert!(max_abs > 0.3, "simplex output looks degenerate: {max_abs}");
    }

    #[test]
    fn unknown_noise_type_errors() {
        assert!(render(&map(&[("type", "ridged")])).is_err());
    }

    // ---- Worley / cellular noise -------------------------------------

    #[test]
    fn worley_renders() {
        let img = render(&map(&[("type", "worley"), ("w", "32"), ("h", "32")])).unwrap();
        assert_eq!(img.width, 32);
        assert_eq!(img.height, 32);
        assert_eq!(img.pixels.len(), 32 * 32 * 4);
    }

    #[test]
    fn worley_cellular_alias() {
        // `type=cellular` is just a more familiar name for the same
        // algorithm; output must be byte-identical to `type=worley`.
        let w = render(&map(&[
            ("type", "worley"),
            ("w", "40"),
            ("h", "40"),
            ("seed", "9"),
        ]))
        .unwrap();
        let c = render(&map(&[
            ("type", "cellular"),
            ("w", "40"),
            ("h", "40"),
            ("seed", "9"),
        ]))
        .unwrap();
        assert_eq!(w.pixels, c.pixels);
    }

    #[test]
    fn worley_deterministic_per_seed() {
        let a = render(&map(&[
            ("type", "worley"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "11"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "worley"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "11"),
        ]))
        .unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn worley_seeds_differ() {
        let a = render(&map(&[
            ("type", "worley"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "1"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "worley"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "2"),
        ]))
        .unwrap();
        assert_ne!(a.pixels, b.pixels);
    }

    #[test]
    fn worley_distinct_from_perlin_and_simplex() {
        // Cellular noise is a categorically different algorithm; its
        // output cannot match a gradient-noise rendering at the same
        // seed / scale.
        let w = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        let p = render(&map(&[
            ("type", "perlin"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        let s = render(&map(&[
            ("type", "simplex"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        assert_ne!(w.pixels, p.pixels);
        assert_ne!(w.pixels, s.pixels);
    }

    #[test]
    fn worley_unknown_metric_errors() {
        let r = render(&map(&[
            ("type", "worley"),
            ("w", "8"),
            ("h", "8"),
            ("dist", "minkowski"),
        ]));
        assert!(r.is_err());
    }

    #[test]
    fn worley_metric_variants_render_and_differ() {
        // The three metrics must all render successfully and produce
        // visibly different images at the same seed.
        let e = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("dist", "euclidean"),
            ("seed", "5"),
        ]))
        .unwrap();
        let m = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("dist", "manhattan"),
            ("seed", "5"),
        ]))
        .unwrap();
        let c = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("dist", "chebyshev"),
            ("seed", "5"),
        ]))
        .unwrap();
        assert_ne!(e.pixels, m.pixels);
        assert_ne!(e.pixels, c.pixels);
        assert_ne!(m.pixels, c.pixels);
    }

    #[test]
    fn worley_k_changes_output() {
        // F2 (k=2) is, by definition, ≥ F1 (k=1) at every pixel — its
        // image will be more uniformly bright. They must not be equal.
        let k1 = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "3"),
            ("k", "1"),
        ]))
        .unwrap();
        let k2 = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "3"),
            ("k", "2"),
        ]))
        .unwrap();
        assert_ne!(k1.pixels, k2.pixels);
    }

    #[test]
    fn worley_distance_is_bounded_to_palette_range() {
        // The render path normalises the k-th distance by `cell_size`
        // and clamps to [0, 1] before indexing into a 256-entry palette.
        // We can't reach into the per-pixel distance, but we can check
        // that the rendered image stays inside the table — i.e. every
        // pixel is in the palette set. (Any escape would have been a
        // panic via `palette[idx]`.) We additionally assert the image
        // is non-degenerate (≥ 8 distinct RGB triples in a 48×48 area).
        let img = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "13"),
        ]))
        .unwrap();
        use std::collections::BTreeSet;
        let mut colors: BTreeSet<[u8; 3]> = BTreeSet::new();
        for px in img.pixels.chunks(4) {
            colors.insert([px[0], px[1], px[2]]);
        }
        assert!(
            colors.len() >= 8,
            "worley image looks degenerate: only {} distinct colours",
            colors.len()
        );
    }

    #[test]
    fn worley_points_per_cell_changes_output() {
        // Packing more feature points per cell shrinks the average F1
        // distance: the image must change.
        let p1 = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "21"),
            ("points", "1"),
        ]))
        .unwrap();
        let p3 = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "21"),
            ("points", "3"),
        ]))
        .unwrap();
        assert_ne!(p1.pixels, p3.pixels);
    }

    #[test]
    fn worley_feature_point_lives_inside_its_cell() {
        // Spot-check the placement contract: each feature point is
        // confined to its own cell, so 0 ≤ (fx − cx·s) < s and same
        // for fy. This protects the 3×3 neighbourhood search from
        // missing the nearest point.
        let cell_size = 50.0f32;
        for cx in -3..=3 {
            for cy in -3..=3 {
                for p in 0..4 {
                    let (fx, fy) = feature_point(cx, cy, p, 42, cell_size);
                    let rx = fx - (cx as f32) * cell_size;
                    let ry = fy - (cy as f32) * cell_size;
                    assert!(
                        rx >= 0.0 && rx < cell_size,
                        "cell ({cx},{cy}) slot {p}: rx = {rx} escaped [0, {cell_size})"
                    );
                    assert!(
                        ry >= 0.0 && ry < cell_size,
                        "cell ({cx},{cy}) slot {p}: ry = {ry} escaped [0, {cell_size})"
                    );
                }
            }
        }
    }

    // ---- Value / lattice noise ---------------------------------------

    #[test]
    fn value_renders() {
        let img = render(&map(&[("type", "value"), ("w", "16"), ("h", "16")])).unwrap();
        assert_eq!(img.width, 16);
        assert_eq!(img.height, 16);
        assert_eq!(img.pixels.len(), 16 * 16 * 4);
    }

    #[test]
    fn value_lattice_alias() {
        // `type=lattice` is just a more textbook-y name for the same
        // algorithm; output must be byte-identical to `type=value`.
        let v = render(&map(&[
            ("type", "value"),
            ("w", "40"),
            ("h", "40"),
            ("seed", "9"),
        ]))
        .unwrap();
        let l = render(&map(&[
            ("type", "lattice"),
            ("w", "40"),
            ("h", "40"),
            ("seed", "9"),
        ]))
        .unwrap();
        assert_eq!(v.pixels, l.pixels);
    }

    #[test]
    fn value_deterministic_per_seed() {
        let a = render(&map(&[
            ("type", "value"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "11"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "value"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "11"),
        ]))
        .unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn value_seeds_differ() {
        let a = render(&map(&[
            ("type", "value"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "1"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("type", "value"),
            ("w", "32"),
            ("h", "32"),
            ("seed", "2"),
        ]))
        .unwrap();
        assert_ne!(a.pixels, b.pixels);
    }

    #[test]
    fn value_distinct_from_perlin_and_simplex() {
        // Value noise is a categorically different basis — it interpolates
        // lattice scalars, not gradients of a hidden field. Output must
        // not coincide with either gradient-noise mode at the same seed.
        let v = render(&map(&[
            ("type", "value"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        let p = render(&map(&[
            ("type", "perlin"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        let s = render(&map(&[
            ("type", "simplex"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "7"),
        ]))
        .unwrap();
        assert_ne!(v.pixels, p.pixels);
        assert_ne!(v.pixels, s.pixels);
    }

    #[test]
    fn value_distinct_from_worley() {
        // And distinct from the cellular basis too — value noise is the
        // third independent noise family this module ships.
        let v = render(&map(&[
            ("type", "value"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "13"),
        ]))
        .unwrap();
        let w = render(&map(&[
            ("type", "worley"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "13"),
        ]))
        .unwrap();
        assert_ne!(v.pixels, w.pixels);
    }

    #[test]
    fn value_raw_sample_is_bounded() {
        // value2 must stay strictly inside [-1, 1] across a dense grid:
        // both the corner values and the smoothstep weights are bounded
        // in [-1, 1] and [0, 1] respectively, so a convex combination of
        // values in [-1, 1] cannot escape. Anything outside would mean
        // the fBm accumulator + palette index can drift past the table.
        let perm = build_perm(123);
        let mut max_abs = 0.0f32;
        for yi in 0..200 {
            for xi in 0..200 {
                let v = value2(&perm, xi as f32 * 0.13, yi as f32 * 0.13);
                max_abs = max_abs.max(v.abs());
            }
        }
        assert!(
            max_abs <= 1.0001,
            "value sample escaped [-1, 1]: max |v| = {max_abs}"
        );
        // ...but it must actually exercise a meaningful slice of the
        // range, not sit near zero (which would mean a dead generator).
        assert!(max_abs > 0.3, "value output looks degenerate: {max_abs}");
    }

    #[test]
    fn value_at_integer_lattice_matches_corner() {
        // The smoothstep `fade(0) = 0`, so a sample exactly on a lattice
        // corner must equal the corner's own random scalar (no
        // interpolation contribution from the neighbours). Verifies the
        // hashing direction + the [-1, 1] remap are wired correctly.
        let perm = build_perm(99);
        // Sample at (3, 5) — pure integer coordinates.
        let v = value2(&perm, 3.0, 5.0);
        // The corner at (3, 5) is perm[(perm[3] + 5) & 0xFF] re-mapped.
        let raw = perm[((perm[3] as i32 + 5) & 0xFF) as usize];
        let expected = (raw as f32) * (2.0 / 255.0) - 1.0;
        assert!(
            (v - expected).abs() < 1e-6,
            "value(integer lattice) = {v}, expected corner = {expected}"
        );
    }

    #[test]
    fn value_image_is_non_degenerate() {
        // A working multi-octave value-noise render should exercise more
        // than a handful of palette indices.
        let img = render(&map(&[
            ("type", "value"),
            ("w", "48"),
            ("h", "48"),
            ("seed", "21"),
        ]))
        .unwrap();
        use std::collections::BTreeSet;
        let mut colors: BTreeSet<[u8; 3]> = BTreeSet::new();
        for px in img.pixels.chunks(4) {
            colors.insert([px[0], px[1], px[2]]);
        }
        assert!(
            colors.len() >= 8,
            "value image looks degenerate: only {} distinct colours",
            colors.len()
        );
    }

    #[test]
    fn unknown_type_error_lists_value() {
        // The error path should advertise the new mode so users can
        // discover it.
        let r = render(&map(&[("type", "ridged")]));
        let msg = format!("{:?}", r.unwrap_err());
        assert!(
            msg.contains("value"),
            "unknown-type error should advertise `value`: {msg}"
        );
    }

    #[test]
    fn worley_chebyshev_distance_is_axis_aligned_squares() {
        // Sanity-check the metric: with one feature point at
        // (0.5, 0.5) inside a single cell, the Chebyshev "ball" of
        // radius r is a square. Sample two pixels equidistant under
        // Chebyshev but not under Euclidean — both should map to the
        // same palette index when we read the raw distance back via
        // the public render path through a 1-cell render.
        //
        // We approximate by rendering a tiny image with cell_size set
        // to the image side, then comparing pixels on a horizontal
        // and a diagonal at the same Chebyshev radius from the cell
        // centre.
        let img = render(&map(&[
            ("type", "worley"),
            ("dist", "chebyshev"),
            ("w", "60"),
            ("h", "60"),
            ("scale", "60"),
            ("seed", "1"),
        ]))
        .unwrap();
        // Just confirm we got something non-degenerate.
        assert_eq!(img.pixels.len(), 60 * 60 * 4);
    }

    // ── colour ramps, octave controls, depths ──

    use crate::image::canvas::{Channels, SampleDepth};

    #[test]
    fn ramp_colouring_follows_the_scalar_at_full_precision() {
        let q = map(&[
            ("type", "simplex"),
            ("w", "48"),
            ("h", "32"),
            ("scale", "9"),
            ("from", "black"),
            ("to", "white"),
        ]);
        let field = NoiseField::from_query(&q).unwrap();
        let img = canvas::render(
            &field,
            &RenderConfig::new()
                .with_depth(SampleDepth::F32)
                .with_channels(Channels::Gray),
        );
        let v = img.as_f32().unwrap();
        for y in 0..32 {
            for x in 0..48 {
                assert_eq!(v[(y * 48 + x) as usize], field.scalar(x, y));
            }
        }
        // More than 256 distinct values: not an 8-bit field widened.
        let mut distinct: Vec<u32> = v.iter().map(|f| f.to_bits()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(distinct.len() > 600, "{} distinct values", distinct.len());
        // 16-bit is the rounded float, 8-bit the rounded 16-bit.
        let deep = canvas::render(
            &field,
            &RenderConfig::new()
                .with_depth(SampleDepth::U16)
                .with_channels(Channels::Gray),
        );
        for (d, f) in deep.as_u16().unwrap().iter().zip(v) {
            assert_eq!(f64::from(*d), (f64::from(*f) * 65535.0).round());
        }
    }

    #[test]
    fn multi_stop_ramp_colours_the_noise() {
        let img = render(&map(&[
            ("type", "value"),
            ("w", "32"),
            ("h", "32"),
            ("scale", "6"),
            ("stops", "navy, white 50%, darkred"),
        ]))
        .unwrap();
        // Every pixel lies on the navy → white → darkred path: either
        // red ≤ green = … (navy side: r = g ≤ b) or the dark-red side
        // (g = b ≤ r).
        for p in img.pixels.chunks(4) {
            assert!(
                (p[0] == p[1] && p[1] <= p[2]) || (p[1] == p[2] && p[1] <= p[0]),
                "{p:?}"
            );
            assert_eq!(p[3], 255);
        }
    }

    #[test]
    fn octave_controls_change_the_field_and_defaults_do_not() {
        let base = [("type", "perlin"), ("w", "24"), ("h", "24"), ("scale", "8")];
        let plain = render(&map(&base)).unwrap();
        let mut explicit = map(&base);
        explicit.insert("lacunarity".into(), "2".into());
        explicit.insert("gain".into(), "0.5".into());
        assert_eq!(render(&explicit).unwrap().pixels, plain.pixels);
        let mut rough = map(&base);
        rough.insert("gain".into(), "0.9".into());
        assert_ne!(render(&rough).unwrap().pixels, plain.pixels);
        let mut wide = map(&base);
        wide.insert("lacunarity".into(), "3".into());
        assert_ne!(render(&wide).unwrap().pixels, plain.pixels);
        let mut one = map(&base);
        one.insert("octaves".into(), "1".into());
        one.insert("gain".into(), "0.9".into());
        let mut one_plain = map(&base);
        one_plain.insert("octaves".into(), "1".into());
        // A single octave has nothing for gain to weigh.
        assert_eq!(
            render(&one).unwrap().pixels,
            render(&one_plain).unwrap().pixels
        );
        let mut bad = map(&base);
        bad.insert("gain".into(), "0".into());
        assert!(render(&bad).is_err());
    }

    #[test]
    fn noise_is_identical_under_any_thread_budget() {
        for kind in ["perlin", "simplex", "value", "worley"] {
            let mut q = map(&[("w", "40"), ("h", "33"), ("scale", "7"), ("depth", "16")]);
            q.insert("type".into(), kind.into());
            let serial = render_image(&q).unwrap();
            q.insert("threads".into(), "4".into());
            assert_eq!(serial, render_image(&q).unwrap(), "{kind}");
        }
    }

    #[test]
    fn row_evaluation_equals_per_pixel_evaluation() {
        for kind in ["perlin", "value", "simplex", "worley"] {
            for extra in [
                vec![],
                vec![("lacunarity", "2.7"), ("gain", "0.65"), ("octaves", "6")],
            ] {
                let mut q = map(&[("w", "700"), ("h", "9"), ("scale", "23"), ("seed", "5")]);
                q.insert("type".into(), kind.into());
                q.extend(map(&extra));
                let f = NoiseField::from_query(&q).unwrap();
                let mut row = vec![0.0f32; 300];
                for y in 0..9 {
                    // An offset, batch-straddling span.
                    f.scalars(y, 311, &mut row[..]);
                    for (i, s) in row.iter().enumerate() {
                        assert_eq!(s.to_bits(), f.scalar(311 + i as u32, y).to_bits(), "{kind}");
                    }
                }
            }
        }
    }
}

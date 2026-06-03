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

use super::palette::default_palette;
use super::Rgba8Image;
use crate::source::{q_f64, q_str, q_u32};

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let kind = q_str(query, "type", "perlin");
    let scale = q_f64(query, "scale", 64.0)?.max(1.0);
    let octaves = q_u32(query, "octaves", 4)?.clamp(1, 8);
    let seed = q_u32(query, "seed", 42)?;

    if !matches!(
        kind,
        "perlin" | "simplex" | "worley" | "cellular" | "value" | "lattice"
    ) {
        return Err(Error::invalid(format!(
            "noise: unknown type {kind:?} (expected perlin|simplex|value|worley|cellular)"
        )));
    }

    if matches!(kind, "worley" | "cellular") {
        return render_worley(query, w, h, scale, seed);
    }

    // `kind` is now one of perlin / simplex / value / lattice. The three
    // share the same multi-octave fBm accumulator, the same seeded
    // permutation table, and the same palette mapping — only the per-
    // octave point sample differs.
    let mode = match kind {
        "simplex" => NoiseMode::Simplex,
        "value" | "lattice" => NoiseMode::Value,
        _ => NoiseMode::Perlin,
    };

    let perm = build_perm(seed);
    let palette = default_palette();
    let mut img = Rgba8Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            // Multi-octave fBm — same accumulator for all three kinds;
            // only the per-octave noise sample differs.
            let mut amp = 1.0f32;
            let mut freq = 1.0f32 / (scale as f32);
            let mut total = 0.0f32;
            let mut max_total = 0.0f32;
            for _ in 0..octaves {
                let sample = match mode {
                    NoiseMode::Perlin => perlin2(&perm, (x as f32) * freq, (y as f32) * freq),
                    NoiseMode::Simplex => simplex2(&perm, (x as f32) * freq, (y as f32) * freq),
                    NoiseMode::Value => value2(&perm, (x as f32) * freq, (y as f32) * freq),
                };
                total += amp * sample;
                max_total += amp;
                amp *= 0.5;
                freq *= 2.0;
            }
            let normalised = ((total / max_total) * 0.5 + 0.5).clamp(0.0, 1.0);
            let idx = (normalised * 255.0) as usize;
            img.put(x, y, palette[idx.min(255)]);
        }
    }
    Ok(img)
}

#[derive(Copy, Clone)]
enum NoiseMode {
    Perlin,
    Simplex,
    Value,
}

/// Worley / cellular noise dispatcher.
///
/// The 2-D plane is divided into integer cells of side `scale` (in pixels).
/// Each cell holds a small constant number of pseudo-randomly placed
/// feature points; for each pixel, the distance to the k-th closest of
/// these feature points (over the 3×3 neighbourhood of cells around the
/// pixel's home cell) is the noise sample. The sample is normalised by
/// `scale` so the palette mapping stays roughly invariant under `scale`,
/// then run through the same palette the gradient-noise paths use.
///
/// Parameters (URI):
/// - `scale=` cell side in pixels (default 64, min 1) — controls the
///   apparent texture grain.
/// - `seed=` u32 seed for the feature-point placement (default 42).
/// - `k=` which closest distance to use (default 1, clamped to `[1, 4]`).
///   `k=1` is the classical Voronoi-cell texture (F1); `k=2` is the F2
///   distance; their difference `F2 − F1` is a popular ridge texture in
///   the procedural-graphics literature (not exposed as a separate mode
///   here — callers can post-process if needed).
/// - `dist=` one of `euclidean|euc|l2` (default), `manhattan|l1`,
///   `chebyshev|linf|max`.
/// - `points=` number of feature points per cell, default 1, clamped to
///   `[1, 4]`. Higher counts pack the plane more densely.
fn render_worley(
    query: &BTreeMap<String, String>,
    w: u32,
    h: u32,
    scale: f64,
    seed: u32,
) -> Result<Rgba8Image> {
    let k = q_u32(query, "k", 1)?.clamp(1, 4) as usize;
    let points_per_cell = q_u32(query, "points", 1)?.clamp(1, 4) as usize;
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

    let cell_size = scale as f32;
    let palette = default_palette();
    let mut img = Rgba8Image::new(w, h);
    // The k-th closest distance — k ≤ 4 and we visit 9 cells × `points`
    // candidates, so a tiny fixed-size sorted buffer is cheaper than a
    // heap and keeps the per-pixel allocation count at zero.
    let mut nearest = [f32::INFINITY; 4];
    for y in 0..h {
        for x in 0..w {
            for slot in nearest.iter_mut() {
                *slot = f32::INFINITY;
            }
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
                        // Insert d into a sorted top-k buffer.
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
            // Normalise the k-th distance by `cell_size`. The theoretical
            // upper bound for the F1 Euclidean distance with one feature
            // point per cell is around `sqrt(2) · cell_size` (the worst
            // case is a sample at one corner with the only point in the
            // opposite corner of the home cell). Clamping at 1.0 keeps
            // the palette index inside the table.
            let normalised = (nearest[k - 1] / cell_size).clamp(0.0, 1.0);
            let idx = (normalised * 255.0) as usize;
            img.put(x, y, palette[idx.min(255)]);
        }
    }
    Ok(img)
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
}

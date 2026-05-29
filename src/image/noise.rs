//! Gradient noise generators: classic Perlin 2-D (`type=perlin`) and
//! Ken Perlin's improved simplex 2-D (`type=simplex`).
//!
//! Both are pure first-principles implementations — no spec and no
//! external-library source. They share the seeded 512-entry
//! permutation table (`build_perm`), so the same `seed=` is
//! bit-deterministic across builds for both kinds, and they share the
//! multi-octave fBm accumulator and the palette mapping in [`render`].

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

    if !matches!(kind, "perlin" | "simplex") {
        return Err(Error::invalid(format!(
            "noise: unknown type {kind:?} (expected perlin|simplex)"
        )));
    }
    let simplex = kind == "simplex";

    let perm = build_perm(seed);
    let palette = default_palette();
    let mut img = Rgba8Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            // Multi-octave fBm — same accumulator for both kinds; only
            // the per-octave gradient-noise sample differs.
            let mut amp = 1.0f32;
            let mut freq = 1.0f32 / (scale as f32);
            let mut total = 0.0f32;
            let mut max_total = 0.0f32;
            for _ in 0..octaves {
                let sample = if simplex {
                    simplex2(&perm, (x as f32) * freq, (y as f32) * freq)
                } else {
                    perlin2(&perm, (x as f32) * freq, (y as f32) * freq)
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
/// Pure first-principles maths — no spec, no external-library source.
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
}

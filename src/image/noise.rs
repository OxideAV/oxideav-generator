//! Perlin noise (and a Perlin alias for `simplex` until we have a real one).

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

    let perm = build_perm(seed);
    let palette = default_palette();
    let mut img = Rgba8Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            // Multi-octave fBm.
            let mut amp = 1.0f32;
            let mut freq = 1.0f32 / (scale as f32);
            let mut total = 0.0f32;
            let mut max_total = 0.0f32;
            for _ in 0..octaves {
                total += amp * perlin2(&perm, (x as f32) * freq, (y as f32) * freq);
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
    fn simplex_alias_routes_to_perlin() {
        // Simplex-named output renders OK (we alias it for now).
        let img = render(&map(&[("type", "simplex"), ("w", "16"), ("h", "16")])).unwrap();
        assert_eq!(img.width, 16);
    }

    #[test]
    fn unknown_noise_type_errors() {
        assert!(render(&map(&[("type", "ridged")])).is_err());
    }
}

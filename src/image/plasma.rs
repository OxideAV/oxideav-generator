//! Plasma cloud via diamond-square (recursive midpoint displacement).

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::palette::default_palette;
use super::Rgba8Image;
use crate::source::{q_f64, q_u32};

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(2);
    let h = q_u32(query, "h", 480)?.max(2);
    let seed = q_u32(query, "seed", 42)?;
    let roughness = q_f64(query, "roughness", 0.7)? as f32;

    let height_field = diamond_square(w as usize, h as usize, seed, roughness);
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
            let x_off = if (y / half) % 2 == 0 { half } else { 0 };
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
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
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
}

//! Identity Hald colour look-up table (the `hald:N` pseudo-image).
//!
//! A level-`N` Hald CLUT samples the RGB cube on a regular grid of
//! `n = N²` points per channel, stored as an `N³ × N³` image (`n³`
//! pixels). Pixels are laid out row-major with red varying fastest,
//! then green, then blue:
//!
//! ```text
//! i      = y · N³ + x                  (0 … n³ − 1)
//! r_idx  = i mod n
//! g_idx  = ⌊i / n⌋ mod n
//! b_idx  = ⌊i / n²⌋
//! value  = idx / (n − 1)               (per channel, in [0, 1])
//! ```
//!
//! Applying a colour transform to this image and using the result as a
//! CLUT reproduces that transform; the untouched image is the identity
//! table. Alpha is always opaque.
//!
//! Values quantise through 16 bits and then to 8 bits, both rounding
//! half-up, so output is byte-identical to `magick hald:N -depth 8
//! rgba:-` (checked by `tests/magick_parity.rs`).
//!
//! Level rules (taken from the `magick` binary's behaviour): a
//! fractional level truncates, a level below 2 selects the default
//! level 8 (512 × 512). Levels above [`MAX_LEVEL`] are refused: they
//! would exceed 4096 × 4096 and sample the cube more finely than 8-bit
//! output can represent.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::Rgba8Image;

/// Level used when none is given, or when the level is below 2.
pub const DEFAULT_LEVEL: u32 = 8;

/// Largest supported level: 16 → a 4096 × 4096 image whose 256 samples
/// per channel are exactly the 8-bit code values.
pub const MAX_LEVEL: u32 = 16;

/// Normalise a requested level: truncate, below 2 → [`DEFAULT_LEVEL`],
/// above [`MAX_LEVEL`] → error.
pub fn effective_level(level: f64) -> Result<u32> {
    if level.is_nan() || level < 2.0 {
        return Ok(DEFAULT_LEVEL);
    }
    let l = level.trunc();
    if l > MAX_LEVEL as f64 {
        return Err(Error::invalid(format!(
            "hald: level {level} exceeds the maximum {MAX_LEVEL} (a {0}x{0} image)",
            MAX_LEVEL.pow(3)
        )));
    }
    Ok(l as u32)
}

/// Render the identity Hald CLUT at `level` (already normalised by
/// [`effective_level`]; values outside `2..=MAX_LEVEL` are normalised
/// the same way).
pub fn render_level(level: u32) -> Result<Rgba8Image> {
    let level = effective_level(level as f64)?;
    let n = (level * level) as usize;
    let side = level * level * level;
    let max = (n - 1) as u32;
    // 8-bit code for each grid index (Q16 then 8-bit, half-up).
    let codes: Vec<u8> = (0..n as u32)
        .map(|i| {
            let q16 = (i * 65535 + max / 2) / max;
            ((q16 + 128) / 257) as u8
        })
        .collect();
    let mut img = Rgba8Image::new(side, side);
    for (i, px) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = codes[i % n];
        px[1] = codes[(i / n) % n];
        px[2] = codes[i / (n * n)];
        px[3] = 255;
    }
    Ok(img)
}

/// Render from a `generate://hald?…` query.
///
/// | Key     | Default | Meaning                                         |
/// |---------|---------|-------------------------------------------------|
/// | `level` | 8       | Hald level `N`; image is `N³ × N³`, `N²` samples per channel. Fractions truncate, `< 2` → 8, max 16. |
///
/// `w` / `h` are ignored: the level alone fixes the size.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let level = match query.get("level").map(|s| s.trim()) {
        None | Some("") => DEFAULT_LEVEL as f64,
        Some(s) => s
            .parse::<f64>()
            .map_err(|_| Error::invalid(format!("hald: level {s:?} is not a number")))?,
    };
    render_level(effective_level(level)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(level: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("level".to_string(), level.to_string());
        m
    }

    #[test]
    fn level_two_matches_magick_bytes() {
        // `magick hald:2 -depth 8 rgb:-`: 8x8, 4 samples per channel
        // (0, 0x55, 0xaa, 0xff), red fastest.
        let img = render(&q("2")).unwrap();
        assert_eq!((img.width, img.height), (8, 8));
        let s = [0u8, 0x55, 0xaa, 0xff];
        for i in 0..64usize {
            let px = &img.pixels[i * 4..i * 4 + 4];
            assert_eq!(px, [s[i % 4], s[(i / 4) % 4], s[i / 16], 255], "pixel {i}");
        }
    }

    #[test]
    fn level_rules() {
        assert_eq!(effective_level(0.0).unwrap(), 8);
        assert_eq!(effective_level(1.0).unwrap(), 8);
        assert_eq!(effective_level(-3.0).unwrap(), 8);
        assert_eq!(effective_level(2.5).unwrap(), 2);
        assert_eq!(effective_level(16.0).unwrap(), 16);
        assert!(effective_level(17.0).is_err());
        assert!(render(&q("abc")).is_err());
        let d = render(&BTreeMap::new()).unwrap();
        assert_eq!((d.width, d.height), (512, 512));
    }

    #[test]
    fn level_sixteen_is_the_exact_8bit_cube() {
        let img = render_level(16).unwrap();
        assert_eq!((img.width, img.height), (4096, 4096));
        // Every (r, g, b) code triple appears exactly once, at index
        // r + 256 g + 65536 b.
        for (i, px) in img.pixels.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(
                *px,
                [
                    (i & 255) as u8,
                    ((i >> 8) & 255) as u8,
                    (i >> 16) as u8,
                    255
                ]
            );
        }
    }

    #[test]
    fn corners_are_black_and_white() {
        for level in 2..=MAX_LEVEL.min(9) {
            let img = render_level(level).unwrap();
            let s = img.width;
            assert_eq!(img.get(0, 0), [0, 0, 0, 255]);
            assert_eq!(img.get(s - 1, s - 1), [255, 255, 255, 255]);
        }
    }
}

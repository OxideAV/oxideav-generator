//! Per-channel gradient ramp at a configurable bit depth — the
//! quantisation / banding / channel-crosstalk probe.
//!
//! Along the ramp axis (horizontal by default), position `p` in a
//! span of `len` pixels maps to one of `2^bits` quantisation levels
//! and then to an 8-bit code, both in exact integer arithmetic:
//!
//! ```text
//! level(p) = ⌊p · 2^bits / len⌋                 (0 … 2^bits − 1)
//! value(p) = round(level · 255 / (2^bits − 1))  (0 … 255)
//! ```
//!
//! Every quantisation step is therefore exactly `⌈len / 2^bits⌉` or
//! `⌊len / 2^bits⌋` pixels wide, values are monotone non-decreasing,
//! the first pixel is always 0, and (whenever `len ≥ 2^bits`) the
//! last pixel is always 255. At `bits=8` with `len = 256` the ramp is
//! the identity — pixel column `x` has value exactly `x`.
//!
//! `channel=` selects which channel carries the ramp: `gray` (default)
//! writes it to R, G, and B simultaneously; `r` / `g` / `b` write it
//! to a single channel with the other two at 0, exposing per-channel
//! gamma / dithering / subsampling crosstalk. Alpha is always 255.
//!
//! Low `bits=` values produce deliberately banded ramps with exactly
//! known step positions — the reference input for banding-detection,
//! dithering, and bit-depth-conversion tests.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::Rgba8Image;
use crate::source::{q_str, q_u32};

/// Render a quantised ramp image.
///
/// Recognised query parameters:
///
/// | Key         | Default      | Meaning                                     |
/// |-------------|--------------|---------------------------------------------|
/// | `w` / `h`   | 640/480      | Output resolution in pixels                 |
/// | `direction` | `horizontal` | `horizontal` (alias `h`) / `vertical` (`v`) |
/// | `bits`      | 8            | Quantisation depth, 1…8 → `2^bits` levels   |
/// | `channel`   | `gray`       | `gray` (aliases `grey`/`luma`/`all`) or one |
/// |             |              | of `r`/`red`, `g`/`green`, `b`/`blue`       |
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let bits = q_u32(query, "bits", 8)?;
    if !(1..=8).contains(&bits) {
        return Err(Error::invalid(format!(
            "ramp: bits must be in 1..=8, got {bits}"
        )));
    }
    let horizontal = match q_str(query, "direction", "horizontal") {
        "horizontal" | "h" => true,
        "vertical" | "v" => false,
        other => {
            return Err(Error::invalid(format!(
                "ramp: unknown direction {other:?} (expected horizontal|vertical)"
            )));
        }
    };
    let channel = q_str(query, "channel", "gray");
    // Per-pixel writer: mask of which RGB channels carry the ramp.
    let mask: [bool; 3] = match channel {
        "gray" | "grey" | "luma" | "all" => [true, true, true],
        "r" | "red" => [true, false, false],
        "g" | "green" => [false, true, false],
        "b" | "blue" => [false, false, true],
        other => {
            return Err(Error::invalid(format!(
                "ramp: unknown channel {other:?} (expected gray|r|g|b)"
            )));
        }
    };

    let levels = 1u32 << bits; // 2^bits
    let len = if horizontal { w } else { h };

    // Precompute the code for each position along the ramp axis.
    let codes: Vec<u8> = (0..len).map(|p| code_at(p, len, levels)).collect();

    let mut img = Rgba8Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = codes[if horizontal { x } else { y } as usize];
            let px = [
                if mask[0] { v } else { 0 },
                if mask[1] { v } else { 0 },
                if mask[2] { v } else { 0 },
                255,
            ];
            img.put(x, y, px);
        }
    }
    Ok(img)
}

/// The module-doc closed form: quantisation level then 8-bit code, in
/// exact integer arithmetic (round half up on the code expansion).
#[inline]
pub fn code_at(p: u32, len: u32, levels: u32) -> u8 {
    let level = (p as u64 * levels as u64 / len as u64) as u32;
    (((level as u64) * 255 + (levels as u64 - 1) / 2) / (levels as u64 - 1)) as u8
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
    fn ramp_256_wide_8bit_is_the_identity() {
        // bits=8, w=256: level(x) = x and code(x) = x exactly.
        let img = render(&map(&[("w", "256"), ("h", "2"), ("bits", "8")])).unwrap();
        for x in 0..256u32 {
            let want = x as u8;
            assert_eq!(img.get(x, 0), [want, want, want, 255], "column {x}");
            assert_eq!(img.get(x, 1), [want, want, want, 255], "column {x}");
        }
    }

    #[test]
    fn ramp_1bit_splits_at_midpoint() {
        // bits=1, w=8: level = ⌊x·2/8⌋ → 0 for x<4, 1 for x≥4;
        // codes 0 and 255.
        let img = render(&map(&[("w", "8"), ("h", "1"), ("bits", "1")])).unwrap();
        for x in 0..8u32 {
            let want = if x < 4 { 0 } else { 255 };
            assert_eq!(img.get(x, 0)[0], want, "column {x}");
        }
    }

    #[test]
    fn ramp_2bit_codes_are_0_85_170_255() {
        // bits=2, w=8: levels 0,0,1,1,2,2,3,3 → round(level·255/3) =
        // 0, 85, 170, 255.
        let img = render(&map(&[("w", "8"), ("h", "1"), ("bits", "2")])).unwrap();
        let want = [0u8, 0, 85, 85, 170, 170, 255, 255];
        for x in 0..8u32 {
            assert_eq!(img.get(x, 0)[0], want[x as usize], "column {x}");
        }
    }

    #[test]
    fn ramp_vertical_ramps_along_y() {
        let img = render(&map(&[
            ("w", "2"),
            ("h", "8"),
            ("bits", "1"),
            ("direction", "vertical"),
        ]))
        .unwrap();
        for y in 0..8u32 {
            let want = if y < 4 { 0 } else { 255 };
            assert_eq!(img.get(0, y)[0], want, "row {y}");
            assert_eq!(img.get(1, y)[0], want, "row {y}");
        }
    }

    #[test]
    fn ramp_single_channel_isolates() {
        for (name, idx) in [("r", 0usize), ("g", 1), ("b", 2)] {
            let img = render(&map(&[
                ("w", "8"),
                ("h", "1"),
                ("bits", "1"),
                ("channel", name),
            ]))
            .unwrap();
            let px = img.get(7, 0);
            for (c, &got) in px.iter().enumerate().take(3) {
                let want = if c == idx { 255 } else { 0 };
                assert_eq!(got, want, "channel={name}, component {c}");
            }
            assert_eq!(px[3], 255);
        }
    }

    #[test]
    fn ramp_is_monotone_non_decreasing() {
        let img = render(&map(&[("w", "100"), ("h", "1"), ("bits", "5")])).unwrap();
        let mut prev = 0u8;
        for x in 0..100u32 {
            let v = img.get(x, 0)[0];
            assert!(v >= prev, "column {x}: {v} < {prev}");
            prev = v;
        }
    }

    #[test]
    fn ramp_hits_exact_level_count_and_endpoints() {
        // len ≥ 2^bits ⇒ every level appears; first pixel 0, last 255.
        let img = render(&map(&[("w", "64"), ("h", "1"), ("bits", "4")])).unwrap();
        let mut distinct = std::collections::BTreeSet::new();
        for x in 0..64u32 {
            distinct.insert(img.get(x, 0)[0]);
        }
        assert_eq!(distinct.len(), 16, "2^4 distinct codes");
        assert_eq!(img.get(0, 0)[0], 0);
        assert_eq!(img.get(63, 0)[0], 255);
    }

    #[test]
    fn ramp_bits_out_of_range_errors() {
        assert!(render(&map(&[("bits", "0")])).is_err());
        assert!(render(&map(&[("bits", "9")])).is_err());
    }

    #[test]
    fn ramp_unknown_channel_or_direction_errors() {
        assert!(render(&map(&[("channel", "chartreuse")])).is_err());
        assert!(render(&map(&[("direction", "diagonal")])).is_err());
    }

    #[test]
    fn ramp_is_deterministic() {
        let args = &[("w", "32"), ("h", "4"), ("bits", "3")];
        let a = render(&map(args)).unwrap();
        let b = render(&map(args)).unwrap();
        assert_eq!(a.pixels, b.pixels);
    }
}

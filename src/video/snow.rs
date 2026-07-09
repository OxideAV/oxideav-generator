//! Seeded temporal noise ("TV snow") — every pixel of every frame is
//! an independent pseudo-random value drawn from a *stateless*
//! counter-mode hash of `(seed, frame, x, y)`.
//!
//! There is no sequential PRNG stream: the value of any pixel is a
//! pure function of its coordinates, so the output is exactly
//! reproducible per-pixel (not just per-buffer), frames can be
//! recomputed in any order, and the closed form is pinned by tests.
//! The word for pixel `(x, y)` of frame `f` is
//!
//! ```text
//! v = seed ⊕ (f · 0x9E3779B9) ⊕ (x · 0x85EBCA6B) ⊕ (y · 0xC2B2AE35)
//! v ^= v >> 16;  v *= 0x7FEB352D;
//! v ^= v >> 15;  v *= 0x846CA68B;
//! v ^= v >> 16;
//! ```
//!
//! (all arithmetic mod 2³²; the multipliers are odd so each stage is a
//! bijection of the 32-bit space — an xorshift-multiply integer
//! finalizer built from first principles). `mode=mono` maps the top
//! byte `v >> 24` to an achromatic grey; `mode=rgb` maps bytes 2/1/0
//! of `v` to R/G/B. Alpha is always 255.
//!
//! As a codec probe, full-frame independent noise is the
//! worst-case-entropy signal: intra prediction, motion search, and
//! transform coding all gain nothing, making it the standard stress
//! input for rate-control and bitstream-buffer paths. Determinism by
//! `(seed, frame, x, y)` means two runs — or two machines — produce
//! byte-identical streams.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::FrameSeq;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_str, q_u32};

/// The counter-mode hash: a pure function of `(seed, frame, x, y)`.
///
/// This exact mapping is a documented output contract (tests pin it),
/// not an implementation detail — changing any constant changes every
/// generated stream.
#[inline]
pub fn mix(seed: u32, frame: u32, x: u32, y: u32) -> u32 {
    let mut v = seed
        ^ frame.wrapping_mul(0x9E37_79B9)
        ^ x.wrapping_mul(0x85EB_CA6B)
        ^ y.wrapping_mul(0xC2B2_AE35);
    v ^= v >> 16;
    v = v.wrapping_mul(0x7FEB_352D);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846C_A68B);
    v ^= v >> 16;
    v
}

/// Render a seeded temporal-noise sequence.
///
/// Recognised query parameters:
///
/// | Key        | Default | Meaning                                        |
/// |------------|---------|------------------------------------------------|
/// | `w` / `h`  | 320/240 | Output resolution in pixels                    |
/// | `duration` | 5       | Seconds                                        |
/// | `fps`      | 30      | Frames per second                              |
/// | `seed`     | 42      | Hash seed — same seed ⇒ byte-identical frames  |
/// | `mode`     | `mono`  | `mono` (grey, aliases `gray`/`grey`) or `rgb`  |
/// |            |         | (independent channels, alias `color`)          |
pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 320)?.max(1);
    let h = q_u32(query, "h", 240)?.max(1);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let seed = q_u32(query, "seed", 42)?;
    let mode = q_str(query, "mode", "mono");
    let rgb = match mode {
        "mono" | "gray" | "grey" => false,
        "rgb" | "color" | "colour" => true,
        other => {
            return Err(Error::invalid(format!(
                "snow: unknown mode {other:?} (expected mono|rgb)"
            )));
        }
    };

    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);

    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let mut img = Rgba8Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = mix(seed, f as u32, x, y);
                let px = if rgb {
                    [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
                } else {
                    let g = (v >> 24) as u8;
                    [g, g, g, 255]
                };
                img.put(x, y, px);
            }
        }
        frames.push(img);
    }
    Ok(FrameSeq { frames, fps })
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

    /// The module-doc hash, restated with its own literals — if an
    /// impl constant drifts, this recomputation catches it.
    fn mix_reference(seed: u32, f: u32, x: u32, y: u32) -> u32 {
        let mut v = seed
            ^ f.wrapping_mul(0x9E37_79B9)
            ^ x.wrapping_mul(0x85EB_CA6B)
            ^ y.wrapping_mul(0xC2B2_AE35);
        v ^= v >> 16;
        v = v.wrapping_mul(0x7FEB_352D);
        v ^= v >> 15;
        v = v.wrapping_mul(0x846C_A68B);
        v ^= v >> 16;
        v
    }

    #[test]
    fn snow_mono_pixels_match_documented_hash() {
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "6"),
            ("seed", "7"),
            ("duration", "0.2"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 2);
        for (f, img) in seq.frames.iter().enumerate() {
            for y in 0..6u32 {
                for x in 0..8u32 {
                    let g = (mix_reference(7, f as u32, x, y) >> 24) as u8;
                    assert_eq!(img.get(x, y), [g, g, g, 255], "frame {f}, pixel ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn snow_rgb_pixels_match_documented_hash() {
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "6"),
            ("seed", "9"),
            ("mode", "rgb"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        for y in 0..6u32 {
            for x in 0..8u32 {
                let v = mix_reference(9, 0, x, y);
                assert_eq!(
                    seq.frames[0].get(x, y),
                    [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255],
                    "pixel ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn snow_is_deterministic_across_renders() {
        let args = &[
            ("w", "16"),
            ("h", "16"),
            ("seed", "42"),
            ("duration", "0.3"),
            ("fps", "10"),
        ];
        let a = render(&map(args)).unwrap();
        let b = render(&map(args)).unwrap();
        assert_eq!(a.frames.len(), b.frames.len());
        for (fa, fb) in a.frames.iter().zip(b.frames.iter()) {
            assert_eq!(fa.pixels, fb.pixels, "same seed must be byte-identical");
        }
    }

    #[test]
    fn snow_different_seeds_differ() {
        let base = &[("w", "16"), ("h", "16"), ("duration", "0.1"), ("fps", "10")];
        let mut qa = map(base);
        qa.insert("seed".into(), "1".into());
        let mut qb = map(base);
        qb.insert("seed".into(), "2".into());
        let a = render(&qa).unwrap();
        let b = render(&qb).unwrap();
        assert_ne!(a.frames[0].pixels, b.frames[0].pixels);
    }

    #[test]
    fn snow_consecutive_frames_differ() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.2"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 2);
        assert_ne!(seq.frames[0].pixels, seq.frames[1].pixels);
    }

    #[test]
    fn snow_mono_is_achromatic() {
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        for p in seq.frames[0].pixels.chunks_exact(4) {
            assert_eq!(p[0], p[1]);
            assert_eq!(p[1], p[2]);
            assert_eq!(p[3], 255);
        }
    }

    #[test]
    fn snow_grey_level_mean_is_near_midpoint() {
        // 64×64 = 4096 samples of a (near-)uniform byte distribution:
        // mean 127.5, σ ≈ 73.9, σ of the mean ≈ 1.16 — a ±6 window is
        // beyond 5σ, so this only fails if the hash is badly biased.
        let seq = render(&map(&[
            ("w", "64"),
            ("h", "64"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        let sum: u64 = seq.frames[0]
            .pixels
            .chunks_exact(4)
            .map(|p| p[0] as u64)
            .sum();
        let mean = sum as f64 / (64.0 * 64.0);
        assert!(
            (mean - 127.5).abs() < 6.0,
            "grey mean {mean} too far from 127.5"
        );
    }

    #[test]
    fn snow_unknown_mode_errors() {
        assert!(render(&map(&[("mode", "sepia")])).is_err());
    }
}

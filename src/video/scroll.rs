//! Constant-velocity scrolling pattern — toroidal translation of a
//! static base pattern, the canonical motion-estimation ground-truth
//! probe.
//!
//! A base frame is rendered once by one of the in-tree image
//! generators, then frame `n` is exactly the base frame translated by
//! `(n·vx, n·vy)` pixels with toroidal (wrap-around) addressing:
//!
//! ```text
//! frame_n(x, y) = base((x − n·vx) mod w, (y − n·vy) mod h)
//! ```
//!
//! `vx` / `vy` are signed integer pixels-per-frame, so the true motion
//! field is *globally constant and known exactly* — every pixel of
//! every frame is a bit-exact copy of a base-frame pixel, no
//! resampling, no interpolation, no boundary effects. That makes the
//! sequence ideal for validating codec motion search (the estimated
//! vector field should be uniformly `(vx, vy)`), temporal prediction
//! (frame `n` predicted from frame `n−1` with the true vector is
//! residual-free), and wrap-period logic (when `vx` divides `w`, the
//! sequence is periodic with period `w / vx` frames).
//!
//! Pure first-principles construction: translation on the torus
//! `Z_w × Z_h` is the only operation involved.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::FrameSeq;
use crate::image::{grating, pattern, plasma, Rgba8Image};
use crate::source::{q_f64, q_i32, q_str, q_u32};

/// Render a scrolling-pattern sequence.
///
/// Recognised query parameters:
///
/// | Key        | Default        | Meaning                                            |
/// |------------|----------------|----------------------------------------------------|
/// | `pattern`  | `checkerboard` | Base frame: `checkerboard` / `hstripes` /          |
/// |            |                | `vstripes` (+ long/`*bars` aliases) / `grating` /  |
/// |            |                | `plasma`                                           |
/// | `vx`       | 1              | Signed horizontal velocity, pixels per frame       |
/// | `vy`       | 0              | Signed vertical velocity, pixels per frame         |
/// | `w` / `h`  | 640/480        | Output resolution in pixels                        |
/// | `duration` | 5              | Seconds                                            |
/// | `fps`      | 30             | Frames per second                                  |
///
/// All remaining keys are forwarded to the base generator unchanged
/// (`size` / `color1` / `color2` for the pattern family, `freq` /
/// `angle` / `phase` / `amplitude` for `grating`, `seed` / `roughness`
/// for `plasma`), so the base frame is bit-identical to what the
/// matching still-image generator produces from the same query.
pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);
    let vx = q_i32(query, "vx", 1)? as i64;
    let vy = q_i32(query, "vy", 0)? as i64;

    let base = render_base(query)?;

    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count as i64 {
        frames.push(translate_wrap(&base, f * vx, f * vy));
    }
    Ok(FrameSeq { frames, fps })
}

/// Render the base frame by dispatching to the matching in-tree image
/// generator. The scroll-specific keys (`pattern`, `vx`, `vy`,
/// `duration`, `fps`) are simply ignored by the image generators, so
/// the full query map is forwarded as-is — except for the pattern
/// family, where `pattern=` is copied into the `type=` key the
/// `pattern` generator dispatches on.
fn render_base(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let kind = q_str(query, "pattern", "checkerboard");
    match kind {
        "checkerboard" | "checker" | "horizontal_stripes" | "hstripes" | "hbars"
        | "vertical_stripes" | "vstripes" | "vbars" => {
            let mut q = query.clone();
            q.insert("type".to_string(), kind.to_string());
            pattern::render(&q)
        }
        "grating" => grating::render(query),
        "plasma" => plasma::render(query),
        other => Err(Error::invalid(format!(
            "scroll: unknown pattern {other:?} (expected checkerboard|hstripes|vstripes|grating|plasma)"
        ))),
    }
}

/// Translate `base` by `(dx, dy)` pixels with toroidal wrap-around:
/// `out(x, y) = base((x − dx) mod w, (y − dy) mod h)`. Positive `dx`
/// moves content rightward, positive `dy` moves content downward.
///
/// Implemented as one wrapped row lookup plus two contiguous byte
/// copies per output row (the row split at the horizontal seam), so
/// every output pixel is a bit-exact copy — no arithmetic on pixel
/// values at all.
fn translate_wrap(base: &Rgba8Image, dx: i64, dy: i64) -> Rgba8Image {
    let w = base.width as usize;
    let h = base.height as usize;
    let off_x = dx.rem_euclid(w as i64) as usize;
    let off_y = dy.rem_euclid(h as i64) as usize;

    let row_bytes = w * 4;
    let split = (w - off_x) * 4; // bytes of src row landing at dst column off_x..

    let mut out = Rgba8Image::new(base.width, base.height);
    for y in 0..h {
        // dst row y reads from src row (y − dy) mod h.
        let sy = (y + h - off_y) % h;
        let src = &base.pixels[sy * row_bytes..(sy + 1) * row_bytes];
        let dst = &mut out.pixels[y * row_bytes..(y + 1) * row_bytes];
        // dst(x) = src((x − off_x) mod w):
        //   dst[off_x .. w]  = src[0 .. w − off_x]
        //   dst[0 .. off_x]  = src[w − off_x .. w]
        dst[off_x * 4..].copy_from_slice(&src[..split]);
        dst[..off_x * 4].copy_from_slice(&src[split..]);
    }
    out
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

    /// The headline ground-truth property: every pixel of frame `n` is
    /// the base-frame pixel at the toroidally back-translated
    /// coordinate. Uses a plasma base (rich, aperiodic content) so an
    /// off-by-one anywhere cannot hide behind pattern periodicity.
    #[test]
    fn scroll_frame_n_is_frame0_translated_toroidally() {
        let (w, h, vx, vy) = (16i64, 12i64, 3i64, 2i64);
        let seq = render(&map(&[
            ("pattern", "plasma"),
            ("w", "16"),
            ("h", "12"),
            ("vx", "3"),
            ("vy", "2"),
            ("duration", "0.5"),
            ("fps", "10"),
            ("seed", "7"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 5);
        let base = &seq.frames[0];
        for (n, frame) in seq.frames.iter().enumerate() {
            let n = n as i64;
            for y in 0..h {
                for x in 0..w {
                    let sx = (x - n * vx).rem_euclid(w) as u32;
                    let sy = (y - n * vy).rem_euclid(h) as u32;
                    assert_eq!(
                        frame.get(x as u32, y as u32),
                        base.get(sx, sy),
                        "frame {n} pixel ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn scroll_zero_velocity_is_static() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("vx", "0"),
            ("vy", "0"),
            ("duration", "0.3"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 3);
        let first = &seq.frames[0].pixels;
        for f in &seq.frames[1..] {
            assert_eq!(&f.pixels, first, "vx=vy=0 must be frame-identical");
        }
    }

    /// On the torus, velocity is only defined mod the frame size:
    /// `−2 ≡ 14 (mod 16)`, so the two sequences must be bit-identical.
    #[test]
    fn scroll_negative_velocity_equals_modular_complement() {
        let common = [
            ("pattern", "plasma"),
            ("w", "16"),
            ("h", "8"),
            ("duration", "0.4"),
            ("fps", "10"),
            ("seed", "3"),
        ];
        let mut neg = map(&common);
        neg.insert("vx".into(), "-2".into());
        let mut pos = map(&common);
        pos.insert("vx".into(), "14".into());
        let a = render(&neg).unwrap();
        let b = render(&pos).unwrap();
        assert_eq!(a.frames.len(), b.frames.len());
        for (fa, fb) in a.frames.iter().zip(&b.frames) {
            assert_eq!(fa.pixels, fb.pixels);
        }
    }

    /// Velocity wraps mod the frame size in the same way: vx=20 on a
    /// 16-wide frame is bit-identical to vx=4.
    #[test]
    fn scroll_velocity_exceeding_width_wraps() {
        let common = [
            ("pattern", "plasma"),
            ("w", "16"),
            ("h", "8"),
            ("duration", "0.3"),
            ("fps", "10"),
            ("seed", "5"),
        ];
        let mut big = map(&common);
        big.insert("vx".into(), "20".into());
        let mut small = map(&common);
        small.insert("vx".into(), "4".into());
        let a = render(&big).unwrap();
        let b = render(&small).unwrap();
        for (fa, fb) in a.frames.iter().zip(&b.frames) {
            assert_eq!(fa.pixels, fb.pixels);
        }
    }

    /// When `vx` divides `w`, the sequence is periodic with period
    /// `w / vx` frames: frame 4 of a vx=4 scroll on a 16-wide frame
    /// has shifted exactly one full revolution.
    #[test]
    fn scroll_full_period_wraps_to_frame0() {
        let seq = render(&map(&[
            ("pattern", "plasma"),
            ("w", "16"),
            ("h", "8"),
            ("vx", "4"),
            ("vy", "0"),
            ("duration", "0.5"),
            ("fps", "10"),
            ("seed", "11"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 5);
        assert_eq!(seq.frames[4].pixels, seq.frames[0].pixels);
        // ...and the intermediate frames are NOT the base frame.
        assert_ne!(seq.frames[1].pixels, seq.frames[0].pixels);
    }

    /// Frame 0 must be bit-identical to the matching still-image
    /// generator's output from the same query — the scroll layer adds
    /// nothing at n = 0.
    #[test]
    fn scroll_frame0_matches_direct_pattern_render() {
        let q = map(&[
            ("w", "20"),
            ("h", "20"),
            ("size", "5"),
            ("color1", "red"),
            ("color2", "blue"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]);
        let seq = render(&q).unwrap();
        let mut direct_q = q.clone();
        direct_q.insert("type".into(), "checkerboard".into());
        let direct = pattern::render(&direct_q).unwrap();
        assert_eq!(seq.frames[0].pixels, direct.pixels);
        // Forwarded colours actually took effect (red cell at origin).
        assert_eq!(seq.frames[0].get(0, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn scroll_grating_base_matches_direct_grating_render() {
        let q = map(&[
            ("pattern", "grating"),
            ("w", "16"),
            ("h", "8"),
            ("freq", "2"),
            ("angle", "0"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]);
        let seq = render(&q).unwrap();
        let direct = grating::render(&q).unwrap();
        assert_eq!(seq.frames[0].pixels, direct.pixels);
    }

    /// vy-only scroll: row 0 of frame 1 is the base frame's bottom row
    /// (content moved down by one pixel, the seam wrapped around).
    #[test]
    fn scroll_vertical_only_shifts_rows_down() {
        let seq = render(&map(&[
            ("pattern", "plasma"),
            ("w", "8"),
            ("h", "6"),
            ("vx", "0"),
            ("vy", "1"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("seed", "9"),
        ]))
        .unwrap();
        let base = &seq.frames[0];
        let f1 = &seq.frames[1];
        for x in 0..8 {
            assert_eq!(f1.get(x, 0), base.get(x, 5), "wrapped seam row");
            assert_eq!(f1.get(x, 3), base.get(x, 2), "interior row");
        }
    }

    #[test]
    fn scroll_frame_count_follows_fps() {
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("duration", "0.7"),
            ("fps", "20"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 14);
        assert_eq!(seq.fps, 20);
    }

    #[test]
    fn scroll_unknown_pattern_errors() {
        let res = render(&map(&[("pattern", "lava"), ("w", "8"), ("h", "8")]));
        let err = match res {
            Ok(_) => panic!("expected error for pattern=lava"),
            Err(e) => e,
        };
        assert!(format!("{err}").contains("lava"));
    }

    #[test]
    fn scroll_non_integer_velocity_errors() {
        let res = render(&map(&[("vx", "1.5"), ("w", "8"), ("h", "8")]));
        assert!(res.is_err(), "fractional vx must be rejected");
    }
}

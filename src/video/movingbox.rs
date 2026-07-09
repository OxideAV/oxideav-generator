//! Moving-box motion probe — a solid foreground rectangle translating
//! at an exactly-known integer velocity over a solid background.
//!
//! Frame `f` is defined pixel-for-pixel by the closed form
//!
//! ```text
//! frame_f(x, y) = fg   if (x − x0 − f·vx) mod w < bw
//!                      and (y − y0 − f·vy) mod h < bh
//!                 bg   otherwise
//! ```
//!
//! (`mod` is the euclidean remainder, so negative velocities and
//! origins work.) `vx` / `vy` are signed integer pixels-per-frame, so
//! the true motion vector of the (only) moving object is *globally
//! constant and known exactly*: frame `f + 1` is frame `f` with the
//! box displaced by `(vx, vy)`, wrapping toroidally at the frame
//! edges. No resampling, no interpolation, no sub-pixel phase — every
//! frame contains exactly `bw · bh` foreground pixels.
//!
//! Where [`scroll`](super::scroll) probes *global* motion (every pixel
//! moves), `movingbox` probes *local* motion: a single small object
//! moves over a static background, which is precisely the case block
//! motion search has to isolate. A motion estimator run on the pair
//! `(f, f + 1)` should produce `(vx, vy)` for blocks covering the box,
//! `(0, 0)` for pure-background blocks, and residual-free prediction
//! everywhere except the covered/uncovered strips the box sweeps.

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::FrameSeq;
use crate::image::palette::parse_color;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_i32, q_str, q_u32};

/// Render a moving-box sequence.
///
/// Recognised query parameters:
///
/// | Key         | Default | Meaning                                       |
/// |-------------|---------|-----------------------------------------------|
/// | `w` / `h`   | 320/240 | Output resolution in pixels                   |
/// | `duration`  | 5       | Seconds                                       |
/// | `fps`       | 30      | Frames per second                             |
/// | `bw` / `bh` | 32/32   | Box size in pixels (clamped to frame size)    |
/// | `x0` / `y0` | 0/0     | Box origin (top-left) in frame 0, signed      |
/// | `vx` / `vy` | 1/0     | Signed velocity, pixels per frame             |
/// | `fg`        | white   | Box colour (named / `#RGB` / `#RRGGBB[AA]`)   |
/// | `bg`        | black   | Background colour                             |
pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 320)?.max(1);
    let h = q_u32(query, "h", 240)?.max(1);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let bw = q_u32(query, "bw", 32)?.clamp(1, w);
    let bh = q_u32(query, "bh", 32)?.clamp(1, h);
    let x0 = q_i32(query, "x0", 0)? as i64;
    let y0 = q_i32(query, "y0", 0)? as i64;
    let vx = q_i32(query, "vx", 1)? as i64;
    let vy = q_i32(query, "vy", 0)? as i64;
    let fg = parse_color(q_str(query, "fg", "white"))?;
    let bg = parse_color(q_str(query, "bg", "black"))?;

    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);

    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count as i64 {
        // Box origin in this frame, wrapped onto the torus Z_w × Z_h.
        let bx = (x0 + f * vx).rem_euclid(w as i64) as u32;
        let by = (y0 + f * vy).rem_euclid(h as i64) as u32;

        let mut img = Rgba8Image::new(w, h);
        // Background fill.
        for chunk in img.pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&bg);
        }
        // Foreground box, wrapped per axis. Exactly bw·bh pixels.
        for dy in 0..bh {
            let y = (by + dy) % h;
            for dx in 0..bw {
                let x = (bx + dx) % w;
                img.put(x, y, fg);
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

    const FG: [u8; 4] = [255, 255, 255, 255];
    const BG: [u8; 4] = [0, 0, 0, 255];

    /// The module-doc closed form, recomputed independently.
    fn closed_form(
        x: i64,
        y: i64,
        f: i64,
        w: i64,
        h: i64,
        bw: i64,
        bh: i64,
        x0: i64,
        y0: i64,
        vx: i64,
        vy: i64,
    ) -> bool {
        (x - x0 - f * vx).rem_euclid(w) < bw && (y - y0 - f * vy).rem_euclid(h) < bh
    }

    #[test]
    fn movingbox_matches_closed_form_every_pixel() {
        let (w, h, bw, bh, x0, y0, vx, vy) = (16i64, 12i64, 5i64, 3i64, 2i64, 1i64, 3i64, -2i64);
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "12"),
            ("bw", "5"),
            ("bh", "3"),
            ("x0", "2"),
            ("y0", "1"),
            ("vx", "3"),
            ("vy", "-2"),
            ("duration", "0.5"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 5);
        for (f, img) in seq.frames.iter().enumerate() {
            for y in 0..h {
                for x in 0..w {
                    let want = if closed_form(x, y, f as i64, w, h, bw, bh, x0, y0, vx, vy) {
                        FG
                    } else {
                        BG
                    };
                    assert_eq!(
                        img.get(x as u32, y as u32),
                        want,
                        "frame {f}, pixel ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn movingbox_every_frame_has_exactly_bw_bh_foreground_pixels() {
        // Even when the box wraps across both edges.
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("bw", "3"),
            ("bh", "3"),
            ("x0", "6"),
            ("y0", "6"),
            ("vx", "1"),
            ("vy", "1"),
            ("duration", "0.8"),
            ("fps", "10"),
        ]))
        .unwrap();
        for (f, img) in seq.frames.iter().enumerate() {
            let fg_count = img.pixels.chunks_exact(4).filter(|p| *p == &FG[..]).count();
            assert_eq!(fg_count, 9, "frame {f} must have exactly bw·bh fg pixels");
        }
    }

    #[test]
    fn movingbox_next_frame_is_previous_displaced_by_velocity() {
        // frame_{f+1}(x, y) == frame_f((x − vx) mod w, (y − vy) mod h):
        // the sequence is residual-free under its own ground-truth
        // motion vector.
        let (w, h, vx, vy) = (16i64, 12i64, 3i64, -2i64);
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "12"),
            ("bw", "4"),
            ("bh", "4"),
            ("vx", "3"),
            ("vy", "-2"),
            ("duration", "0.4"),
            ("fps", "10"),
        ]))
        .unwrap();
        for f in 0..seq.frames.len() - 1 {
            let (cur, next) = (&seq.frames[f], &seq.frames[f + 1]);
            for y in 0..h {
                for x in 0..w {
                    let sx = (x - vx).rem_euclid(w) as u32;
                    let sy = (y - vy).rem_euclid(h) as u32;
                    assert_eq!(
                        next.get(x as u32, y as u32),
                        cur.get(sx, sy),
                        "frame {f}→{}, pixel ({x}, {y})",
                        f + 1
                    );
                }
            }
        }
    }

    #[test]
    fn movingbox_zero_velocity_is_static() {
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("vx", "0"),
            ("vy", "0"),
            ("duration", "0.3"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert!(seq.frames.len() >= 2);
        let first = &seq.frames[0].pixels;
        for f in &seq.frames[1..] {
            assert_eq!(&f.pixels, first, "vx=vy=0 must be frame-identical");
        }
    }

    #[test]
    fn movingbox_custom_colors() {
        let seq = render(&map(&[
            ("w", "4"),
            ("h", "4"),
            ("bw", "1"),
            ("bh", "1"),
            ("fg", "red"),
            ("bg", "blue"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert_eq!(seq.frames[0].get(0, 0), [255, 0, 0, 255]);
        assert_eq!(seq.frames[0].get(1, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn movingbox_box_size_clamped_to_frame() {
        // bw/bh larger than the frame degenerate to an all-fg frame.
        let seq = render(&map(&[
            ("w", "4"),
            ("h", "4"),
            ("bw", "99"),
            ("bh", "99"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        for p in seq.frames[0].pixels.chunks_exact(4) {
            assert_eq!(p, &FG[..]);
        }
    }

    #[test]
    fn movingbox_unknown_color_errors() {
        assert!(render(&map(&[("fg", "not-a-color")])).is_err());
    }

    #[test]
    fn movingbox_is_deterministic() {
        let args = &[
            ("w", "12"),
            ("h", "10"),
            ("vx", "2"),
            ("vy", "1"),
            ("duration", "0.3"),
            ("fps", "10"),
        ];
        let a = render(&map(args)).unwrap();
        let b = render(&map(args)).unwrap();
        for (fa, fb) in a.frames.iter().zip(b.frames.iter()) {
            assert_eq!(fa.pixels, fb.pixels);
        }
    }
}

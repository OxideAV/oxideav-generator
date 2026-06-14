//! Rotating colour-wheel test pattern — hue mapped to polar angle,
//! saturation to normalised radius, rendered as an HSL image.
//!
//! For each pixel we take the vector from the frame centre, `(dx, dy)`,
//! and resolve it into polar coordinates:
//!
//! - angle `θ = atan2(dy, dx)` (radians, in `[-π, π]`), converted to a
//!   degree hue in `[0, 360)`;
//! - radius `r = sqrt(dx² + dy²)`, normalised by `r_max` (half the
//!   smaller frame dimension) and clamped to `[0, 1]`.
//!
//! The hue is `(θ_deg + spin · t) mod 360`, so the wheel rotates at
//! `spin` degrees per second across the sequence (`t` is the frame's
//! presentation time in seconds). Saturation rises linearly with the
//! clamped normalised radius — the centre is achromatic (white at the
//! default lightness 0.5) and the rim is fully saturated. Lightness is
//! a fixed parameter.
//!
//! Because hue is a continuous function of angle and the rotation is a
//! pure additive phase offset, the wheel is a smooth angular-motion
//! probe: every frame is a rigid rotation of the previous one about the
//! centre, with an analytically known offset. It also sweeps the full
//! hue circle in a single frame, exercising the chroma path the way the
//! zone plate exercises the luma frequency path.

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::FrameSeq;
use crate::image::palette::hsl_to_rgb;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_u32};

/// Render a rotating colour-wheel sequence.
///
/// Recognised query parameters:
///
/// | Key          | Default | Meaning                                            |
/// |--------------|---------|----------------------------------------------------|
/// | `w` / `h`    | 320/240 | Output resolution in pixels                        |
/// | `duration`   | 5       | Seconds                                            |
/// | `fps`        | 30      | Frames per second                                  |
/// | `spin`       | 60      | Rotation rate in degrees per second (sign = dir)   |
/// | `lightness`  | 0.5     | HSL lightness for every pixel (0…1)               |
/// | `saturation` | 1.0     | Saturation at the rim; scales the radial ramp      |
pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 320)?.max(1);
    let h = q_u32(query, "h", 240)?.max(1);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let spin = q_f64(query, "spin", 60.0)? as f32; // degrees per second
    let lightness = q_f64(query, "lightness", 0.5)?.clamp(0.0, 1.0) as f32;
    let rim_sat = q_f64(query, "saturation", 1.0)?.clamp(0.0, 1.0) as f32;

    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);

    // Geometric centre and the radius that reaches the nearest edge.
    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;
    let r_max = ((w.min(h) as f32) * 0.5).max(1.0);

    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        // Presentation time of this frame, in seconds.
        let t = f as f32 / fps as f32;
        let phase = spin * t; // degrees

        let mut img = Rgba8Image::new(w, h);
        for y in 0..h {
            let dy = y as f32 - cy;
            for x in 0..w {
                let dx = x as f32 - cx;

                // Angle → hue. atan2 yields [-π, π]; shift into [0, 360).
                let theta_deg = dy.atan2(dx).to_degrees();
                let hue = (theta_deg + phase).rem_euclid(360.0);

                // Radius → saturation, clamped to the unit disc.
                let r = (dx * dx + dy * dy).sqrt();
                let sat = (r / r_max).clamp(0.0, 1.0) * rim_sat;

                let (r8, g8, b8) = hsl_to_rgb(hue / 360.0, sat, lightness);
                img.put(x, y, [r8, g8, b8, 255]);
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

    #[test]
    fn colorwheel_frame_count_follows_fps() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.5"),
            ("fps", "20"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 10);
        assert_eq!(seq.fps, 20);
    }

    #[test]
    fn colorwheel_centre_is_achromatic() {
        // At r = 0 saturation is 0, so the centre is a pure grey set by
        // lightness. With an odd size the centre is an integer pixel.
        // lightness 0.5 → mid grey 128 on every channel.
        let seq = render(&map(&[
            ("w", "9"),
            ("h", "9"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        let [r, g, b, a] = seq.frames[0].get(4, 4);
        assert_eq!(a, 255);
        assert_eq!(r, g, "centre must be achromatic (r==g)");
        assert_eq!(g, b, "centre must be achromatic (g==b)");
        // Mid lightness, zero saturation → 0.5 * 255 = 127.5, truncated
        // to 127 by the shared HSL→RGB converter.
        assert_eq!(r, 127);
    }

    #[test]
    fn colorwheel_lightness_zero_is_black_everywhere() {
        // Lightness 0.0 collapses HSL to black regardless of hue/sat.
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("duration", "0.1"),
            ("fps", "10"),
            ("lightness", "0.0"),
        ]))
        .unwrap();
        for y in 0..8u32 {
            for x in 0..8u32 {
                assert_eq!(seq.frames[0].get(x, y), [0, 0, 0, 255]);
            }
        }
    }

    #[test]
    fn colorwheel_zero_saturation_is_grey_everywhere() {
        // saturation=0 scales the radial ramp to zero → every pixel grey.
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("duration", "0.1"),
            ("fps", "10"),
            ("saturation", "0"),
        ]))
        .unwrap();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let [r, g, b, _] = seq.frames[0].get(x, y);
                assert_eq!(r, g);
                assert_eq!(g, b);
            }
        }
    }

    #[test]
    fn colorwheel_opposite_angles_have_distinct_hues() {
        // Two pixels on opposite sides of the centre sit 180° apart on
        // the hue circle, so they must render to different colours
        // (both are off-centre, hence saturated).
        let seq = render(&map(&[
            ("w", "9"),
            ("h", "9"),
            ("duration", "0.1"),
            ("fps", "10"),
        ]))
        .unwrap();
        let left = seq.frames[0].get(0, 4);
        let right = seq.frames[0].get(8, 4);
        assert_ne!(left, right);
    }

    #[test]
    fn colorwheel_static_when_spin_zero() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("spin", "0"),
        ]))
        .unwrap();
        assert!(seq.frames.len() >= 2);
        let first = &seq.frames[0].pixels;
        for f in &seq.frames[1..] {
            assert_eq!(&f.pixels, first, "spin=0 must be frame-identical");
        }
    }

    #[test]
    fn colorwheel_rotates_when_spinning() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.3"),
            ("fps", "10"),
            ("spin", "120"),
        ]))
        .unwrap();
        assert!(seq.frames.len() >= 2);
        assert_ne!(
            seq.frames[0].pixels,
            seq.frames.last().unwrap().pixels,
            "spinning wheel must change between frames"
        );
    }

    #[test]
    fn colorwheel_is_deterministic() {
        let args = &[
            ("w", "24"),
            ("h", "20"),
            ("duration", "0.2"),
            ("fps", "15"),
            ("spin", "90"),
        ];
        let a = render(&map(args)).unwrap();
        let b = render(&map(args)).unwrap();
        assert_eq!(a.frames.len(), b.frames.len());
        for (fa, fb) in a.frames.iter().zip(b.frames.iter()) {
            assert_eq!(fa.pixels, fb.pixels, "render must be deterministic");
        }
    }
}

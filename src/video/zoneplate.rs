//! Zone-plate test pattern — a radial chirp `cos(k * r²)` rendered as
//! a luma image.
//!
//! The zone plate is a classic spatial-frequency test pattern: the
//! local spatial frequency rises linearly with distance from the
//! centre, so the pattern simultaneously exercises every spatial
//! frequency the renderer supports. Aliasing, ringing, and
//! interpolation artefacts all show up as moiré rings; a perfect
//! sampler produces the analytically-expected fringe count.
//!
//! Optional `motion=…` modes ("none", "temporal", "horizontal",
//! "vertical") modulate the chirp parameter (or shift the centre)
//! across the frame index so the pattern animates without changing
//! its overall structure — useful for codec motion-search probes.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::FrameSeq;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_str, q_u32};

/// Render a zone-plate sequence.
///
/// Recognised query parameters:
///
/// | Key         | Default  | Meaning                                              |
/// |-------------|----------|------------------------------------------------------|
/// | `w` / `h`   | 640/480  | Output resolution in pixels                          |
/// | `duration`  | 5        | Seconds                                              |
/// | `fps`       | 30       | Frames per second                                    |
/// | `k`         | 0.05     | Radial chirp rate (cos(k * r²))                      |
/// | `motion`    | `none`   | `none` / `temporal` / `horizontal` / `vertical`      |
/// | `amplitude` | 1.0      | Output luma scale (0…1)                              |
pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 640)?.max(2);
    let h = q_u32(query, "h", 480)?.max(2);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);
    let k = q_f64(query, "k", 0.05)?.max(0.0) as f32;
    let amplitude = q_f64(query, "amplitude", 1.0)?.clamp(0.0, 1.0) as f32;
    let motion = q_str(query, "motion", "none").to_string();

    let cx = (w as f32 - 1.0) * 0.5;
    let cy = (h as f32 - 1.0) * 0.5;

    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let t = f as f32 / frame_count.max(1) as f32;
        // Motion modes: shift the centre or scale the chirp.
        let (cx_t, cy_t, k_t) = match motion.as_str() {
            "none" | "" => (cx, cy, k),
            "temporal" => (cx, cy, k * (1.0 + 0.5 * t)),
            "horizontal" => (cx + (w as f32 * 0.25) * (t - 0.5), cy, k),
            "vertical" => (cx, cy + (h as f32 * 0.25) * (t - 0.5), k),
            other => {
                return Err(Error::invalid(format!(
                    "zoneplate: motion {other:?} (expected none|temporal|horizontal|vertical)"
                )));
            }
        };

        let mut img = Rgba8Image::new(w, h);
        for y in 0..h {
            let dy = y as f32 - cy_t;
            for x in 0..w {
                let dx = x as f32 - cx_t;
                let r2 = dx * dx + dy * dy;
                // cos in [-1, 1] → map to [0, 255] luma.
                let v = ((k_t * r2).cos() * amplitude + 1.0) * 0.5;
                let byte = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                img.put(x, y, [byte, byte, byte, 255]);
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
    fn zoneplate_centre_pixel_is_peak_luma() {
        // At r = 0, cos(0) = 1, so the centre is the brightest pixel
        // (255 at amplitude=1). For an odd-sized image the centre is
        // an integer pixel; we pick 9×9 so the centre is (4, 4).
        let seq = render(&map(&[
            ("w", "9"),
            ("h", "9"),
            ("duration", "0.1"),
            ("fps", "10"),
            ("k", "0.05"),
        ]))
        .unwrap();
        assert_eq!(seq.frames[0].get(4, 4), [255, 255, 255, 255]);
    }

    #[test]
    fn zoneplate_frame_count_follows_fps() {
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
    fn zoneplate_static_motion_emits_identical_frames() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("motion", "none"),
        ]))
        .unwrap();
        assert!(seq.frames.len() >= 2);
        let first = &seq.frames[0].pixels;
        for f in &seq.frames[1..] {
            assert_eq!(&f.pixels, first, "static motion must be frame-identical");
        }
    }

    #[test]
    fn zoneplate_temporal_motion_changes_frames() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("motion", "temporal"),
        ]))
        .unwrap();
        assert!(seq.frames.len() >= 2);
        // First and last frames should differ — temporal motion scales
        // the chirp rate across the sequence.
        let first = &seq.frames[0].pixels;
        let last = &seq.frames.last().unwrap().pixels;
        assert_ne!(first, last);
    }

    #[test]
    fn zoneplate_horizontal_motion_changes_frames() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "16"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("motion", "horizontal"),
        ]))
        .unwrap();
        // First and last shift the centre laterally → pixel content
        // differs.
        assert_ne!(seq.frames[0].pixels, seq.frames.last().unwrap().pixels);
    }

    #[test]
    fn zoneplate_unknown_motion_errors() {
        let res = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("duration", "0.1"),
            ("fps", "10"),
            ("motion", "spiral"),
        ]));
        let err = match res {
            Ok(_) => panic!("expected error for motion=spiral"),
            Err(e) => e,
        };
        assert!(format!("{err}").contains("spiral"));
    }

    #[test]
    fn zoneplate_amplitude_zero_is_mid_grey() {
        // amplitude=0 collapses the cos term; v = (0 + 1) * 0.5 = 0.5
        // → byte 128 (rounded).
        let seq = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("duration", "0.1"),
            ("fps", "10"),
            ("amplitude", "0"),
        ]))
        .unwrap();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let px = seq.frames[0].get(x, y);
                assert_eq!(px, [128, 128, 128, 255]);
            }
        }
    }
}

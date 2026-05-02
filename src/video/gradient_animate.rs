//! Hue-rotating gradient — emit a horizontal H-rotated gradient
//! per frame, with the rotation rate controlled by `hue_rate`.

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::FrameSeq;
use crate::image::palette::hsl_to_rgb;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_u32};

pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 320)?.max(1);
    let h = q_u32(query, "h", 240)?.max(1);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let hue_rate = q_f64(query, "hue_rate", 30.0)? as f32; // degrees per second
    let saturation = q_f64(query, "saturation", 0.6)? as f32;
    let lightness = q_f64(query, "lightness", 0.5)? as f32;

    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let mut img = Rgba8Image::new(w, h);
        let base_hue = (f as f32 / fps as f32) * hue_rate; // degrees
        for y in 0..h {
            for x in 0..w {
                let pos = if w > 1 {
                    x as f32 / (w - 1) as f32
                } else {
                    0.0
                };
                let h_deg = (base_hue + pos * 360.0) % 360.0;
                let (r, g, b) = hsl_to_rgb(h_deg / 360.0, saturation, lightness);
                img.put(x, y, [r, g, b, 255]);
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
    fn gradient_animate_rotates_hue_over_time() {
        let seq = render(&map(&[
            ("w", "32"),
            ("h", "8"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("hue_rate", "180"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 2);
        // First and last frame should differ at any pixel position.
        assert_ne!(seq.frames[0].get(5, 4), seq.frames[1].get(5, 4));
    }
}

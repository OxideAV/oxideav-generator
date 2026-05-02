//! ffmpeg-style `testsrc`: vertical SMPTE-ish colour bars + a moving
//! horizontal time bar + a per-frame frame counter shown as a square
//! marker.
//!
//! The exact pixel layout doesn't have to match ffmpeg bit-for-bit
//! (the plan calls for *structural* parity — frame count, dims,
//! pixel format) — what matters is that the stream is recognisable
//! as a test pattern and changes per frame.

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::FrameSeq;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_u32};

const BAR_COLORS: &[[u8; 4]] = &[
    [255, 255, 255, 255], // white
    [255, 255, 0, 255],   // yellow
    [0, 255, 255, 255],   // cyan
    [0, 255, 0, 255],     // green
    [255, 0, 255, 255],   // magenta
    [255, 0, 0, 255],     // red
    [0, 0, 255, 255],     // blue
    [0, 0, 0, 255],       // black
];

pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 640)?.max(2);
    let h = q_u32(query, "h", 480)?.max(2);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);

    let mut frames = Vec::with_capacity(frame_count);
    let bar_width = w / BAR_COLORS.len() as u32;
    for f in 0..frame_count {
        let mut img = Rgba8Image::new(w, h);
        // Vertical colour bars.
        for y in 0..h {
            for x in 0..w {
                let bar = ((x / bar_width.max(1)) as usize).min(BAR_COLORS.len() - 1);
                img.put(x, y, BAR_COLORS[bar]);
            }
        }
        // Moving horizontal scan bar.
        let bar_y = ((f as u32) * 2) % h;
        for x in 0..w {
            for dy in 0..(h / 80).max(1) {
                let y = (bar_y + dy).min(h - 1);
                img.put(x, y, [255, 255, 255, 255]);
            }
        }
        // Frame-counter square in the upper-left corner: 16×16 box
        // whose colour cycles through the bar palette.
        let marker = BAR_COLORS[f % BAR_COLORS.len()];
        for y in 0..16.min(h) {
            for x in 0..16.min(w) {
                img.put(x, y, marker);
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
    fn frame_count_matches_duration_times_fps() {
        let seq = render(&map(&[
            ("w", "64"),
            ("h", "48"),
            ("duration", "1"),
            ("fps", "10"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 10);
    }

    #[test]
    fn each_frame_has_correct_dims() {
        let seq = render(&map(&[
            ("w", "32"),
            ("h", "16"),
            ("duration", "0.1"),
            ("fps", "30"),
        ]))
        .unwrap();
        for f in &seq.frames {
            assert_eq!(f.width, 32);
            assert_eq!(f.height, 16);
        }
    }
}

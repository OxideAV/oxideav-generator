//! SMPTE 75% colour bars + a thin pluge / sub-black row at the bottom.
//!
//! Static frame replicated for the requested duration — SMPTE bars
//! don't move; the FPS just controls how many times the same image
//! is emitted.

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::FrameSeq;
use crate::image::Rgba8Image;
use crate::source::{q_f64, q_u32};

// 75% SMPTE bars: gray, yellow, cyan, green, magenta, red, blue.
const BARS_75: &[[u8; 4]] = &[
    [191, 191, 191, 255],
    [191, 191, 0, 255],
    [0, 191, 191, 255],
    [0, 191, 0, 255],
    [191, 0, 191, 255],
    [191, 0, 0, 255],
    [0, 0, 191, 255],
];

// Sub-bar pluge stripe (75%).
const PLUGE: &[[u8; 4]] = &[
    [0, 0, 191, 255],
    [255, 255, 255, 255],
    [191, 0, 191, 255],
    [0, 0, 0, 255],
];

pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 640)?.max(2);
    let h = q_u32(query, "h", 480)?.max(2);
    let duration_s = q_f64(query, "duration", 5.0)?.max(0.0);
    let fps = q_u32(query, "fps", 30)?.max(1);
    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);

    let bar_w = (w / BARS_75.len() as u32).max(1);
    let pluge_h = (h / 6).max(1);
    let main_h = h - pluge_h;

    let mut img = Rgba8Image::new(w, h);
    for y in 0..main_h {
        for x in 0..w {
            let bar = ((x / bar_w) as usize).min(BARS_75.len() - 1);
            img.put(x, y, BARS_75[bar]);
        }
    }
    let pluge_w = (w / PLUGE.len() as u32).max(1);
    for y in main_h..h {
        for x in 0..w {
            let bar = ((x / pluge_w) as usize).min(PLUGE.len() - 1);
            img.put(x, y, PLUGE[bar]);
        }
    }

    let frames = (0..frame_count).map(|_| img.clone()).collect();
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
    fn smptebars_one_frame_per_fps_unit() {
        let seq = render(&map(&[
            ("w", "70"),
            ("h", "48"),
            ("duration", "0.5"),
            ("fps", "20"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 10);
    }

    #[test]
    fn first_bar_is_75_percent_gray() {
        let seq = render(&map(&[
            ("w", "70"),
            ("h", "60"),
            ("duration", "0.04"),
            ("fps", "25"),
        ]))
        .unwrap();
        assert_eq!(seq.frames[0].get(2, 2), [191, 191, 191, 255]);
    }
}

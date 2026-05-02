//! Structural parity with ffmpeg's `lavfi` test sources.
//!
//! We don't bit-compare against ffmpeg (different generator
//! implementations). The plan only asks for *structural* parity —
//! frame count, dimensions, pixel format. These tests pin those
//! invariants so a regression in our generator code is caught
//! without depending on ffmpeg being installed.

use std::collections::BTreeMap;

use oxideav_generator::video::{smptebars, testsrc};

fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn testsrc_320x240_30fps_5s_has_150_frames() {
    let seq = testsrc::render(&map(&[
        ("w", "320"),
        ("h", "240"),
        ("fps", "30"),
        ("duration", "5"),
    ]))
    .unwrap();
    assert_eq!(seq.frames.len(), 150);
    assert_eq!(seq.fps, 30);
    for f in &seq.frames {
        assert_eq!(f.width, 320);
        assert_eq!(f.height, 240);
        // RGBA8 = 4 bytes per pixel.
        assert_eq!(f.pixels.len(), 320 * 240 * 4);
    }
}

#[test]
fn smptebars_640x480_24fps_2s_has_48_frames() {
    let seq = smptebars::render(&map(&[
        ("w", "640"),
        ("h", "480"),
        ("fps", "24"),
        ("duration", "2"),
    ]))
    .unwrap();
    assert_eq!(seq.frames.len(), 48);
    assert_eq!(seq.fps, 24);
    for f in &seq.frames {
        assert_eq!(f.width, 640);
        assert_eq!(f.height, 480);
    }
}

#[test]
fn testsrc_default_duration_uses_plan_default() {
    // No explicit duration → 5s default × 30fps default = 150 frames at 640×480.
    let seq = testsrc::render(&BTreeMap::new()).unwrap();
    assert_eq!(seq.frames.len(), 150);
    assert_eq!(seq.frames[0].width, 640);
    assert_eq!(seq.frames[0].height, 480);
}

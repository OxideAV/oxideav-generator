//! Video generators (testsrc / smptebars / fractal_zoom /
//! gradient_animate).
//!
//! Each generator emits a precomputed [`FrameSeq`] of RGBA8 frames
//! with a known `(width, height, fps, duration_s)`. Both the URI
//! [`FrameSource`](oxideav_core::FrameSource) wrapper and the
//! zero-input filter wrapper turn these into
//! [`VideoFrame`](oxideav_core::VideoFrame) values for the pipeline —
//! no container layer (Y4M / PNG / etc.) is involved on the source
//! path.

pub mod fractal_zoom;
pub mod gradient_animate;
pub mod smptebars;
pub mod testsrc;

use crate::image::Rgba8Image;

/// A finite stream of generated frames.
pub struct FrameSeq {
    pub frames: Vec<Rgba8Image>,
    pub fps: u32,
}

impl FrameSeq {
    pub fn duration_s(&self) -> f32 {
        if self.fps == 0 {
            0.0
        } else {
            self.frames.len() as f32 / self.fps as f32
        }
    }
}

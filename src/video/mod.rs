//! Video generators (testsrc / smptebars / fractal_zoom /
//! gradient_animate).
//!
//! Each generator emits an iterator of RGBA8 frames with a known
//! `(width, height, fps, duration_s)`. The filter-side wrapper turns
//! these into [`VideoFrame`](oxideav_core::VideoFrame) values for the
//! pipeline; the URI source side is wired into Y4M but currently
//! returns `Unsupported` because no Y4M demuxer is in tree.

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

//! Image generators (xc / gradient / pattern / fractal / plasma /
//! noise) plus a tiny in-tree PNG encoder.
//!
//! Each generator returns RGBA8 row-major bytes via [`Rgba8Image`];
//! [`png_encode`] turns that into a standalone PNG byte stream.

pub mod fractal;
pub mod gradient;
pub mod noise;
pub mod palette;
pub mod pattern;
pub mod plasma;
pub mod png;
pub mod xc;

/// Row-major RGBA8 image — 4 bytes per pixel, `width * height * 4`
/// total. Matches `PixelFormat::Rgba`.
#[derive(Clone, Debug)]
pub struct Rgba8Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba8Image {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    #[inline]
    pub fn put(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.pixels[idx..idx + 4].copy_from_slice(&rgba);
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let s = &self.pixels[idx..idx + 4];
        [s[0], s[1], s[2], s[3]]
    }
}

pub use png::encode as png_encode;

//! Image generators (xc / gradient / pattern / fractal / plasma /
//! noise / ramp / hald).
//!
//! Each generator has a `render(query)` returning RGBA8 row-major
//! bytes via [`Rgba8Image`]; [`render_still`] runs any of them by name
//! and delivers the result in the sample depth and channel layout the
//! query asks for (see [`canvas`]). Frames flow straight to the
//! pipeline as
//! [`SourceOutput::Frames`](oxideav_core::SourceOutput::Frames); no
//! container layer (PNG / Y4M / WAV) is involved on the source path.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

pub mod canvas;
pub mod chart;
mod detmath;
pub mod fractal;
pub mod gradient;
pub mod grating;
pub mod hald;
#[cfg(feature = "label")]
pub mod label;
pub mod noise;
pub mod palette;
pub mod pattern;
pub mod plasma;
pub mod ramp;
pub mod random;
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

/// Render the still-image generator `kind` (`xc`, `gradient`,
/// `radial-gradient`, `conic-gradient`, `pattern`, `grating`, `ramp`,
/// `hald`, `label`, `fractal`, `plasma`, `noise`, `random`, `chart`) from a
/// `generate://`
/// query. Returns `None` when `kind` is not a still-image generator.
///
/// The output format comes from the query's `depth=8|16|float`,
/// `channels=gray|graya|rgb|rgba`, `dither=none|ordered|noise` and
/// `threads=N` keys ([`canvas::RenderConfig::from_query`]); the default
/// is 8-bit RGBA. Gradients are evaluated at full precision in every
/// depth; generators that are defined on 8-bit levels are widened.
pub fn render_still(kind: &str, query: &BTreeMap<String, String>) -> Option<Result<canvas::Image>> {
    let widen = |img: Result<Rgba8Image>| -> Result<canvas::Image> {
        let cfg = canvas::RenderConfig::from_query(query)?;
        Ok(canvas::convert_rgba8(img?, &cfg))
    };
    let typed_gradient = |kind: &str| {
        let mut q = query.clone();
        q.insert("type".to_string(), kind.to_string());
        gradient::render_image(&q)
    };
    Some(match kind {
        "xc" => xc::render_image(query),
        "gradient" => gradient::render_image(query),
        "radial-gradient" | "radial_gradient" | "radial" => typed_gradient("radial"),
        "conic-gradient" | "conic_gradient" | "conic" => typed_gradient("conic"),
        "pattern" => pattern::render_image(query),
        "grating" => widen(grating::render(query)),
        "ramp" => widen(ramp::render(query)),
        "hald" => widen(hald::render(query)),
        #[cfg(feature = "label")]
        "label" => widen(label::render(query)),
        #[cfg(not(feature = "label"))]
        "label" => Err(Error::Unsupported(
            "generate://label: oxideav-generator was built without the `label` feature".into(),
        )),
        "fractal" => widen(fractal::render(query)),
        "plasma" => plasma::render_image(query),
        "noise" => noise::render_image(query),
        "random" => random::render_image(query),
        "chart" => chart::render_image(query),
        _ => return None,
    })
}

/// The framework pixel format of a rendered still, or an error for
/// float samples (which have no framework pixel format yet).
pub(crate) fn frame_format(img: &canvas::Image) -> Result<oxideav_core::PixelFormat> {
    img.pixel_format().ok_or_else(|| {
        Error::Unsupported(
            "float samples have no framework pixel format; request depth=8 or depth=16 here, \
             or call the generator API directly for float output"
                .into(),
        )
    })
}

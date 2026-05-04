//! `label:` text-to-image generator.
//!
//! Renders a string to an RGBA8 canvas using `oxideav-scribe` for
//! shaping + rasterising. Mirrors ImageMagick's `label:Hello world`
//! source: produces a still image sized to the rendered text plus
//! padding (or to an explicit `w=… h=…` if the caller provides them).
//!
//! Default font is the bundled DejaVu Sans Mono (~340 KB). Pass
//! `font=/path/to/your.ttf` to override.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};
use oxideav_scribe::{render_text, Face, Rgba};

use super::palette::parse_color;
use super::Rgba8Image;
use crate::source::{q_f64, q_str, q_u32};

/// Bundled fallback font. DejaVu Sans Mono 2.37 — Bitstream Vera
/// derivative under a permissive license (see assets/DEJAVU-LICENSE).
const DEFAULT_FONT: &[u8] = include_bytes!("../../assets/DejaVuSansMono.ttf");

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let text = q_str(query, "text", "");
    let size_px = q_f64(query, "size", 24.0)? as f32;
    if !(size_px.is_finite() && size_px > 0.0) {
        return Err(Error::invalid(format!(
            "label: size must be a positive finite number, got {size_px}"
        )));
    }
    let color: Rgba = parse_color(q_str(query, "color", "black"))?;
    let bg: Rgba = parse_color(q_str(query, "bg", "white"))?;
    let padding = q_u32(query, "padding", 4)?;
    let explicit_w = query.get("w").map(|s| s.parse::<u32>()).transpose().map_err(|_| {
        Error::invalid("label: w must be a non-negative integer".to_string())
    })?;
    let explicit_h = query.get("h").map(|s| s.parse::<u32>()).transpose().map_err(|_| {
        Error::invalid("label: h must be a non-negative integer".to_string())
    })?;

    let face = load_face(query.get("font").map(|s| s.as_str()))?;

    // Shape + raster the text into a tight RgbaBitmap. For empty input
    // we still produce a canvas so callers always get a valid frame.
    let glyph_bmp = render_text(&face, text, size_px, color)
        .map_err(|e| Error::invalid(format!("label: scribe render failed: {e:?}")))?;

    let glyph_w = glyph_bmp.width;
    let glyph_h = glyph_bmp.height;

    let canvas_w = explicit_w
        .unwrap_or_else(|| glyph_w.saturating_add(padding.saturating_mul(2)).max(1));
    let canvas_h = explicit_h
        .unwrap_or_else(|| glyph_h.saturating_add(padding.saturating_mul(2)).max(1));

    let mut img = Rgba8Image::new(canvas_w, canvas_h);

    // Fill background.
    for y in 0..canvas_h {
        for x in 0..canvas_w {
            img.put(x, y, bg);
        }
    }

    if glyph_w == 0 || glyph_h == 0 {
        return Ok(img);
    }

    // Centre the glyph bitmap inside the canvas. Both axes are
    // independently centred — yields a sensible placement whether the
    // caller used auto-fit (== padding on each side) or an explicit
    // canvas larger than the text.
    let off_x = canvas_w.saturating_sub(glyph_w) / 2;
    let off_y = canvas_h.saturating_sub(glyph_h) / 2;

    blit_straight_alpha(&mut img, off_x, off_y, &glyph_bmp.data, glyph_w, glyph_h);

    Ok(img)
}

/// Compose `src` (straight-alpha RGBA, row-major, width*4 stride) over
/// the destination canvas at `(off_x, off_y)`. Skips out-of-bounds
/// pixels (saturating offsets handle the case where the destination is
/// smaller than `src + offset`).
fn blit_straight_alpha(
    dst: &mut Rgba8Image,
    off_x: u32,
    off_y: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
) {
    let dst_w = dst.width;
    let dst_h = dst.height;
    let stride = (src_w as usize) * 4;
    for sy in 0..src_h {
        let dy = off_y + sy;
        if dy >= dst_h {
            break;
        }
        for sx in 0..src_w {
            let dx = off_x + sx;
            if dx >= dst_w {
                break;
            }
            let si = (sy as usize) * stride + (sx as usize) * 4;
            let s = &src[si..si + 4];
            let sa = s[3];
            if sa == 0 {
                continue;
            }
            let bg = dst.get(dx, dy);
            // Straight-alpha over: out = src * a + dst * (1-a).
            let inv = 255 - sa;
            let r = ((s[0] as u32) * (sa as u32) + (bg[0] as u32) * (inv as u32) + 127) / 255;
            let g = ((s[1] as u32) * (sa as u32) + (bg[1] as u32) * (inv as u32) + 127) / 255;
            let b = ((s[2] as u32) * (sa as u32) + (bg[2] as u32) * (inv as u32) + 127) / 255;
            // Output alpha = 1 - (1-sa)(1-da).
            let da = bg[3] as u32;
            let out_a = sa as u32 + (da * inv as u32 + 127) / 255;
            let out_a = out_a.min(255) as u8;
            dst.put(dx, dy, [r as u8, g as u8, b as u8, out_a]);
        }
    }
}

fn load_face(font_path: Option<&str>) -> Result<Face> {
    let bytes: Vec<u8> = match font_path {
        Some(path) => std::fs::read(path).map_err(|e| {
            Error::invalid(format!("label: failed to read font {path:?}: {e}"))
        })?,
        None => DEFAULT_FONT.to_vec(),
    };
    Face::from_ttf_bytes(bytes).map_err(|e| {
        Error::invalid(format!("label: failed to parse font: {e:?}"))
    })
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
    fn label_default_white_bg_with_padding() {
        let img = render(&map(&[("text", "Hi")])).unwrap();
        // Auto-fit canvas is text bbox + 2*padding. 'Hi' at 24 px in
        // DejaVu Sans Mono is well over zero pixels in either axis.
        assert!(img.width > 8);
        assert!(img.height > 8);
        // Top-left corner should be background (white).
        assert_eq!(img.get(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn label_explicit_dimensions_override() {
        let img = render(&map(&[("text", "X"), ("w", "200"), ("h", "100")])).unwrap();
        assert_eq!(img.width, 200);
        assert_eq!(img.height, 100);
    }

    #[test]
    fn label_empty_text_yields_padding_only_canvas() {
        let img = render(&map(&[("text", "")])).unwrap();
        // Empty shape collapses to 0×0; auto-fit then becomes 2*padding
        // on each side (4 default ⇒ 8×8 minimum), so we never return a
        // zero-size frame.
        assert!(img.width >= 1);
        assert!(img.height >= 1);
    }

    #[test]
    fn label_color_and_bg_round_trip() {
        let img = render(&map(&[
            ("text", "."),
            ("color", "red"),
            ("bg", "blue"),
            ("padding", "8"),
        ]))
        .unwrap();
        // With padding > 0 the corners are guaranteed to be background
        // (no glyph alpha can reach them).
        assert_eq!(img.get(0, 0), [0, 0, 255, 255]);
        assert_eq!(img.get(img.width - 1, img.height - 1), [0, 0, 255, 255]);
    }

    #[test]
    fn label_bad_size_rejected() {
        let err = render(&map(&[("text", "x"), ("size", "0")])).unwrap_err();
        assert!(format!("{err:?}").contains("size must be a positive"));
    }

    #[test]
    fn label_missing_font_file_clear_error() {
        let err =
            render(&map(&[("text", "x"), ("font", "/nonexistent/file.ttf")])).unwrap_err();
        assert!(format!("{err:?}").contains("failed to read font"));
    }
}

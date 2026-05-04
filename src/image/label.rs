//! `label:` text-to-image generator.
//!
//! Renders a string to an RGBA8 canvas using `oxideav-scribe` (vector
//! shaping → positioned glyph nodes) plus `oxideav-raster` (vector →
//! pixels). Mirrors ImageMagick's `label:Hello world` source: produces
//! a still image sized to the rendered text plus padding (or to an
//! explicit `w=… h=…` if the caller provides them).
//!
//! Default font is the bundled DejaVu Sans Mono (~340 KB). Pass
//! `font=/path/to/your.ttf` to override.

use std::collections::BTreeMap;

use oxideav_core::{
    Error, FillRule, Group, Node, Paint, PathNode, Result, Rgba as CoreRgba, Transform2D,
    VectorFrame,
};
use oxideav_raster::Renderer;
use oxideav_scribe::{Face, FaceChain, Rgba, Shaper};

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
    let explicit_w = query
        .get("w")
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| Error::invalid("label: w must be a non-negative integer".to_string()))?;
    let explicit_h = query
        .get("h")
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| Error::invalid("label: h must be a non-negative integer".to_string()))?;

    let face = load_face(query.get("font").map(|s| s.as_str()))?;

    // Vector pipeline: shape into a positioned-glyph chain, measure the
    // run's natural extent, recolour the default-black fill to the
    // requested colour, then render via oxideav-raster.

    // Pre-compute the run's natural pixel extent for canvas sizing. The
    // shaper returns positioned glyphs whose `(x_offset + x_advance)`
    // chain gives the total pen advance; the face metrics give the
    // vertical extent (ascent above baseline + descent below).
    let glyphs = Shaper::shape(&face, text, size_px)
        .map_err(|e| Error::invalid(format!("label: scribe shape failed: {e:?}")))?;
    let advance_px: f32 = glyphs.iter().map(|g| g.x_offset + g.x_advance).sum();
    let ascent_px = face.ascent_px(size_px);
    let descent_px = face.descent_px(size_px); // typically negative
    let glyph_w = advance_px.ceil().max(0.0) as u32;
    let glyph_h = (ascent_px - descent_px).ceil().max(0.0) as u32;

    let canvas_w =
        explicit_w.unwrap_or_else(|| glyph_w.saturating_add(padding.saturating_mul(2)).max(1));
    let canvas_h =
        explicit_h.unwrap_or_else(|| glyph_h.saturating_add(padding.saturating_mul(2)).max(1));

    // Empty / whitespace-only run → return a padded background canvas.
    if glyph_w == 0 || glyph_h == 0 {
        return Ok(filled_canvas(canvas_w, canvas_h, bg));
    }

    // Build the vector scene: an outer Group whose children are the
    // shape_to_paths glyph nodes (each already wrapped in a cache-keyed
    // Group with its own `Transform2D::translate(target_x, y_offset)`
    // relative to the run's pen origin). The outer Group translates
    // the whole run so the baseline lands at
    // `(centre_off_x, centre_off_y + ascent_px)` inside the canvas.
    let chain = FaceChain::new(face);
    let placed = Shaper::shape_to_paths(&chain, text, size_px);

    // Centre the run inside the canvas. Both axes independently centred
    // — same behaviour as the previous bitmap-blit path.
    let centre_off_x = (canvas_w.saturating_sub(glyph_w) / 2) as f32;
    let centre_off_y = (canvas_h.saturating_sub(glyph_h) / 2) as f32;
    // Pen Y inside the canvas: top of the bbox sits at centre_off_y,
    // baseline lives `ascent_px` below that.
    let pen_y = centre_off_y + ascent_px;
    let pen_x = centre_off_x;

    let fill = Paint::Solid(rgba_to_core(color));
    let mut root = Group {
        transform: Transform2D::translate(pen_x, pen_y),
        ..Group::default()
    };
    for (_face_idx, glyph_node, transform) in placed {
        // shape_to_paths already wraps each glyph in
        // Group { cache_key: Some, children: [PathNode|Image] } with an
        // identity inner transform; we wrap that in a *placement* Group
        // carrying the per-glyph translate. The recolour pass walks the
        // inner PathNode (outline glyphs) and replaces the default-black
        // fill with the requested colour. Image glyphs (CBDT/sbix) keep
        // their carried palette unchanged.
        let recoloured = recolour_glyph(glyph_node, &fill);
        let placement = Group {
            transform,
            children: vec![recoloured],
            ..Group::default()
        };
        root.children.push(Node::Group(placement));
    }
    let mut frame = VectorFrame::new(canvas_w as f32, canvas_h as f32);
    frame.root = root;

    let mut renderer = Renderer::new(canvas_w, canvas_h);
    renderer.background = rgba_to_core(bg);
    let video_frame = renderer.render(&frame);
    let plane = video_frame
        .planes
        .into_iter()
        .next()
        .ok_or_else(|| Error::invalid("label: raster produced empty frame".to_string()))?;

    // The renderer emits packed straight-alpha RGBA8 with stride =
    // width*4; copy verbatim into Rgba8Image.
    let expected = (canvas_w as usize) * (canvas_h as usize) * 4;
    if plane.data.len() != expected || plane.stride != (canvas_w as usize) * 4 {
        return Err(Error::invalid(format!(
            "label: raster output size mismatch: stride={} bytes={} expected={}",
            plane.stride,
            plane.data.len(),
            expected
        )));
    }
    Ok(Rgba8Image {
        width: canvas_w,
        height: canvas_h,
        pixels: plane.data,
    })
}

/// Build a `width × height` canvas filled with `colour`. Used when the
/// run shapes to zero glyphs (empty / whitespace-only text) so callers
/// always get a valid frame.
fn filled_canvas(width: u32, height: u32, colour: Rgba) -> Rgba8Image {
    let mut img = Rgba8Image::new(width, height);
    for y in 0..height {
        for x in 0..width {
            img.put(x, y, colour);
        }
    }
    img
}

/// Replace the default-black `PathNode.fill` on outline glyph nodes
/// with the requested run colour. `shape_to_paths` always wraps each
/// glyph in `Group { children: [PathNode | Image] }` (round-8 cache
/// envelope), so we walk into that one child and rewrite the fill in
/// place. Bitmap glyphs (`Node::Image`) carry their own colour and are
/// left untouched.
fn recolour_glyph(node: Node, fill: &Paint) -> Node {
    match node {
        Node::Group(mut g) => {
            for child in g.children.iter_mut() {
                let placeholder = std::mem::replace(child, Node::Group(Group::default()));
                *child = recolour_glyph(placeholder, fill);
            }
            Node::Group(g)
        }
        Node::Path(p) => Node::Path(PathNode {
            path: p.path,
            fill: Some(fill.clone()),
            stroke: p.stroke,
            fill_rule: FillRule::NonZero,
        }),
        // Bitmap glyphs (CBDT/sbix → Node::Image) keep their carried
        // palette; the run `color` parameter is meaningless for them.
        other => other,
    }
}

fn rgba_to_core(c: Rgba) -> CoreRgba {
    CoreRgba::new(c[0], c[1], c[2], c[3])
}

fn load_face(font_path: Option<&str>) -> Result<Face> {
    let bytes: Vec<u8> = match font_path {
        Some(path) => std::fs::read(path)
            .map_err(|e| Error::invalid(format!("label: failed to read font {path:?}: {e}")))?,
        None => DEFAULT_FONT.to_vec(),
    };
    Face::from_ttf_bytes(bytes)
        .map_err(|e| Error::invalid(format!("label: failed to parse font: {e:?}")))
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
        let err = render(&map(&[("text", "x"), ("font", "/nonexistent/file.ttf")])).unwrap_err();
        assert!(format!("{err:?}").contains("failed to read font"));
    }
}

//! Solid colour generator (`xc:` — the traditional constant-colour
//! canvas shorthand from classical CLI image tools; "xc" stands for
//! "X constant colour").

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::Rgba8Image;
use super::canvas::{self, Image, RenderConfig, SolidField};
use super::palette::parse_color_unit;
use crate::source::{q_str, q_u32};

/// The canvas as a [`SolidField`]: `w`, `h`, `color`.
pub fn field(query: &BTreeMap<String, String>) -> Result<SolidField> {
    Ok(SolidField {
        width: q_u32(query, "w", 640)?.max(1),
        height: q_u32(query, "h", 480)?.max(1),
        color: parse_color_unit(q_str(query, "color", "black"))?,
    })
}

/// `generate://xc?…` as 8-bit RGBA.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    Ok(canvas::render(&field(query)?, &RenderConfig::new()).into_rgba8())
}

/// `generate://xc?…` in the depth / layout the query asks for.
pub fn render_image(query: &BTreeMap<String, String>) -> Result<Image> {
    Ok(canvas::render(
        &field(query)?,
        &RenderConfig::from_query(query)?,
    ))
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
    fn xc_red_default_dimensions() {
        let img = render(&map(&[("color", "red")])).unwrap();
        assert_eq!(img.width, 640);
        assert_eq!(img.height, 480);
        assert_eq!(img.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(100, 100), [255, 0, 0, 255]);
    }

    #[test]
    fn xc_hex_with_alpha() {
        let img = render(&map(&[("color", "#80808080"), ("w", "10"), ("h", "10")])).unwrap();
        assert_eq!(img.get(5, 5), [128, 128, 128, 128]);
    }

    #[test]
    fn xc_at_every_depth_and_thread_budget() {
        let q = |extra: &[(&str, &str)]| {
            let mut m = map(&[("color", "#336699"), ("w", "7"), ("h", "5")]);
            m.extend(map(extra));
            render_image(&m).unwrap()
        };
        let deep = q(&[("depth", "16"), ("channels", "rgb")]);
        assert!(
            deep.as_u16()
                .unwrap()
                .chunks(3)
                .all(|p| p == [0x3333, 0x6666, 0x9999])
        );
        let float = q(&[("depth", "float"), ("channels", "rgb")]);
        assert!(
            float
                .as_f32()
                .unwrap()
                .chunks(3)
                .all(|p| p == [0.2, 0.4, 0.6])
        );
        assert_eq!(q(&[]), q(&[("threads", "4")]));
        // A 16-bit hex colour is exact at 16 bits and rounds at 8.
        let deep = render_image(&map(&[
            ("color", "#123456789abc"),
            ("w", "2"),
            ("h", "1"),
            ("depth", "16"),
            ("channels", "rgb"),
        ]))
        .unwrap();
        assert_eq!(deep.as_u16().unwrap()[..3], [0x1234, 0x5678, 0x9abc]);
        let low = render(&map(&[("color", "#123456789abc"), ("w", "1"), ("h", "1")])).unwrap();
        assert_eq!(low.get(0, 0), [0x12, 0x56, 0x9a, 255]);
        assert_eq!(q(&[("dither", "noise")]), q(&[]));
    }
}

//! Solid colour generator (`xc:` — ImageMagick "constant colour" canvas).

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::palette::parse_color;
use super::{png_encode, Rgba8Image};
use crate::source::{q_str, q_u32};

/// `generate://xc?color=red&w=640&h=480` → PNG bytes.
pub fn generate(query: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let img = render(query)?;
    Ok(png_encode(&img))
}

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let color = parse_color(q_str(query, "color", "black"))?;
    let mut img = Rgba8Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.put(x, y, color);
        }
    }
    Ok(img)
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
    fn xc_emits_valid_png() {
        let bytes = generate(&map(&[("color", "blue"), ("w", "8"), ("h", "8")])).unwrap();
        // PNG signature
        assert_eq!(
            &bytes[0..8],
            &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']
        );
    }
}

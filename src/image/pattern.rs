//! Geometric patterns — checkerboard plus simple stripes.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::palette::parse_color;
use super::{png_encode, Rgba8Image};
use crate::source::{q_str, q_u32};

/// `generate://pattern?type=checkerboard&w=…&h=…&size=32&color1=black&color2=white` →
/// PNG bytes.
pub fn generate(query: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let img = render(query)?;
    Ok(png_encode(&img))
}

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let kind = q_str(query, "type", "checkerboard");
    let cell = q_u32(query, "size", 32)?.max(1);
    let c1 = parse_color(q_str(query, "color1", "black"))?;
    let c2 = parse_color(q_str(query, "color2", "white"))?;

    let mut img = Rgba8Image::new(w, h);
    match kind {
        "checkerboard" | "checker" => fill_checker(&mut img, cell, c1, c2),
        "horizontal_stripes" | "hstripes" | "hbars" => fill_h_stripes(&mut img, cell, c1, c2),
        "vertical_stripes" | "vstripes" | "vbars" => fill_v_stripes(&mut img, cell, c1, c2),
        other => {
            return Err(Error::invalid(format!(
                "pattern: unknown type {other:?} (expected checkerboard|horizontal_stripes|vertical_stripes)"
            )));
        }
    }
    Ok(img)
}

fn fill_checker(img: &mut Rgba8Image, cell: u32, c1: [u8; 4], c2: [u8; 4]) {
    for y in 0..img.height {
        for x in 0..img.width {
            let cx = x / cell;
            let cy = y / cell;
            let c = if (cx + cy) % 2 == 0 { c1 } else { c2 };
            img.put(x, y, c);
        }
    }
}

fn fill_h_stripes(img: &mut Rgba8Image, cell: u32, c1: [u8; 4], c2: [u8; 4]) {
    for y in 0..img.height {
        let band = y / cell;
        let c = if band % 2 == 0 { c1 } else { c2 };
        for x in 0..img.width {
            img.put(x, y, c);
        }
    }
}

fn fill_v_stripes(img: &mut Rgba8Image, cell: u32, c1: [u8; 4], c2: [u8; 4]) {
    for y in 0..img.height {
        for x in 0..img.width {
            let band = x / cell;
            let c = if band % 2 == 0 { c1 } else { c2 };
            img.put(x, y, c);
        }
    }
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
    fn checker_alternates_per_cell() {
        let img = render(&map(&[
            ("type", "checkerboard"),
            ("w", "20"),
            ("h", "20"),
            ("size", "10"),
        ]))
        .unwrap();
        // Top-left cell = c1 (black), top-right cell = c2 (white).
        assert_eq!(img.get(0, 0), [0, 0, 0, 255]);
        assert_eq!(img.get(15, 0), [255, 255, 255, 255]);
        assert_eq!(img.get(15, 15), [0, 0, 0, 255]);
    }

    #[test]
    fn horizontal_stripes_alternate_per_row_band() {
        let img = render(&map(&[
            ("type", "hstripes"),
            ("w", "20"),
            ("h", "20"),
            ("size", "5"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0), [0, 0, 0, 255]);
        assert_eq!(img.get(0, 7), [255, 255, 255, 255]);
        assert_eq!(img.get(0, 12), [0, 0, 0, 255]);
    }

    #[test]
    fn unknown_pattern_errors() {
        assert!(render(&map(&[("type", "tartan")])).is_err());
    }
}

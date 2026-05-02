//! Linear and radial gradients.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::palette::parse_color;
use super::{png_encode, Rgba8Image};
use crate::source::{q_str, q_u32};

/// `generate://gradient?from=red&to=blue&direction=horizontal&w=…&h=…` →
/// PNG bytes.
pub fn generate(query: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let img = render(query)?;
    Ok(png_encode(&img))
}

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let from = parse_color(q_str(query, "from", "black"))?;
    let to = parse_color(q_str(query, "to", "white"))?;
    let kind = q_str(query, "type", "linear");
    let direction = q_str(query, "direction", "horizontal");

    let mut img = Rgba8Image::new(w, h);
    match kind {
        "linear" => fill_linear(&mut img, from, to, direction)?,
        "radial" => fill_radial(&mut img, from, to),
        other => {
            return Err(Error::invalid(format!(
                "gradient: unknown type {other:?} (expected linear|radial)"
            )));
        }
    }
    Ok(img)
}

fn fill_linear(img: &mut Rgba8Image, from: [u8; 4], to: [u8; 4], dir: &str) -> Result<()> {
    let w = img.width;
    let h = img.height;
    for y in 0..h {
        for x in 0..w {
            let t = match dir {
                "horizontal" | "h" | "lr" => {
                    if w <= 1 {
                        0.0
                    } else {
                        x as f32 / (w - 1) as f32
                    }
                }
                "vertical" | "v" | "tb" => {
                    if h <= 1 {
                        0.0
                    } else {
                        y as f32 / (h - 1) as f32
                    }
                }
                "diagonal" | "d" | "diag" => {
                    let denom = ((w - 1) + (h - 1)).max(1) as f32;
                    (x + y) as f32 / denom
                }
                other => {
                    return Err(Error::invalid(format!(
                        "gradient: unknown direction {other:?} (expected horizontal|vertical|diagonal)"
                    )));
                }
            };
            img.put(x, y, lerp_rgba(from, to, t));
        }
    }
    Ok(())
}

fn fill_radial(img: &mut Rgba8Image, from: [u8; 4], to: [u8; 4]) {
    let w = img.width;
    let h = img.height;
    let cx = (w as f32) / 2.0;
    let cy = (h as f32) / 2.0;
    let max_r = (cx * cx + cy * cy).sqrt().max(1.0);
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32) - cx;
            let dy = (y as f32) - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let t = (r / max_r).min(1.0);
            img.put(x, y, lerp_rgba(from, to, t));
        }
    }
}

#[inline]
fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| ((x as f32) + ((y as f32) - (x as f32)) * t).round() as u8;
    [
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ]
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
    fn linear_horizontal_endpoints() {
        let img = render(&map(&[
            ("from", "red"),
            ("to", "blue"),
            ("w", "100"),
            ("h", "10"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 5), [255, 0, 0, 255]);
        assert_eq!(img.get(99, 5), [0, 0, 255, 255]);
    }

    #[test]
    fn linear_vertical_endpoints() {
        let img = render(&map(&[
            ("from", "white"),
            ("to", "black"),
            ("direction", "vertical"),
            ("w", "10"),
            ("h", "100"),
        ]))
        .unwrap();
        assert_eq!(img.get(5, 0), [255, 255, 255, 255]);
        assert_eq!(img.get(5, 99), [0, 0, 0, 255]);
    }

    #[test]
    fn radial_center_is_from() {
        let img = render(&map(&[
            ("type", "radial"),
            ("from", "yellow"),
            ("to", "black"),
            ("w", "100"),
            ("h", "100"),
        ]))
        .unwrap();
        let center = img.get(50, 50);
        // Allow off-by-one half-pixel slop.
        assert!(center[0] > 200 && center[1] > 200);
    }

    #[test]
    fn unknown_direction_errors() {
        let err = render(&map(&[
            ("from", "red"),
            ("to", "blue"),
            ("direction", "spiral"),
        ]))
        .unwrap_err();
        assert!(format!("{err}").contains("spiral"));
    }
}

//! Mandelbrot + Julia sets.
//!
//! Standard escape-time iteration with palette lookup — no smoothing,
//! no anti-aliasing. Centre / zoom / iteration depth all configurable.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::palette::default_palette;
use super::Rgba8Image;
use crate::source::{q_f64, q_str, q_u32};

pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let kind = q_str(query, "type", "mandelbrot");
    let cx = q_f64(query, "cx", -0.5)?;
    let cy = q_f64(query, "cy", 0.0)?;
    let zoom = q_f64(query, "zoom", 1.0)?.max(1e-12);
    let iter = q_u32(query, "iter", 256)?.max(1);
    let escape_r2 = q_f64(query, "escape_r2", 4.0)?;

    let mut img = Rgba8Image::new(w, h);
    let palette = default_palette();

    // Window: a 4.0 × (4.0 * h/w) box at zoom=1, centred on (cx, cy),
    // shrinking with zoom.
    let win_w = 4.0 / zoom;
    let win_h = win_w * (h as f64) / (w as f64);
    let x0 = cx - win_w / 2.0;
    let y0 = cy - win_h / 2.0;
    let dx = win_w / (w as f64);
    let dy = win_h / (h as f64);

    match kind {
        "mandelbrot" => {
            for py in 0..h {
                for px in 0..w {
                    let cr = x0 + (px as f64) * dx;
                    let ci = y0 + (py as f64) * dy;
                    let it = mandelbrot_escape(cr, ci, iter, escape_r2);
                    let p = palette[((it * 255 / iter) % 256) as usize];
                    img.put(px, py, p);
                }
            }
        }
        "julia" => {
            // For Julia we reuse cx/cy as the Julia constant, and let the
            // viewport sit at (0, 0) with an unzoomed 4×(4*h/w) window
            // unless `view_zoom` is given.
            let jc_re = cx;
            let jc_im = cy;
            let view_zoom = q_f64(query, "view_zoom", 1.0)?.max(1e-12);
            let win_w = 4.0 / view_zoom;
            let win_h = win_w * (h as f64) / (w as f64);
            let x0 = -win_w / 2.0;
            let y0 = -win_h / 2.0;
            let dx = win_w / (w as f64);
            let dy = win_h / (h as f64);
            for py in 0..h {
                for px in 0..w {
                    let zr = x0 + (px as f64) * dx;
                    let zi = y0 + (py as f64) * dy;
                    let it = julia_escape(zr, zi, jc_re, jc_im, iter, escape_r2);
                    let p = palette[((it * 255 / iter) % 256) as usize];
                    img.put(px, py, p);
                }
            }
        }
        other => {
            return Err(Error::invalid(format!(
                "fractal: unknown type {other:?} (expected mandelbrot|julia)"
            )));
        }
    }
    Ok(img)
}

#[inline]
fn mandelbrot_escape(cr: f64, ci: f64, max_iter: u32, escape_r2: f64) -> u32 {
    let mut zr = 0.0;
    let mut zi = 0.0;
    let mut it = 0;
    while it < max_iter {
        let zr2 = zr * zr;
        let zi2 = zi * zi;
        if zr2 + zi2 > escape_r2 {
            return it;
        }
        let new_zi = 2.0 * zr * zi + ci;
        let new_zr = zr2 - zi2 + cr;
        zr = new_zr;
        zi = new_zi;
        it += 1;
    }
    max_iter
}

#[inline]
fn julia_escape(mut zr: f64, mut zi: f64, cr: f64, ci: f64, max_iter: u32, escape_r2: f64) -> u32 {
    let mut it = 0;
    while it < max_iter {
        let zr2 = zr * zr;
        let zi2 = zi * zi;
        if zr2 + zi2 > escape_r2 {
            return it;
        }
        let new_zi = 2.0 * zr * zi + ci;
        let new_zr = zr2 - zi2 + cr;
        zr = new_zr;
        zi = new_zi;
        it += 1;
    }
    max_iter
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
    fn mandelbrot_origin_does_not_escape() {
        let it = mandelbrot_escape(-0.5, 0.0, 256, 4.0);
        assert_eq!(it, 256, "(-0.5, 0) is in the main cardioid");
    }

    #[test]
    fn mandelbrot_escapes_far_from_set() {
        let it = mandelbrot_escape(2.0, 2.0, 256, 4.0);
        assert!(it < 5, "(2, 2) should escape almost immediately");
    }

    #[test]
    fn julia_renders_at_low_res() {
        let img = render(&map(&[
            ("type", "julia"),
            ("cx", "-0.7"),
            ("cy", "0.27"),
            ("w", "32"),
            ("h", "32"),
            ("iter", "32"),
        ]))
        .unwrap();
        assert_eq!(img.width, 32);
        assert_eq!(img.height, 32);
    }

    #[test]
    fn unknown_fractal_type_errors() {
        let err = render(&map(&[("type", "burning_ship")])).unwrap_err();
        assert!(format!("{err}").contains("burning_ship"));
    }
}

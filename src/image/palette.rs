//! CSS named colour table + `#RRGGBB` / `#RRGGBBAA` parser, plus the
//! shared 256-entry default palette used by fractal / plasma / Perlin
//! generators.

use oxideav_core::{Error, Result};

/// Parse a colour string into RGBA8 bytes.
///
/// Accepts:
/// - CSS named colours (case-insensitive): `red`, `green`, `blue`,
///   `transparent`, … (subset, see [`NAMED_COLORS`])
/// - `#RGB` (4-bit per component, expanded to 8-bit by repetition)
/// - `#RRGGBB`
/// - `#RRGGBBAA`
pub fn parse_color(s: &str) -> Result<[u8; 4]> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        return parse_hex(hex)
            .ok_or_else(|| Error::invalid(format!("color: '#{hex}' is not a valid hex colour")));
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rgba) = NAMED_COLORS
        .iter()
        .find(|(n, _)| *n == lower.as_str())
        .map(|(_, c)| *c)
    {
        return Ok(rgba);
    }
    Err(Error::invalid(format!("color: unknown colour {trimmed:?}")))
}

fn parse_hex(s: &str) -> Option<[u8; 4]> {
    fn h(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    match bytes.len() {
        3 => {
            let r = h(bytes[0])?;
            let g = h(bytes[1])?;
            let b = h(bytes[2])?;
            Some([r * 17, g * 17, b * 17, 255])
        }
        4 => {
            let r = h(bytes[0])?;
            let g = h(bytes[1])?;
            let b = h(bytes[2])?;
            let a = h(bytes[3])?;
            Some([r * 17, g * 17, b * 17, a * 17])
        }
        6 => {
            let r = h(bytes[0])? * 16 + h(bytes[1])?;
            let g = h(bytes[2])? * 16 + h(bytes[3])?;
            let b = h(bytes[4])? * 16 + h(bytes[5])?;
            Some([r, g, b, 255])
        }
        8 => {
            let r = h(bytes[0])? * 16 + h(bytes[1])?;
            let g = h(bytes[2])? * 16 + h(bytes[3])?;
            let b = h(bytes[4])? * 16 + h(bytes[5])?;
            let a = h(bytes[6])? * 16 + h(bytes[7])?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

/// Named CSS colour table (subset — the popular core named colours,
/// not the full HTML4/CSS3 list. Add more on demand.).
pub static NAMED_COLORS: &[(&str, [u8; 4])] = &[
    ("transparent", [0, 0, 0, 0]),
    ("none", [0, 0, 0, 0]),
    ("black", [0, 0, 0, 255]),
    ("white", [255, 255, 255, 255]),
    ("red", [255, 0, 0, 255]),
    ("green", [0, 128, 0, 255]),
    ("lime", [0, 255, 0, 255]),
    ("blue", [0, 0, 255, 255]),
    ("yellow", [255, 255, 0, 255]),
    ("cyan", [0, 255, 255, 255]),
    ("aqua", [0, 255, 255, 255]),
    ("magenta", [255, 0, 255, 255]),
    ("fuchsia", [255, 0, 255, 255]),
    ("silver", [192, 192, 192, 255]),
    ("gray", [128, 128, 128, 255]),
    ("grey", [128, 128, 128, 255]),
    ("maroon", [128, 0, 0, 255]),
    ("olive", [128, 128, 0, 255]),
    ("purple", [128, 0, 128, 255]),
    ("teal", [0, 128, 128, 255]),
    ("navy", [0, 0, 128, 255]),
    ("orange", [255, 165, 0, 255]),
    ("pink", [255, 192, 203, 255]),
    ("brown", [165, 42, 42, 255]),
    ("gold", [255, 215, 0, 255]),
    ("violet", [238, 130, 238, 255]),
    ("indigo", [75, 0, 130, 255]),
];

/// 256-entry default palette — a smooth viridis-ish gradient through
/// black → blue → cyan → green → yellow → red → white. Used by
/// fractal / plasma / Perlin renderers when no `palette=` override
/// is given.
pub fn default_palette() -> [[u8; 4]; 256] {
    let mut palette = [[0u8; 4]; 256];
    for (i, slot) in palette.iter_mut().enumerate() {
        let t = i as f32 / 255.0;
        let (r, g, b) = viridis(t);
        *slot = [r, g, b, 255];
    }
    palette
}

/// Approximate viridis colour map. Continuous, monotonic luminance,
/// reads well at greyscale.
fn viridis(t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    // Five-stop linear interpolation: dark purple → blue → green → yellow → light yellow.
    let stops: [(f32, [f32; 3]); 5] = [
        (0.00, [0.267, 0.005, 0.329]),
        (0.25, [0.230, 0.299, 0.546]),
        (0.50, [0.128, 0.567, 0.551]),
        (0.75, [0.369, 0.789, 0.382]),
        (1.00, [0.993, 0.906, 0.144]),
    ];
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let local = (t - t0) / (t1 - t0).max(1e-6);
            let r = c0[0] + local * (c1[0] - c0[0]);
            let g = c0[1] + local * (c1[1] - c0[1]);
            let b = c0[2] + local * (c1[2] - c0[2]);
            return (
                (r * 255.0).clamp(0.0, 255.0) as u8,
                (g * 255.0).clamp(0.0, 255.0) as u8,
                (b * 255.0).clamp(0.0, 255.0) as u8,
            );
        }
    }
    (255, 255, 0)
}

/// HSL → RGB (each in 0..1). Used by gradient_animate's hue rotation.
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        (
            hue_to_rgb(p, q, h + 1.0 / 3.0),
            hue_to_rgb(p, q, h),
            hue_to_rgb(p, q, h - 1.0 / 3.0),
        )
    };
    (
        (r * 255.0).clamp(0.0, 255.0) as u8,
        (g * 255.0).clamp(0.0, 255.0) as u8,
        (b * 255.0).clamp(0.0, 255.0) as u8,
    )
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_red_is_pure_red() {
        assert_eq!(parse_color("red").unwrap(), [255, 0, 0, 255]);
    }

    #[test]
    fn hex_six_digit() {
        assert_eq!(parse_color("#ff0000").unwrap(), [255, 0, 0, 255]);
        assert_eq!(parse_color("#00FF00").unwrap(), [0, 255, 0, 255]);
    }

    #[test]
    fn hex_eight_digit_with_alpha() {
        assert_eq!(parse_color("#ff000080").unwrap(), [255, 0, 0, 128]);
    }

    #[test]
    fn hex_three_digit_short_form() {
        // #f00 expands to #ff0000.
        assert_eq!(parse_color("#f00").unwrap(), [255, 0, 0, 255]);
    }

    #[test]
    fn unknown_color_errors() {
        assert!(parse_color("ferocious-pink").is_err());
    }

    #[test]
    fn case_insensitive_named() {
        assert_eq!(parse_color("RED").unwrap(), [255, 0, 0, 255]);
        assert_eq!(parse_color("Red").unwrap(), [255, 0, 0, 255]);
    }

    #[test]
    fn palette_endpoints_sane() {
        let p = default_palette();
        // First entry should be dark, last bright.
        let dark_lum = (p[0][0] as u32 + p[0][1] as u32 + p[0][2] as u32) / 3;
        let bright_lum = (p[255][0] as u32 + p[255][1] as u32 + p[255][2] as u32) / 3;
        assert!(dark_lum < bright_lum);
    }
}

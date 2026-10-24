//! Colour parser (CSS Color 4 named colours, hex, `rgb()` / `rgba()` /
//! `srgb()` / `srgba()` / `hsl()` / `hsla()` / `gray()` functional
//! forms, X11 `grayN` percent greys), plus the shared 256-entry
//! default palette used by fractal / plasma / Perlin generators.

use oxideav_core::{Error, Result};

/// Parse a colour string into RGBA8 bytes.
///
/// Accepts (all case-insensitive, surrounding whitespace ignored):
/// - the 148 CSS Color 4 named colours plus `transparent` / `none`
///   (see [`NAMED_COLORS`])
/// - `grayN` / `greyN` for `N ∈ 0..=100` — the X11 percent greys
///   ImageMagick resolves (`gray50` is 127, not 128)
/// - `#RGB`, `#RGBA` (4-bit per component, expanded by repetition),
///   `#RRGGBB`, `#RRGGBBAA`, `#RRRRGGGGBBBB`, `#RRRRGGGGBBBBAAAA`
///   (16-bit per component, reduced to 8-bit by `round(v / 257)`)
/// - `rgb(r, g, b)` / `rgba(r, g, b, a)` and the `srgb()` / `srgba()`
///   spellings — components as 0–255 numbers (fractions allowed,
///   rounded half-up, clamped) or percentages; alpha as 0–1 or a
///   percentage
/// - `gray(v)` / `grey(v)` — one 0–255 number or percentage
/// - `hsl(h, s%, l%)` / `hsla(h, s%, l%, a)` — CSS Color 4 §7 HSL,
///   hue in degrees (any real, wrapped), saturation / lightness as
///   percentages or 0–1 fractions
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
    if let Some(level) = parse_x11_gray(&lower) {
        return Ok([level, level, level, 255]);
    }
    if let Some((func, args)) = lower.split_once('(')
        && let Some(args) = args.strip_suffix(')')
    {
        return parse_functional(func.trim(), args, trimmed);
    }
    Err(Error::invalid(format!("color: unknown colour {trimmed:?}")))
}

/// `gray0` … `gray100` / `grey0` … `grey100`. Anything else (no digits,
/// above 100, leading `+`, …) is `None` so the caller falls through to
/// its own error.
fn parse_x11_gray(lower: &str) -> Option<u8> {
    let digits = lower
        .strip_prefix("gray")
        .or_else(|| lower.strip_prefix("grey"))?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: usize = digits.parse().ok()?;
    X11_GRAY_LEVELS.get(n).copied()
}

/// `rgb(...)` / `rgba(...)` / `srgb(...)` / `srgba(...)` / `gray(...)` /
/// `grey(...)` / `hsl(...)` / `hsla(...)`.
fn parse_functional(func: &str, args: &str, original: &str) -> Result<[u8; 4]> {
    let bad = || Error::invalid(format!("color: malformed colour {original:?}"));
    let parts: Vec<&str> = args
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    match func {
        "rgb" | "rgba" | "srgb" | "srgba" => {
            if parts.len() != 3 && parts.len() != 4 {
                return Err(bad());
            }
            let r = channel_255(parts[0]).ok_or_else(bad)?;
            let g = channel_255(parts[1]).ok_or_else(bad)?;
            let b = channel_255(parts[2]).ok_or_else(bad)?;
            let a = match parts.get(3) {
                Some(p) => alpha_unit(p).ok_or_else(bad)?,
                None => 255,
            };
            Ok([r, g, b, a])
        }
        "gray" | "grey" => {
            if parts.len() != 1 && parts.len() != 2 {
                return Err(bad());
            }
            let v = channel_255(parts[0]).ok_or_else(bad)?;
            let a = match parts.get(1) {
                Some(p) => alpha_unit(p).ok_or_else(bad)?,
                None => 255,
            };
            Ok([v, v, v, a])
        }
        "hsl" | "hsla" => {
            if parts.len() != 3 && parts.len() != 4 {
                return Err(bad());
            }
            let h: f64 = parts[0]
                .strip_suffix("deg")
                .unwrap_or(parts[0])
                .parse()
                .map_err(|_| bad())?;
            let s = unit_fraction(parts[1]).ok_or_else(bad)?;
            let l = unit_fraction(parts[2]).ok_or_else(bad)?;
            let a = match parts.get(3) {
                Some(p) => alpha_unit(p).ok_or_else(bad)?,
                None => 255,
            };
            let (r, g, b) = hsl_to_rgb_f64(h, s, l);
            Ok([quantize_unit(r), quantize_unit(g), quantize_unit(b), a])
        }
        _ => Err(Error::invalid(format!(
            "color: unknown colour function {func:?} in {original:?}"
        ))),
    }
}

/// `0..=255` number (fractional allowed, rounded half-up, clamped) or
/// `N%` of 255.
fn channel_255(p: &str) -> Option<u8> {
    if let Some(pct) = p.strip_suffix('%') {
        let v: f64 = pct.parse().ok()?;
        return Some(quantize_unit(v / 100.0));
    }
    let v: f64 = p.parse().ok()?;
    Some(v.round().clamp(0.0, 255.0) as u8)
}

/// Alpha: `0..=1` fraction or `N%`.
fn alpha_unit(p: &str) -> Option<u8> {
    Some(quantize_unit(unit_fraction(p)?))
}

/// `0..=1` fraction or `N%` → f64 in `[0, 1]`.
fn unit_fraction(p: &str) -> Option<f64> {
    if let Some(pct) = p.strip_suffix('%') {
        let v: f64 = pct.parse().ok()?;
        return Some((v / 100.0).clamp(0.0, 1.0));
    }
    let v: f64 = p.parse().ok()?;
    Some(v.clamp(0.0, 1.0))
}

/// `[0, 1]` → 8-bit, rounded half-up.
fn quantize_unit(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// CSS Color 4 §7.1 HSL → RGB (`hslToRgb` sample algorithm): hue in
/// degrees, `s` / `l` in `[0, 1]`; returns RGB in `[0, 1]`.
fn hsl_to_rgb_f64(hue: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let h = hue.rem_euclid(360.0) / 30.0;
    let a = s * l.min(1.0 - l);
    let f = |n: f64| {
        let k = (n + h) % 12.0;
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    (f(0.0), f(8.0), f(4.0))
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
    // 16-bit component → 8-bit, `round(v / 257)`.
    fn wide(b: &[u8]) -> Option<u8> {
        let v = (h(b[0])? as u32) << 12
            | (h(b[1])? as u32) << 8
            | (h(b[2])? as u32) << 4
            | h(b[3])? as u32;
        Some(((v + 128) / 257) as u8)
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
        12 => Some([
            wide(&bytes[0..4])?,
            wide(&bytes[4..8])?,
            wide(&bytes[8..12])?,
            255,
        ]),
        16 => Some([
            wide(&bytes[0..4])?,
            wide(&bytes[4..8])?,
            wide(&bytes[8..12])?,
            wide(&bytes[12..16])?,
        ]),
        _ => None,
    }
}

/// Named colour table — the complete CSS Color Level 4 §6.1 "Named
/// colors" list (148 keywords; W3C CSS Color 4 staged at
/// `docs/text/opentype/spec/w3c-css-color-4.html`), plus the
/// `transparent` / `none` aliases for fully transparent black. All
/// names are matched case-insensitively.
pub static NAMED_COLORS: &[(&str, [u8; 4])] = &[
    ("transparent", [0, 0, 0, 0]),
    ("none", [0, 0, 0, 0]),
    ("aliceblue", [240, 248, 255, 255]),
    ("antiquewhite", [250, 235, 215, 255]),
    ("aqua", [0, 255, 255, 255]),
    ("aquamarine", [127, 255, 212, 255]),
    ("azure", [240, 255, 255, 255]),
    ("beige", [245, 245, 220, 255]),
    ("bisque", [255, 228, 196, 255]),
    ("black", [0, 0, 0, 255]),
    ("blanchedalmond", [255, 235, 205, 255]),
    ("blue", [0, 0, 255, 255]),
    ("blueviolet", [138, 43, 226, 255]),
    ("brown", [165, 42, 42, 255]),
    ("burlywood", [222, 184, 135, 255]),
    ("cadetblue", [95, 158, 160, 255]),
    ("chartreuse", [127, 255, 0, 255]),
    ("chocolate", [210, 105, 30, 255]),
    ("coral", [255, 127, 80, 255]),
    ("cornflowerblue", [100, 149, 237, 255]),
    ("cornsilk", [255, 248, 220, 255]),
    ("crimson", [220, 20, 60, 255]),
    ("cyan", [0, 255, 255, 255]),
    ("darkblue", [0, 0, 139, 255]),
    ("darkcyan", [0, 139, 139, 255]),
    ("darkgoldenrod", [184, 134, 11, 255]),
    ("darkgray", [169, 169, 169, 255]),
    ("darkgreen", [0, 100, 0, 255]),
    ("darkgrey", [169, 169, 169, 255]),
    ("darkkhaki", [189, 183, 107, 255]),
    ("darkmagenta", [139, 0, 139, 255]),
    ("darkolivegreen", [85, 107, 47, 255]),
    ("darkorange", [255, 140, 0, 255]),
    ("darkorchid", [153, 50, 204, 255]),
    ("darkred", [139, 0, 0, 255]),
    ("darksalmon", [233, 150, 122, 255]),
    ("darkseagreen", [143, 188, 143, 255]),
    ("darkslateblue", [72, 61, 139, 255]),
    ("darkslategray", [47, 79, 79, 255]),
    ("darkslategrey", [47, 79, 79, 255]),
    ("darkturquoise", [0, 206, 209, 255]),
    ("darkviolet", [148, 0, 211, 255]),
    ("deeppink", [255, 20, 147, 255]),
    ("deepskyblue", [0, 191, 255, 255]),
    ("dimgray", [105, 105, 105, 255]),
    ("dimgrey", [105, 105, 105, 255]),
    ("dodgerblue", [30, 144, 255, 255]),
    ("firebrick", [178, 34, 34, 255]),
    ("floralwhite", [255, 250, 240, 255]),
    ("forestgreen", [34, 139, 34, 255]),
    ("fuchsia", [255, 0, 255, 255]),
    ("gainsboro", [220, 220, 220, 255]),
    ("ghostwhite", [248, 248, 255, 255]),
    ("gold", [255, 215, 0, 255]),
    ("goldenrod", [218, 165, 32, 255]),
    ("gray", [128, 128, 128, 255]),
    ("green", [0, 128, 0, 255]),
    ("greenyellow", [173, 255, 47, 255]),
    ("grey", [128, 128, 128, 255]),
    ("honeydew", [240, 255, 240, 255]),
    ("hotpink", [255, 105, 180, 255]),
    ("indianred", [205, 92, 92, 255]),
    ("indigo", [75, 0, 130, 255]),
    ("ivory", [255, 255, 240, 255]),
    ("khaki", [240, 230, 140, 255]),
    ("lavender", [230, 230, 250, 255]),
    ("lavenderblush", [255, 240, 245, 255]),
    ("lawngreen", [124, 252, 0, 255]),
    ("lemonchiffon", [255, 250, 205, 255]),
    ("lightblue", [173, 216, 230, 255]),
    ("lightcoral", [240, 128, 128, 255]),
    ("lightcyan", [224, 255, 255, 255]),
    ("lightgoldenrodyellow", [250, 250, 210, 255]),
    ("lightgray", [211, 211, 211, 255]),
    ("lightgreen", [144, 238, 144, 255]),
    ("lightgrey", [211, 211, 211, 255]),
    ("lightpink", [255, 182, 193, 255]),
    ("lightsalmon", [255, 160, 122, 255]),
    ("lightseagreen", [32, 178, 170, 255]),
    ("lightskyblue", [135, 206, 250, 255]),
    ("lightslategray", [119, 136, 153, 255]),
    ("lightslategrey", [119, 136, 153, 255]),
    ("lightsteelblue", [176, 196, 222, 255]),
    ("lightyellow", [255, 255, 224, 255]),
    ("lime", [0, 255, 0, 255]),
    ("limegreen", [50, 205, 50, 255]),
    ("linen", [250, 240, 230, 255]),
    ("magenta", [255, 0, 255, 255]),
    ("maroon", [128, 0, 0, 255]),
    ("mediumaquamarine", [102, 205, 170, 255]),
    ("mediumblue", [0, 0, 205, 255]),
    ("mediumorchid", [186, 85, 211, 255]),
    ("mediumpurple", [147, 112, 219, 255]),
    ("mediumseagreen", [60, 179, 113, 255]),
    ("mediumslateblue", [123, 104, 238, 255]),
    ("mediumspringgreen", [0, 250, 154, 255]),
    ("mediumturquoise", [72, 209, 204, 255]),
    ("mediumvioletred", [199, 21, 133, 255]),
    ("midnightblue", [25, 25, 112, 255]),
    ("mintcream", [245, 255, 250, 255]),
    ("mistyrose", [255, 228, 225, 255]),
    ("moccasin", [255, 228, 181, 255]),
    ("navajowhite", [255, 222, 173, 255]),
    ("navy", [0, 0, 128, 255]),
    ("oldlace", [253, 245, 230, 255]),
    ("olive", [128, 128, 0, 255]),
    ("olivedrab", [107, 142, 35, 255]),
    ("orange", [255, 165, 0, 255]),
    ("orangered", [255, 69, 0, 255]),
    ("orchid", [218, 112, 214, 255]),
    ("palegoldenrod", [238, 232, 170, 255]),
    ("palegreen", [152, 251, 152, 255]),
    ("paleturquoise", [175, 238, 238, 255]),
    ("palevioletred", [219, 112, 147, 255]),
    ("papayawhip", [255, 239, 213, 255]),
    ("peachpuff", [255, 218, 185, 255]),
    ("peru", [205, 133, 63, 255]),
    ("pink", [255, 192, 203, 255]),
    ("plum", [221, 160, 221, 255]),
    ("powderblue", [176, 224, 230, 255]),
    ("purple", [128, 0, 128, 255]),
    ("rebeccapurple", [102, 51, 153, 255]),
    ("red", [255, 0, 0, 255]),
    ("rosybrown", [188, 143, 143, 255]),
    ("royalblue", [65, 105, 225, 255]),
    ("saddlebrown", [139, 69, 19, 255]),
    ("salmon", [250, 128, 114, 255]),
    ("sandybrown", [244, 164, 96, 255]),
    ("seagreen", [46, 139, 87, 255]),
    ("seashell", [255, 245, 238, 255]),
    ("sienna", [160, 82, 45, 255]),
    ("silver", [192, 192, 192, 255]),
    ("skyblue", [135, 206, 235, 255]),
    ("slateblue", [106, 90, 205, 255]),
    ("slategray", [112, 128, 144, 255]),
    ("slategrey", [112, 128, 144, 255]),
    ("snow", [255, 250, 250, 255]),
    ("springgreen", [0, 255, 127, 255]),
    ("steelblue", [70, 130, 180, 255]),
    ("tan", [210, 180, 140, 255]),
    ("teal", [0, 128, 128, 255]),
    ("thistle", [216, 191, 216, 255]),
    ("tomato", [255, 99, 71, 255]),
    ("turquoise", [64, 224, 208, 255]),
    ("violet", [238, 130, 238, 255]),
    ("wheat", [245, 222, 179, 255]),
    ("white", [255, 255, 255, 255]),
    ("whitesmoke", [245, 245, 245, 255]),
    ("yellow", [255, 255, 0, 255]),
    ("yellowgreen", [154, 205, 50, 255]),
];

/// `gray0` … `gray100` (and the `grey` spelling) — the X11 percent-grey
/// names ImageMagick resolves. The 8-bit levels are NOT a simple
/// `round(n · 2.55)` (gray50 is 127, gray10 is 26, gray90 is 229), so
/// the table was transcribed from the `magick` binary's own answers
/// (`magick -size 1x1 xc:grayN -depth 8 txt:`), one entry per percent.
static X11_GRAY_LEVELS: [u8; 101] = [
    0, 3, 5, 8, 10, 13, 15, 18, 20, 23, 26, 28, 31, 33, 36, 38, 41, 43, 46, 48, 51, 54, 56, 59, 61,
    64, 66, 69, 71, 74, 77, 79, 82, 84, 87, 89, 92, 94, 97, 99, 102, 105, 107, 110, 112, 115, 117,
    120, 122, 125, 127, 130, 133, 135, 138, 140, 143, 145, 148, 150, 153, 156, 158, 161, 163, 166,
    168, 171, 173, 176, 179, 181, 184, 186, 189, 191, 194, 196, 199, 201, 204, 207, 209, 212, 214,
    217, 219, 222, 224, 227, 229, 232, 235, 237, 240, 242, 245, 247, 250, 252, 255,
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

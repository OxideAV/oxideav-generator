//! Linear, radial and conic gradients — two colours or a stop list —
//! with the ImageMagick 7 pseudo-image option surface (`gradient:` / `radial-gradient:` and
//! the `gradient:direction` / `angle` / `vector` / `extent` /
//! `bounding-box` / `center` / `radii` defines).
//!
//! # Geometry
//!
//! Pixel `(x, y)` is sampled at its integer coordinate (no half-pixel
//! offset). A two-stop gradient is a parameter `t ∈ [0, 1]` per pixel;
//! `t = 0` is the first colour, `t = 1` the second.
//!
//! **Linear** — three ways to pick the axis, in precedence order:
//!
//! 1. `vector = (x1, y1) → (x2, y2)`: `t = ((p − p1) · (p2 − p1)) /
//!    |p2 − p1|²`, clamped. A zero-length vector gives `t = 0`
//!    everywhere.
//! 2. `angle = θ` (degrees, clockwise from North, so 0 = bottom→top,
//!    90 = left→right): unit direction `d = (sin θ, −cos θ)`;
//!    `t = (p · d − m) / (M − m)` where `m` / `M` are the smallest /
//!    largest projections of the four corner pixels `(0, 0)`,
//!    `(w−1, 0)`, `(0, h−1)`, `(w−1, h−1)`. The ramp therefore spans
//!    exactly the image's extent along `d`, whatever the aspect ratio.
//! 3. `direction` (compass point): the vector between two corner
//!    pixels / edge pixels — `South` is `(0,0)→(0,h−1)`, `East` is
//!    `(0,0)→(w−1,0)`, `NorthEast` is `(0,h−1)→(w−1,0)`, and so on
//!    (see [`GradientDirection::vector`]) — evaluated as case 1. Note
//!    that on a non-square canvas a diagonal *direction* runs corner
//!    to corner while the equivalent *angle* (45°) does not.
//!
//! Default: `South`; `SouthEast` when an `extent` is given without a
//! direction; `East` when the canvas is one pixel tall (a one-row
//! canvas would otherwise be a single colour).
//!
//! **Radial** — `t = √((dx/rx)² + (dy/ry)²)` clamped, with
//! `(dx, dy)` the offset from the centre:
//!
//! - centre: explicit `center`; else, when a `vector` or `direction`
//!   is given, *half the vector's end point* `(x2/2, y2/2)`; else the
//!   canvas centre `((w−1)/2, (h−1)/2)`.
//! - radii: explicit `radii = rx[, ry]` (a missing `ry` keeps the
//!   extent's vertical radius); else from the `extent` and the canvas
//!   half-sizes `hx = (w−1)/2`, `hy = (h−1)/2` — `Circle` / `Maximum`
//!   → `max(hx, hy)` both ways, `Minimum` → `min(hx, hy)`, `Ellipse`
//!   → `(hx, hy)`, `Diagonal` → `√(hx² + hy²)`. Default `Circle`.
//!   The extent is a property of the canvas, not of the centre.
//!   Four further extents are measured from the radial centre `(cx, cy)`
//!   to the canvas border, whose edges are the outermost pixel centres
//!   `x = 0`, `x = w−1`, `y = 0`, `y = h−1` (the same convention as the
//!   half-sizes above). All four are circles:
//!   `ClosestSide` → `min(cx, w−1−cx, cy, h−1−cy)`, `FarthestSide` →
//!   the `max` of the same four, `ClosestCorner` / `FarthestCorner` →
//!   the smallest / largest distance to the four corner pixels. With
//!   the default centre they reduce to `Minimum`, `Maximum`,
//!   `Diagonal` and `Diagonal`.
//! - a zero radius is a step: `t = 0` on the axis, `1` off it.
//!
//! **Bounding box** `WxH+X+Y` (must fit inside the canvas, like
//! ImageMagick which refuses otherwise) masks the result: pixels
//! outside keep the first colour's RGB, opaque when both stops are
//! opaque and fully transparent when either stop carries alpha. The
//! gradient inside is still computed against the *whole* canvas.
//!
//! # Colour
//!
//! Stops interpolate with premultiplied alpha and are un-premultiplied
//! afterwards (so `rgba(255,0,0,0.5)-blue` reaches `(85, 0, 170, 191)`
//! at the midpoint; a fully transparent pixel is `(0,0,0,0)`). The
//! default `Srgb` space interpolates the stored (gamma-encoded) values,
//! which is what ImageMagick does; `Linear` decodes the stops through
//! the sRGB transfer function (CSS Color 4 §10.2 `lin_sRGB` /
//! `gam_sRGB`), interpolates in linear light and re-encodes — the
//! "gamma-correct" gradient, `black-white` then reaches 188 rather
//! than 128 at the midpoint.
//!
//! Quantisation follows the Q16 path: `q = round(65535 · v)`, then
//! `round(q / 257)` to 8 bits, so values match the `magick` binary's
//! `-depth 8` output exactly.
//!
//! # Stops, spread, conic, colour spaces
//!
//! **Stops** — instead of two colours a gradient can carry a list of
//! [`GradientStop`]s (`stops=red, white 30%, blue`). Offsets are
//! fractions of the ramp; a missing first / last offset is 0 / 1, an
//! offset smaller than an earlier one is raised to it, and runs of
//! missing offsets are spaced evenly. Between two stops the colour is
//! the linear interpolation of the premultiplied components at
//! `u = (t − o₀) / (o₁ − o₀)`; two stops at the same offset make a hard
//! edge (the later one applies from the edge on).
//!
//! **Spread** ([`GradientSpread`]) decides what happens outside
//! `t ∈ [0, 1]`: `Pad` clamps, `Repeat` uses `t − ⌊t⌋`, `Reflect` the
//! triangle wave `m = t mod 2`, `t′ = m` for `m ≤ 1` else `2 − m`.
//!
//! **Conic** — `t` is the angle of the pixel around the centre
//! (`center`, default the canvas centre), clockwise from North, as a
//! fraction of a turn, minus the start angle `angle / 360`, wrapped to
//! `[0, 1)`. The centre pixel itself is `t = 0`. The angle comes from
//! [`turn_fraction`], which uses only `+ − × ÷ √` and is therefore
//! bit-identical on every platform and exact on the eight principal
//! directions.
//!
//! **Radial angle** — `angle = θ` rotates the ellipse clockwise: the
//! centre offset `(dx, dy)` is taken in the rotated frame
//! `u = dx·cos θ + dy·sin θ`, `v = dy·cos θ − dx·sin θ` before the
//! radii are applied.
//!
//! **Colour spaces** ([`GradientColorspace`]) — besides `Srgb` and
//! `Linear`, stops can be interpolated in Oklab, CIELAB (D50) or their
//! polar forms. In a polar space the hue travels along the [`HueArc`]
//! (shorter by default), a stop without chroma takes its neighbour's
//! hue, and lightness and chroma — not hue — are premultiplied. The
//! result is clamped to the sRGB gamut in linear light before
//! encoding, and a parameter that falls exactly on a stop returns the
//! stop's colour itself.
//!
//! # Depth and layout
//!
//! [`GradientOptions::field`] yields a [`GradientField`], a
//! [`Field`] that [`canvas::render`] delivers as
//! 8-bit, 16-bit or float samples in grey, grey + alpha, RGB or RGBA,
//! optionally dithered and row-banded over a thread budget. The
//! `render*` functions here are the 8-bit RGBA shorthands.
//!
//! # Parity note
//!
//! ImageMagick 7.1.2 writes the pixel at the gradient vector's start
//! point `(x1, y1)` with the value of its left-hand neighbour
//! `(x1−1, y1)` (visible as e.g. `-define gradient:vector=4,0,0,0`
//! giving `… 64 64` instead of `… 64 0` on a 5-pixel row). That is a
//! one-pixel artefact, not a documented property of the gradient; this
//! module renders the mathematically defined value instead. The
//! `tests/magick_parity.rs` harness accounts for it explicitly.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use oxideav_pixfmt::colorspace::{
    ColorModel, D50, D65, LabWhite, RgbPrimaries, bradford_adaptation, lab_to_xyz, lch_to_lab,
    oklab_to_xyz_d65,
};

use super::Rgba8Image;
use super::canvas::{self, Field, Image, RenderConfig};
use super::palette::{parse_color, parse_color_unit};
use crate::source::{q_f64, q_str, q_u32};

/// Compass direction of a linear gradient (ImageMagick
/// `gradient:direction`). `North` runs bottom → top, `East` left →
/// right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GradientDirection {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl GradientDirection {
    /// Case-insensitive parse of the ImageMagick compass names
    /// (`NorthWest`, `north-west`, `nw`, …) plus this crate's
    /// historical `horizontal` / `vertical` / `diagonal` spellings.
    pub fn parse(s: &str) -> Option<Self> {
        let k: String = s
            .trim()
            .chars()
            .filter(|c| *c != '-' && *c != '_' && !c.is_whitespace())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Some(match k.as_str() {
            "north" | "n" | "bt" | "up" => Self::North,
            "south" | "s" | "vertical" | "v" | "tb" | "down" => Self::South,
            "east" | "e" | "horizontal" | "h" | "lr" | "right" => Self::East,
            "west" | "w" | "rl" | "left" => Self::West,
            "northeast" | "ne" => Self::NorthEast,
            "northwest" | "nw" => Self::NorthWest,
            "southeast" | "se" | "diagonal" | "d" | "diag" => Self::SouthEast,
            "southwest" | "sw" => Self::SouthWest,
            _ => return None,
        })
    }

    /// Canonical ImageMagick spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::North => "North",
            Self::South => "South",
            Self::East => "East",
            Self::West => "West",
            Self::NorthEast => "NorthEast",
            Self::NorthWest => "NorthWest",
            Self::SouthEast => "SouthEast",
            Self::SouthWest => "SouthWest",
        }
    }

    /// The gradient vector `(x1, y1, x2, y2)` this direction denotes on
    /// a `w × h` canvas, in pixel coordinates (corner / edge pixels,
    /// so `w−1` and `h−1`).
    pub fn vector(self, w: u32, h: u32) -> [f64; 4] {
        let r = (w.max(1) - 1) as f64;
        let b = (h.max(1) - 1) as f64;
        match self {
            Self::South => [0.0, 0.0, 0.0, b],
            Self::North => [0.0, b, 0.0, 0.0],
            Self::East => [0.0, 0.0, r, 0.0],
            Self::West => [r, 0.0, 0.0, 0.0],
            Self::SouthEast => [0.0, 0.0, r, b],
            Self::NorthWest => [r, b, 0.0, 0.0],
            Self::NorthEast => [0.0, b, r, 0.0],
            Self::SouthWest => [r, 0.0, 0.0, b],
        }
    }
}

/// Radial-gradient radius rule (ImageMagick `gradient:extent`). See the
/// module docs for the exact radii each one yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GradientExtent {
    Circle,
    Diagonal,
    Ellipse,
    Maximum,
    Minimum,
    /// Circle reaching the border side nearest the centre.
    ClosestSide,
    /// Circle reaching the border side farthest from the centre.
    FarthestSide,
    /// Circle through the canvas corner nearest the centre.
    ClosestCorner,
    /// Circle through the canvas corner farthest from the centre.
    FarthestCorner,
}

impl GradientExtent {
    /// Case-insensitive parse of the ImageMagick names, plus
    /// `closest-side` / `farthest-side` / `closest-corner` /
    /// `farthest-corner` (hyphen, underscore or no separator).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "circle" => Self::Circle,
            "diagonal" => Self::Diagonal,
            "ellipse" => Self::Ellipse,
            "maximum" | "max" => Self::Maximum,
            "minimum" | "min" => Self::Minimum,
            "closest-side" | "closestside" | "closest_side" => Self::ClosestSide,
            "farthest-side" | "farthestside" | "farthest_side" => Self::FarthestSide,
            "closest-corner" | "closestcorner" | "closest_corner" => Self::ClosestCorner,
            "farthest-corner" | "farthestcorner" | "farthest_corner" => Self::FarthestCorner,
            _ => return None,
        })
    }

    /// Canonical ImageMagick spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Circle => "Circle",
            Self::Diagonal => "Diagonal",
            Self::Ellipse => "Ellipse",
            Self::Maximum => "Maximum",
            Self::Minimum => "Minimum",
            Self::ClosestSide => "ClosestSide",
            Self::FarthestSide => "FarthestSide",
            Self::ClosestCorner => "ClosestCorner",
            Self::FarthestCorner => "FarthestCorner",
        }
    }

    /// `(rx, ry)` for a `w × h` canvas with the default (canvas-centre)
    /// radial centre.
    pub fn radii(self, w: u32, h: u32) -> (f64, f64) {
        let hx = (w.max(1) - 1) as f64 / 2.0;
        let hy = (h.max(1) - 1) as f64 / 2.0;
        self.radii_at(w, h, hx, hy)
    }

    /// `(rx, ry)` for a `w × h` canvas and radial centre `(cx, cy)`.
    /// Only the side / corner extents depend on the centre.
    pub fn radii_at(self, w: u32, h: u32, cx: f64, cy: f64) -> (f64, f64) {
        let hx = (w.max(1) - 1) as f64 / 2.0;
        let hy = (h.max(1) - 1) as f64 / 2.0;
        let right = (w.max(1) - 1) as f64;
        let bottom = (h.max(1) - 1) as f64;
        // Unsigned distances to the four sides (the centre may sit
        // outside the canvas).
        let dl = cx.abs();
        let dr = (right - cx).abs();
        let dt = cy.abs();
        let db = (bottom - cy).abs();
        let dist = |a: f64, b: f64| (a * a + b * b).sqrt();
        let corners = [dist(dl, dt), dist(dr, dt), dist(dl, db), dist(dr, db)];
        let circle = |r: f64| (r, r);
        match self {
            Self::Circle | Self::Maximum => circle(hx.max(hy)),
            Self::Minimum => circle(hx.min(hy)),
            Self::Ellipse => (hx, hy),
            Self::Diagonal => circle(dist(hx, hy)),
            Self::ClosestSide => circle(dl.min(dr).min(dt).min(db)),
            Self::FarthestSide => circle(dl.max(dr).max(dt).max(db)),
            Self::ClosestCorner => circle(corners.into_iter().fold(f64::INFINITY, f64::min)),
            Self::FarthestCorner => circle(corners.into_iter().fold(0.0, f64::max)),
        }
    }
}

/// Colour space the stops are interpolated in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GradientColorspace {
    /// Interpolate the stored gamma-encoded sRGB values (ImageMagick's
    /// behaviour; the default).
    #[default]
    Srgb,
    /// Decode through the sRGB transfer function, interpolate in linear
    /// light, re-encode.
    Linear,
    /// Oklab: perceptually even lightness, no hue sweep.
    Oklab,
    /// Oklab in polar form (lightness, chroma, hue): keeps chroma up
    /// between saturated stops; the hue path is set by [`HueArc`].
    Oklch,
    /// CIELAB (D50 white).
    Lab,
    /// CIELAB in polar form, CIE LCh(ab) (D50 white).
    Lch,
}

impl GradientColorspace {
    /// `srgb` (default), `rgb` / `linear` (ImageMagick calls the
    /// linear-light space `RGB`), `oklab`, `oklch`, `lab`, `lch`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "srgb" | "gamma" => Self::Srgb,
            "rgb" | "linear" | "linear-rgb" | "linearrgb" | "linear_rgb" | "srgb-linear" => {
                Self::Linear
            }
            "oklab" => Self::Oklab,
            "oklch" => Self::Oklch,
            "lab" | "cielab" => Self::Lab,
            "lch" | "lchab" | "cielch" => Self::Lch,
            _ => return None,
        })
    }

    /// The third component is a hue angle.
    fn is_polar(self) -> bool {
        matches!(self, Self::Oklch | Self::Lch)
    }
}

/// Shape of the gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GradientKind {
    /// Colour varies along a line.
    #[default]
    Linear,
    /// Colour varies with the (elliptical) distance from a centre.
    Radial,
    /// Colour varies with the angle around a centre.
    Conic,
}

impl GradientKind {
    /// `linear`, `radial` (also `radial-gradient`), `conic` (also
    /// `conical`, `angular`, `sweep`).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "linear" | "gradient" => Self::Linear,
            "radial" | "radial-gradient" | "radial_gradient" => Self::Radial,
            "conic" | "conical" | "angular" | "sweep" | "conic-gradient" | "conic_gradient" => {
                Self::Conic
            }
            _ => return None,
        })
    }
}

/// What the ramp does outside `t ∈ [0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GradientSpread {
    /// Hold the end colours (`t` clamped).
    #[default]
    Pad,
    /// Start over: `t − ⌊t⌋` (a `t` already inside `[0, 1]` is kept,
    /// so `t = 1` still shows the last colour).
    Repeat,
    /// Run back and forth: the triangle wave of period 2.
    Reflect,
}

impl GradientSpread {
    /// `pad` / `clamp`, `repeat`, `reflect` / `mirror`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "pad" | "clamp" | "none" => Self::Pad,
            "repeat" | "tile" => Self::Repeat,
            "reflect" | "mirror" => Self::Reflect,
            _ => return None,
        })
    }
}

/// Which way round the hue circle a polar space ([`GradientColorspace::Oklch`],
/// [`GradientColorspace::Lch`]) travels between two stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HueArc {
    /// The arc of at most 180°.
    #[default]
    Shorter,
    /// The arc of at least 180°.
    Longer,
    /// Always towards larger angles.
    Increasing,
    /// Always towards smaller angles.
    Decreasing,
}

impl HueArc {
    /// `shorter`, `longer`, `increasing`, `decreasing`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "shorter" | "short" => Self::Shorter,
            "longer" | "long" => Self::Longer,
            "increasing" => Self::Increasing,
            "decreasing" => Self::Decreasing,
            _ => return None,
        })
    }

    /// The end hue (degrees, possibly outside `[0, 360)`) to
    /// interpolate towards linearly from `h0` so the path follows this
    /// arc to `h1`.
    fn resolve(self, h0: f64, h1: f64) -> f64 {
        // Difference folded into [0, 360).
        let up = (h1 - h0).rem_euclid(360.0);
        let d = match self {
            Self::Shorter => {
                if up > 180.0 {
                    up - 360.0
                } else {
                    up
                }
            }
            Self::Longer => {
                if up > 0.0 && up < 180.0 {
                    up - 360.0
                } else if up == 0.0 {
                    360.0
                } else {
                    up
                }
            }
            Self::Increasing => up,
            Self::Decreasing => {
                if up == 0.0 {
                    0.0
                } else {
                    up - 360.0
                }
            }
        };
        h0 + d
    }
}

/// One colour stop of a multi-stop gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct GradientStop {
    /// Position along the ramp (`0` = start, `1` = end); `None` lets
    /// [`GradientOptions::resolved_stops`] place it.
    pub offset: Option<f64>,
    /// Straight-alpha sRGB-encoded `[r, g, b, a]`, each in `[0, 1]`.
    pub color: [f64; 4],
}

impl GradientStop {
    /// A stop of the given colour with an automatic position.
    pub fn new(color: [f64; 4]) -> Self {
        Self {
            offset: None,
            color,
        }
    }

    /// A stop from an 8-bit RGBA colour.
    pub fn from_rgba8(color: [u8; 4]) -> Self {
        Self::new(color.map(|v| f64::from(v) / 255.0))
    }

    /// Fix the position.
    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Parse `colour`, `colour 25%`, `colour 0.25` or `colour@0.25`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let number = |tok: &str| -> Option<f64> {
            match tok.strip_suffix('%') {
                Some(p) => p.trim().parse::<f64>().ok().map(|v| v / 100.0),
                None => tok.parse::<f64>().ok(),
            }
        };
        // The offset is the text after the last `@` or space that sits
        // outside any parentheses.
        let mut depth = 0i32;
        let mut split = None;
        for (i, b) in s.bytes().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'@' | b' ' if depth <= 0 => split = Some(i),
                _ => {}
            }
        }
        if let Some(i) = split
            && let Some(offset) = number(s[i + 1..].trim())
        {
            return Ok(Self::new(parse_color_unit(&s[..i])?).with_offset(offset));
        }
        Ok(Self::new(parse_color_unit(s)?))
    }

    /// Parse a `,` or `;` separated stop list (separators inside
    /// parentheses belong to the colour).
    pub fn parse_list(s: &str) -> Result<Vec<Self>> {
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut start = 0;
        for (i, b) in s.bytes().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' | b';' if depth <= 0 => {
                    if !s[start..i].trim().is_empty() {
                        out.push(Self::parse(&s[start..i])?);
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        if !s[start..].trim().is_empty() {
            out.push(Self::parse(&s[start..])?);
        }
        if out.is_empty() {
            return Err(Error::invalid("gradient: empty stop list"));
        }
        Ok(out)
    }
}

/// Rectangle the gradient is confined to (`gradient:bounding-box`,
/// `WxH+X+Y` geometry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BoundingBox {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
}

impl BoundingBox {
    pub fn new(width: u32, height: u32, x: u32, y: u32) -> Self {
        Self {
            width,
            height,
            x,
            y,
        }
    }

    /// Parse `WxH`, `WxH+X+Y`. Negative offsets are rejected (the
    /// `magick` binary errors out on them too).
    pub fn parse(s: &str) -> Result<Self> {
        let bad = || Error::invalid(format!("gradient: bounding-box {s:?} is not WxH+X+Y"));
        // An unescaped `+` in a query string decodes to a space
        // (form-encoding), so `bbox=3x3+1+1` arrives as `3x3 1 1`;
        // treat interior spaces as `+` separators.
        let owned = s.trim().replace(' ', "+");
        let s = owned.as_str();
        let (size, offs) = match s.find(['+', '-']) {
            Some(i) => (&s[..i], &s[i..]),
            None => (s, ""),
        };
        let (w, h) = size.split_once(['x', 'X']).ok_or_else(bad)?;
        let width: u32 = w.trim().parse().map_err(|_| bad())?;
        let height: u32 = h.trim().parse().map_err(|_| bad())?;
        let (x, y) = if offs.is_empty() {
            (0, 0)
        } else {
            let o = offs.strip_prefix('+').ok_or_else(|| {
                Error::invalid(format!(
                    "gradient: bounding-box {s:?} has a negative offset"
                ))
            })?;
            let (xs, ys) = o.split_once(['+', '-']).ok_or_else(bad)?;
            if ys.contains('-') || o.contains('-') {
                return Err(Error::invalid(format!(
                    "gradient: bounding-box {s:?} has a negative offset"
                )));
            }
            (
                xs.trim().parse().map_err(|_| bad())?,
                ys.trim().parse().map_err(|_| bad())?,
            )
        };
        Ok(Self::new(width, height, x, y))
    }
}

/// Everything that shapes a two-stop gradient. Build with
/// [`GradientOptions::new`] + the `with_*` setters, or from a
/// `generate://gradient?…` query via [`GradientOptions::from_query`],
/// then hand to [`render_linear`] / [`render_radial`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct GradientOptions {
    pub width: u32,
    pub height: u32,
    /// First stop (`t = 0`). Default white, like ImageMagick's bare
    /// `gradient:`.
    pub from: [u8; 4],
    /// Second stop (`t = 1`). Default black.
    pub to: [u8; 4],
    pub direction: Option<GradientDirection>,
    /// Degrees, clockwise from North.
    pub angle: Option<f64>,
    /// `(x1, y1, x2, y2)` in pixel coordinates.
    pub vector: Option<[f64; 4]>,
    pub extent: Option<GradientExtent>,
    pub bounding_box: Option<BoundingBox>,
    /// Radial centre override.
    pub center: Option<(f64, f64)>,
    /// Radial radii override `(rx, ry)`; `ry = None` keeps the extent's
    /// vertical radius.
    pub radii: Option<(f64, Option<f64>)>,
    pub colorspace: GradientColorspace,
    /// Explicit colour stops; when non-empty they replace `from` /
    /// `to`.
    pub stops: Vec<GradientStop>,
    pub spread: GradientSpread,
    /// Hue path in the polar colour spaces.
    pub hue: HueArc,
}

impl GradientOptions {
    /// `w × h` white → black, South, no overrides.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            from: [255, 255, 255, 255],
            to: [0, 0, 0, 255],
            direction: None,
            angle: None,
            vector: None,
            extent: None,
            bounding_box: None,
            center: None,
            radii: None,
            colorspace: GradientColorspace::Srgb,
            stops: Vec::new(),
            spread: GradientSpread::Pad,
            hue: HueArc::Shorter,
        }
    }

    /// Replace `from` / `to` by an explicit stop list (two or more
    /// colours, optionally positioned).
    pub fn with_stops(mut self, stops: Vec<GradientStop>) -> Self {
        self.stops = stops;
        self
    }

    pub fn with_spread(mut self, spread: GradientSpread) -> Self {
        self.spread = spread;
        self
    }

    pub fn with_hue_arc(mut self, hue: HueArc) -> Self {
        self.hue = hue;
        self
    }

    pub fn with_colors(mut self, from: [u8; 4], to: [u8; 4]) -> Self {
        self.from = from;
        self.to = to;
        self
    }

    pub fn with_direction(mut self, direction: GradientDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    pub fn with_angle(mut self, degrees: f64) -> Self {
        self.angle = Some(degrees);
        self
    }

    pub fn with_vector(mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        self.vector = Some([x1, y1, x2, y2]);
        self
    }

    pub fn with_extent(mut self, extent: GradientExtent) -> Self {
        self.extent = Some(extent);
        self
    }

    pub fn with_bounding_box(mut self, bbox: BoundingBox) -> Self {
        self.bounding_box = Some(bbox);
        self
    }

    pub fn with_center(mut self, x: f64, y: f64) -> Self {
        self.center = Some((x, y));
        self
    }

    pub fn with_radii(mut self, rx: f64, ry: Option<f64>) -> Self {
        self.radii = Some((rx, ry));
        self
    }

    pub fn with_colorspace(mut self, colorspace: GradientColorspace) -> Self {
        self.colorspace = colorspace;
        self
    }

    /// Read the `generate://gradient?…` query form:
    /// `w`, `h`, `from`, `to`, `direction`, `angle`, `vector=x1,y1,x2,y2`,
    /// `extent`, `bbox` (aliases `bounding-box`, `bounding_box`,
    /// `bounding-box=WxH+X+Y`), `center=x,y`, `radii=rx[,ry]`,
    /// `colorspace=srgb|rgb|linear`. `type` is left to the caller.
    pub fn from_query(query: &BTreeMap<String, String>) -> Result<Self> {
        let w = q_u32(query, "w", 640)?.max(1);
        let h = q_u32(query, "h", 480)?.max(1);
        let mut opts = Self::new(w, h).with_colors(
            parse_color(q_str(query, "from", "white"))?,
            parse_color(q_str(query, "to", "black"))?,
        );
        if let Some(d) = query.get("direction") {
            opts.direction = Some(GradientDirection::parse(d).ok_or_else(|| {
                Error::invalid(format!(
                    "gradient: unknown direction {d:?} (expected North|South|East|West|NorthEast|NorthWest|SouthEast|SouthWest, or horizontal|vertical|diagonal)"
                ))
            })?);
        }
        if query.contains_key("angle") {
            opts.angle = Some(q_f64(query, "angle", 0.0)?);
        }
        if let Some(v) = query.get("vector") {
            let nums = parse_f64_list(v, "vector")?;
            if nums.len() != 4 {
                return Err(Error::invalid(format!(
                    "gradient: vector {v:?} must be x1,y1,x2,y2"
                )));
            }
            opts.vector = Some([nums[0], nums[1], nums[2], nums[3]]);
        }
        if let Some(e) = query.get("extent") {
            opts.extent = Some(GradientExtent::parse(e).ok_or_else(|| {
                Error::invalid(format!(
                    "gradient: unknown extent {e:?} (expected Circle|Diagonal|Ellipse|Maximum|Minimum|closest-side|farthest-side|closest-corner|farthest-corner)"
                ))
            })?);
        }
        if let Some(b) = query
            .get("bbox")
            .or_else(|| query.get("bounding-box"))
            .or_else(|| query.get("bounding_box"))
        {
            opts.bounding_box = Some(BoundingBox::parse(b)?);
        }
        if let Some(c) = query.get("center") {
            let nums = parse_f64_list(c, "center")?;
            if nums.len() != 2 {
                return Err(Error::invalid(format!(
                    "gradient: center {c:?} must be x,y"
                )));
            }
            opts.center = Some((nums[0], nums[1]));
        }
        if let Some(r) = query.get("radii") {
            let nums = parse_f64_list(r, "radii")?;
            match nums.len() {
                1 => opts.radii = Some((nums[0], None)),
                2 => opts.radii = Some((nums[0], Some(nums[1]))),
                _ => {
                    return Err(Error::invalid(format!(
                        "gradient: radii {r:?} must be rx or rx,ry"
                    )));
                }
            }
        }
        if let Some(cs) = query.get("colorspace") {
            opts.colorspace = GradientColorspace::parse(cs).ok_or_else(|| {
                Error::invalid(format!(
                    "gradient: unknown colorspace {cs:?} (expected srgb|rgb|linear|oklab|oklch|lab|lch)"
                ))
            })?;
        }
        if let Some(list) = query.get("stops") {
            opts.stops = GradientStop::parse_list(list)?;
        } else {
            // Colours written with more than 8 bits (16-bit hex) keep
            // their precision as an explicit two-stop list.
            let from = parse_color_unit(q_str(query, "from", "white"))?;
            let to = parse_color_unit(q_str(query, "to", "black"))?;
            let unit = |c: [u8; 4]| c.map(|v| f64::from(v) / 255.0);
            if from != unit(opts.from) || to != unit(opts.to) {
                opts.stops = vec![GradientStop::new(from), GradientStop::new(to)];
            }
        }
        if let Some(sp) = query.get("spread") {
            opts.spread = GradientSpread::parse(sp).ok_or_else(|| {
                Error::invalid(format!(
                    "gradient: unknown spread {sp:?} (expected pad|repeat|reflect)"
                ))
            })?;
        }
        if let Some(hue) = query.get("hue") {
            opts.hue = HueArc::parse(hue).ok_or_else(|| {
                Error::invalid(format!(
                    "gradient: unknown hue arc {hue:?} (expected shorter|longer|increasing|decreasing)"
                ))
            })?;
        }
        Ok(opts)
    }

    /// Validate the bounding box against the canvas (ImageMagick
    /// refuses a box that does not fit).
    fn checked_bbox(&self) -> Result<Option<BoundingBox>> {
        match self.bounding_box {
            None => Ok(None),
            Some(b) => {
                let fits = b.width >= 1
                    && b.height >= 1
                    && b.x.checked_add(b.width).is_some_and(|r| r <= self.width)
                    && b.y
                        .checked_add(b.height)
                        .is_some_and(|btm| btm <= self.height);
                if !fits {
                    return Err(Error::invalid(format!(
                        "gradient: bounding-box {}x{}+{}+{} does not fit inside the {}x{} canvas",
                        b.width, b.height, b.x, b.y, self.width, self.height
                    )));
                }
                Ok(Some(b))
            }
        }
    }

    /// The linear gradient's effective vector, or `None` for angle mode.
    fn linear_vector(&self) -> Option<[f64; 4]> {
        if let Some(v) = self.vector {
            return Some(v);
        }
        if self.angle.is_some() {
            return None;
        }
        let dir = match self.direction {
            Some(d) => d,
            None if self.extent.is_some() => GradientDirection::SouthEast,
            None if self.height == 1 => GradientDirection::East,
            None => GradientDirection::South,
        };
        Some(dir.vector(self.width, self.height))
    }

    /// Radial centre per the precedence in the module docs.
    fn radial_center(&self) -> (f64, f64) {
        if let Some(c) = self.center {
            return c;
        }
        let v = self
            .vector
            .or_else(|| self.direction.map(|d| d.vector(self.width, self.height)));
        match v {
            Some([_, _, x2, y2]) => (x2 / 2.0, y2 / 2.0),
            None => (
                (self.width - 1) as f64 / 2.0,
                (self.height - 1) as f64 / 2.0,
            ),
        }
    }

    /// Radial radii per the precedence in the module docs.
    fn radial_radii(&self) -> (f64, f64) {
        let (cx, cy) = self.radial_center();
        let (ex, ey) =
            self.extent
                .unwrap_or(GradientExtent::Circle)
                .radii_at(self.width, self.height, cx, cy);
        match self.radii {
            Some((rx, Some(ry))) => (rx, ry),
            Some((rx, None)) => (rx, ey),
            None => (ex, ey),
        }
    }
}

fn parse_f64_list(s: &str, what: &str) -> Result<Vec<f64>> {
    s.split([',', 'x', ' '])
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            p.trim().parse::<f64>().map_err(|_| {
                Error::invalid(format!(
                    "gradient: {what} {s:?} must be a comma-separated list of numbers"
                ))
            })
        })
        .collect()
}

/// `generate://gradient?…` entry point: `type=linear` (default),
/// `type=radial` or `type=conic`, every other key per
/// [`GradientOptions::from_query`]. 8-bit RGBA; see [`render_image`]
/// for the other depths and layouts.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let opts = GradientOptions::from_query(query)?;
    let kind = kind_from_query(query)?;
    Ok(canvas::render(&opts.field(kind)?, &RenderConfig::new()).into_rgba8())
}

/// Like [`render`], with the output shaped by the query's `depth=`,
/// `channels=`, `dither=` and `threads=` keys
/// ([`RenderConfig::from_query`]).
pub fn render_image(query: &BTreeMap<String, String>) -> Result<Image> {
    let opts = GradientOptions::from_query(query)?;
    let kind = kind_from_query(query)?;
    let cfg = RenderConfig::from_query(query)?;
    Ok(canvas::render(&opts.field(kind)?, &cfg))
}

fn kind_from_query(query: &BTreeMap<String, String>) -> Result<GradientKind> {
    let t = q_str(query, "type", "linear");
    GradientKind::parse(t).ok_or_else(|| {
        Error::invalid(format!(
            "gradient: unknown type {t:?} (expected linear|radial|conic)"
        ))
    })
}

/// Render a linear gradient as 8-bit RGBA.
pub fn render_linear(opts: &GradientOptions) -> Result<Rgba8Image> {
    Ok(canvas::render(&opts.field(GradientKind::Linear)?, &RenderConfig::new()).into_rgba8())
}

/// Render a radial gradient as 8-bit RGBA.
pub fn render_radial(opts: &GradientOptions) -> Result<Rgba8Image> {
    Ok(canvas::render(&opts.field(GradientKind::Radial)?, &RenderConfig::new()).into_rgba8())
}

/// Render a conic gradient as 8-bit RGBA.
pub fn render_conic(opts: &GradientOptions) -> Result<Rgba8Image> {
    Ok(canvas::render(&opts.field(GradientKind::Conic)?, &RenderConfig::new()).into_rgba8())
}

/// Where the gradient parameter `t` comes from.
#[derive(Clone, Copy, Debug)]
enum Geometry {
    /// `t = ((p − p1) · d) / |d|²`.
    Vector {
        x1: f64,
        y1: f64,
        dx: f64,
        dy: f64,
        len2: f64,
    },
    /// `t = (p · d − min) / range`.
    Angle {
        dx: f64,
        dy: f64,
        min: f64,
        range: f64,
    },
    /// `t = √((u/rx)² + (v/ry)²)`, `(u, v)` the centre offset, rotated
    /// by `(cos, sin)` when `rotated`.
    Radial {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        cos: f64,
        sin: f64,
        rotated: bool,
    },
    /// `t = frac(turn(p − c) − start)`.
    Conic { cx: f64, cy: f64, start: f64 },
}

/// `d / r` with the zero-radius step rule.
#[inline]
fn radial_norm(d: f64, r: f64) -> f64 {
    if r > 0.0 {
        d / r
    } else if d == 0.0 {
        0.0
    } else {
        f64::INFINITY
    }
}

impl Geometry {
    /// The part of `t` that only depends on the row, hoisted out of the
    /// pixel loop. The per-pixel expression is unchanged by the
    /// hoisting (same operations, same order).
    #[inline]
    fn row_term(&self, y: u32) -> f64 {
        let py = f64::from(y);
        match *self {
            Geometry::Vector { y1, dy, .. } => (py - y1) * dy,
            Geometry::Angle { dy, .. } => py * dy,
            Geometry::Radial { cy, .. } | Geometry::Conic { cy, .. } => py - cy,
        }
    }

    /// Gradient parameter of pixel `x` on a row whose
    /// [`Geometry::row_term`] is `row`.
    #[inline]
    fn t(&self, x: u32, row: f64) -> f64 {
        let px = f64::from(x);
        match *self {
            // Plain division (not a cached reciprocal) keeps exact-half
            // cases on the documented side.
            Geometry::Vector { x1, dx, len2, .. } => {
                if len2 > 0.0 {
                    ((px - x1) * dx + row) / len2
                } else {
                    0.0
                }
            }
            Geometry::Angle { dx, min, range, .. } => {
                if range > 0.0 {
                    (px * dx + row - min) / range
                } else {
                    0.0
                }
            }
            Geometry::Radial {
                cx,
                rx,
                ry,
                cos,
                sin,
                rotated,
                ..
            } => {
                let (u, v) = if rotated {
                    let ox = px - cx;
                    (ox * cos + row * sin, row * cos - ox * sin)
                } else {
                    (px - cx, row)
                };
                let nx = radial_norm(u, rx);
                let ny = radial_norm(v, ry);
                (nx * nx + ny * ny).sqrt()
            }
            Geometry::Conic { cx, start, .. } => {
                // Clockwise from North: North is −y, East is +x.
                let t = turn_fraction(-row, px - cx) - start;
                t - t.floor()
            }
        }
    }
}

/// Angle of the vector `(x, y)` as a fraction of a full turn in
/// `[0, 1)`, measured from `+x` towards `+y`; `0` for the zero vector.
///
/// Built from `+ − × ÷ √` only, so the result is the same on every
/// platform (no dependence on the C library's `atan2`). The eight
/// principal directions are exact multiples of `1/8`. Elsewhere the
/// octant-reduced ratio `z ∈ (0, 1)` goes through two half-angle steps
/// `z ← z / (1 + √(1 + z²))` (so `atan z = 4·atan z″` with
/// `z″ < tan(π/16) ≈ 0.199`) and the odd Maclaurin series to `z″²¹`,
/// whose truncation error is below `10⁻¹⁷`.
#[doc(hidden)]
pub fn turn_fraction(x: f64, y: f64) -> f64 {
    let ax = x.abs();
    let ay = y.abs();
    if ax == 0.0 && ay == 0.0 {
        return 0.0;
    }
    let (lo, hi) = if ay <= ax { (ay, ax) } else { (ax, ay) };
    let octant = if lo == hi {
        0.125
    } else if lo == 0.0 {
        0.0
    } else {
        let half = |z: f64| z / (1.0 + (1.0 + z * z).sqrt());
        let z = half(half(lo / hi));
        let z2 = z * z;
        // Horner form of z − z³/3 + z⁵/5 − … + z²¹/21.
        const RECIPROCALS: [f64; 10] = [
            1.0 / 19.0,
            1.0 / 17.0,
            1.0 / 15.0,
            1.0 / 13.0,
            1.0 / 11.0,
            1.0 / 9.0,
            1.0 / 7.0,
            1.0 / 5.0,
            1.0 / 3.0,
            1.0,
        ];
        let mut s = 1.0 / 21.0;
        for r in RECIPROCALS {
            s = r - z2 * s;
        }
        // atan(lo/hi) = 4·z·s radians; one turn is 2π.
        (4.0 * z * s) / std::f64::consts::TAU
    };
    let quadrant = if ay <= ax { octant } else { 0.25 - octant };
    match (x < 0.0, y < 0.0) {
        (false, false) => quadrant,
        (true, false) => 0.5 - quadrant,
        (true, true) => 0.5 + quadrant,
        (false, true) => {
            if quadrant == 0.0 {
                0.0
            } else {
                1.0 - quadrant
            }
        }
    }
}

/// One piece of the colour ramp between two consecutive stops.
#[derive(Clone, Copy, Debug)]
struct Segment {
    o0: f64,
    o1: f64,
    /// Interpolation-space components at `o0`, premultiplied by alpha
    /// (hue excepted), `[c0, c1, c2, alpha]`.
    c0: [f64; 4],
    /// The stops' own straight encoded colours, returned verbatim at
    /// the segment ends in the perceptual spaces.
    s0: [f64; 4],
    s1: [f64; 4],
    /// `c1 − c0`, component by component.
    delta: [f64; 4],
    /// Both ends have alpha exactly 1: the interpolated alpha is then
    /// exactly 1 and un-premultiplying is the identity, so it is
    /// skipped.
    opaque: bool,
}

/// Outcome of locating a gradient parameter on the ramp.
enum Located {
    /// A stop's own colour (straight, encoded).
    Stop([f64; 4]),
    /// Straight interpolation-space components and alpha.
    Mix([f64; 3], f64),
}

type Mat3 = [[f64; 3]; 3];

fn mat3_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    m
}

/// Inverse by the adjugate (the matrices here are well conditioned).
fn mat3_inverse(m: &Mat3) -> Mat3 {
    let c = |i: usize, j: usize| {
        let (i1, i2) = ((i + 1) % 3, (i + 2) % 3);
        let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
        m[i1][j1] * m[i2][j2] - m[i1][j2] * m[i2][j1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    let mut inv = [[0.0; 3]; 3];
    for (i, row) in inv.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = c(j, i) / det;
        }
    }
    inv
}

#[inline]
fn mat3_apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// The colour ramp: stops resolved into segments in the interpolation
/// space, plus what is needed to come back to encoded sRGB.
#[derive(Clone, Debug)]
struct ColorRamp {
    space: GradientColorspace,
    spread: GradientSpread,
    segments: Vec<Segment>,
    /// XYZ → linear sRGB (from D65 XYZ for Oklab, from D50 XYZ for
    /// CIELAB, the chromatic adaptation folded in).
    xyz_to_linear: Mat3,
    /// D50 white, for CIELAB.
    white: [f64; 3],
    /// Colour painted outside the bounding box.
    outside: [f64; 4],
    /// The third component is a hue (not premultiplied).
    polar: bool,
    /// A parameter on a stop returns the stop's own colour.
    exact_ends: bool,
}

impl ColorRamp {
    fn new(opts: &GradientOptions) -> Result<Self> {
        let space = opts.colorspace;
        let stops = opts.resolved_stops()?;
        let model = match space {
            GradientColorspace::Srgb => ColorModel::Srgb,
            GradientColorspace::Linear => ColorModel::LinearSrgb,
            GradientColorspace::Oklab => ColorModel::Oklab,
            GradientColorspace::Oklch => ColorModel::Oklch,
            GradientColorspace::Lab => ColorModel::Lab(LabWhite::D50),
            GradientColorspace::Lch => ColorModel::Lch(LabWhite::D50),
        };
        let polar = space.is_polar();
        // Straight components in the interpolation space. Srgb and
        // Linear keep their historical arithmetic exactly.
        let convert = |c: [f64; 4]| -> [f64; 3] {
            match space {
                GradientColorspace::Srgb => [c[0], c[1], c[2]],
                GradientColorspace::Linear => [
                    srgb_to_linear(c[0]),
                    srgb_to_linear(c[1]),
                    srgb_to_linear(c[2]),
                ],
                _ => model.from_srgb([c[0], c[1], c[2]]),
            }
        };
        let mut comps: Vec<[f64; 3]> = stops.iter().map(|s| convert(s.1)).collect();
        if polar {
            // A colour with no chroma has no hue of its own: it takes
            // its neighbour's, so the ramp does not sweep through
            // unrelated hues on the way to grey.
            let powerless: Vec<bool> = comps.iter().map(|c| c[1] < 1e-6).collect();
            for i in 0..comps.len() {
                if powerless[i] {
                    let donor = (i + 1..comps.len())
                        .find(|j| !powerless[*j])
                        .or_else(|| (0..i).rev().find(|j| !powerless[*j]));
                    comps[i][2] = donor.map_or(0.0, |j| comps[j][2]);
                }
            }
        }
        let premultiply = |c: [f64; 3], a: f64| -> [f64; 4] {
            if polar {
                [c[0] * a, c[1] * a, c[2], a]
            } else {
                [c[0] * a, c[1] * a, c[2] * a, a]
            }
        };
        // A fully transparent stop reads back as transparent black,
        // like every other fully transparent pixel of the ramp.
        let stops: Vec<(f64, [f64; 4])> = stops
            .into_iter()
            .map(|(o, c)| (o, if c[3] > 0.0 { c } else { [0.0; 4] }))
            .collect();
        let mut segments = Vec::with_capacity(stops.len().saturating_sub(1).max(1));
        for i in 0..stops.len().saturating_sub(1) {
            let (o0, s0) = stops[i];
            let (o1, s1) = stops[i + 1];
            let c0 = premultiply(comps[i], s0[3]);
            let mut c1 = premultiply(comps[i + 1], s1[3]);
            if polar {
                c1[2] = opts.hue.resolve(c0[2], c1[2]);
            }
            segments.push(Segment {
                o0,
                o1,
                c0,
                s0,
                s1,
                delta: [c1[0] - c0[0], c1[1] - c0[1], c1[2] - c0[2], c1[3] - c0[3]],
                opaque: c0[3] == 1.0 && c1[3] == 1.0,
            });
        }
        if segments.is_empty() {
            // A single stop: one flat segment.
            let (o, s) = stops[0];
            let c = premultiply(comps[0], s[3]);
            segments.push(Segment {
                o0: o,
                o1: o,
                c0: c,
                s0: s,
                s1: s,
                delta: [0.0; 4],
                opaque: c[3] == 1.0,
            });
        }
        let rgb_to_xyz = RgbPrimaries::BT709.rgb_to_xyz();
        let (xyz_to_linear, white) = match space {
            GradientColorspace::Lab | GradientColorspace::Lch => {
                let d50 = D50.to_xyz();
                let adapt = bradford_adaptation(D65.to_xyz(), d50);
                (mat3_inverse(&mat3_mul(&adapt, &rgb_to_xyz)), d50)
            }
            _ => (mat3_inverse(&rgb_to_xyz), D65.to_xyz()),
        };
        let opaque = stops.iter().all(|s| s.1[3] >= 1.0);
        let first = stops[0].1;
        let outside = [first[0], first[1], first[2], if opaque { 1.0 } else { 0.0 }];
        Ok(Self {
            space,
            spread: opts.spread,
            segments,
            xyz_to_linear,
            white,
            outside,
            polar,
            exact_ends: !matches!(space, GradientColorspace::Srgb | GradientColorspace::Linear),
        })
    }

    /// Interpolation-space components → linear-light sRGB (clamped to
    /// the gamut for the perceptual spaces). Not used for `Srgb`.
    #[inline]
    fn to_linear(&self, v: [f64; 3]) -> [f64; 3] {
        let from_xyz = |xyz: [f64; 3]| {
            let lin = mat3_apply(&self.xyz_to_linear, xyz);
            [
                lin[0].clamp(0.0, 1.0),
                lin[1].clamp(0.0, 1.0),
                lin[2].clamp(0.0, 1.0),
            ]
        };
        match self.space {
            GradientColorspace::Srgb | GradientColorspace::Linear => v,
            GradientColorspace::Oklab => from_xyz(oklab_to_xyz_d65(v)),
            GradientColorspace::Oklch => from_xyz(oklab_to_xyz_d65(lch_to_lab(v))),
            GradientColorspace::Lab => from_xyz(lab_to_xyz(v, self.white)),
            GradientColorspace::Lch => from_xyz(lab_to_xyz(lch_to_lab(v), self.white)),
        }
    }

    /// Interpolation-space components → straight encoded sRGB.
    #[inline]
    fn decode(&self, v: [f64; 3]) -> [f64; 3] {
        if self.space == GradientColorspace::Srgb {
            return v;
        }
        let lin = self.to_linear(v);
        [
            linear_to_srgb(lin[0]),
            linear_to_srgb(lin[1]),
            linear_to_srgb(lin[2]),
        ]
    }

    /// `Some((c0, delta))` when the ramp is two opaque colours over
    /// `[0, 1]`, padded, in the sRGB space: the colour at `t` is then
    /// exactly `c0 + delta · clamp(t, 0, 1)` per component with alpha
    /// 1, which is what [`ColorRamp::locate`] computes for it.
    #[inline]
    fn simple(&self) -> Option<([f64; 4], [f64; 4])> {
        let seg = self.segments.first()?;
        (self.segments.len() == 1
            && self.space == GradientColorspace::Srgb
            && self.spread == GradientSpread::Pad
            && seg.opaque
            && seg.o0 == 0.0
            && seg.o1 == 1.0)
            .then_some((seg.c0, seg.delta))
    }

    /// Apply the spread method and find the colour at `t`, short of
    /// the conversion back to encoded sRGB.
    #[inline]
    fn locate(&self, t: f64) -> Located {
        let t = if t.is_nan() {
            0.0
        } else {
            match self.spread {
                GradientSpread::Pad => t.clamp(0.0, 1.0),
                GradientSpread::Repeat => {
                    if (0.0..=1.0).contains(&t) {
                        t
                    } else {
                        t - t.floor()
                    }
                }
                GradientSpread::Reflect => {
                    let m = t - 2.0 * (t / 2.0).floor();
                    if m > 1.0 { 2.0 - m } else { m }
                }
            }
        };
        let last = self.segments.len() - 1;
        let mut i = 0;
        while i < last && t >= self.segments[i].o1 {
            i += 1;
        }
        let seg = &self.segments[i];
        let u = if t >= seg.o1 {
            if self.exact_ends || seg.o1 <= seg.o0 {
                return Located::Stop(seg.s1);
            }
            1.0
        } else if t <= seg.o0 {
            if self.exact_ends {
                return Located::Stop(seg.s0);
            }
            0.0
        } else if seg.o0 == 0.0 && seg.o1 == 1.0 {
            t
        } else {
            (t - seg.o0) / (seg.o1 - seg.o0)
        };
        let mix = |i: usize| seg.c0[i] + seg.delta[i] * u;
        if seg.opaque {
            return Located::Mix([mix(0), mix(1), mix(2)], 1.0);
        }
        let a = mix(3);
        if a > 0.0 {
            let hue_or_scaled = if self.polar { mix(2) } else { mix(2) / a };
            Located::Mix([mix(0) / a, mix(1) / a, hue_or_scaled], a)
        } else {
            Located::Stop([0.0, 0.0, 0.0, a])
        }
    }

    /// Straight encoded colour at gradient parameter `t`.
    #[inline]
    fn at(&self, t: f64) -> [f64; 4] {
        match self.locate(t) {
            Located::Stop(c) => c,
            Located::Mix(v, a) => {
                let rgb = self.decode(v);
                [rgb[0], rgb[1], rgb[2], a]
            }
        }
    }

    /// The colour at `t` quantised to 16 bits: the same value as
    /// quantising [`ColorRamp::at`], with the transfer function and the
    /// rounding folded into one table lookup per component.
    #[inline]
    fn at_q16(&self, t: f64, table: &EncodeTable) -> [u16; 4] {
        match self.locate(t) {
            Located::Stop(c) => c.map(canvas::unit_to_q16),
            Located::Mix(v, a) => {
                let lin = self.to_linear(v);
                [
                    table.encode(lin[0]),
                    table.encode(lin[1]),
                    table.encode(lin[2]),
                    canvas::unit_to_q16(a),
                ]
            }
        }
    }
}

/// `v ↦ round(65535 · clamp(linear_to_srgb(v), 0, 1))` as a table.
///
/// The map is a non-decreasing step function, so it is described
/// completely by the 65535 positions where it steps up. Each position
/// is located to the exact `f64` with the reference function itself
/// (a closed-form first guess, then a walk over neighbouring doubles),
/// which makes a lookup return precisely what the reference function
/// returns — without evaluating a power per sample.
struct EncodeTable {
    /// `thresholds[k]`: the least `v` whose code is at least `k`
    /// (`thresholds[0]` is unused; entries past 65535 are `+∞`
    /// sentinels so a lookup never needs a bounds branch).
    thresholds: Vec<f64>,
    /// `coarse[b]`: the code of `v = b / 2¹⁶`.
    coarse: Vec<u16>,
    /// `fine[b]`: the code of `v = b / 2²⁰`, for the steep dark end
    /// `v < 1/16`.
    fine: Vec<u16>,
    /// The most codes any bucket spans: how many thresholds a lookup
    /// has to test after reading its bucket's starting code.
    max_steps: usize,
}

impl EncodeTable {
    /// Below this the fine buckets are used.
    const FINE_LIMIT: f64 = 1.0 / 16.0;
    /// Lookups test this many thresholds without branching; a table
    /// whose buckets span more falls back to a loop for the rest.
    const UNROLLED_STEPS: usize = 3;

    /// The reference function.
    #[inline]
    fn reference(v: f64) -> u16 {
        canvas::unit_to_q16(linear_to_srgb(v))
    }

    fn build() -> Self {
        let mut thresholds = vec![f64::INFINITY; 65536 + 8];
        thresholds[0] = f64::NEG_INFINITY;
        for (k, slot) in thresholds.iter_mut().enumerate().take(65536).skip(1) {
            let code = k as u16;
            let mut v = srgb_to_linear((k as f64 - 0.5) / 65535.0);
            // Walk down while the code is still reached, then up to the
            // first double that reaches it.
            let mut steps = 0;
            while Self::reference(v) >= code && steps < 4096 {
                v = v.next_down();
                steps += 1;
            }
            while Self::reference(v) < code && steps < 8192 {
                v = v.next_up();
                steps += 1;
            }
            *slot = v;
        }
        let coarse: Vec<u16> = (0..=65536u32)
            .map(|b| Self::reference(f64::from(b) / 65536.0))
            .collect();
        let fine: Vec<u16> = (0..=65536u32)
            .map(|b| Self::reference(f64::from(b) / 1_048_576.0))
            .collect();
        let span = |t: &[u16], from: usize| {
            t[from..]
                .windows(2)
                .map(|w| usize::from(w[1] - w[0]))
                .max()
                .unwrap_or(0)
        };
        // Coarse buckets are only read from FINE_LIMIT up.
        let max_steps = span(&fine, 0).max(span(&coarse, 4096));
        Self {
            thresholds,
            coarse,
            fine,
            max_steps,
        }
    }

    fn cell() -> &'static std::sync::OnceLock<EncodeTable> {
        static TABLE: std::sync::OnceLock<EncodeTable> = std::sync::OnceLock::new();
        &TABLE
    }

    /// The shared table, built on first use (a few milliseconds).
    fn get() -> &'static Self {
        Self::cell().get_or_init(Self::build)
    }

    /// Whether [`EncodeTable::get`] is free.
    fn is_built() -> bool {
        Self::cell().get().is_some()
    }

    #[inline]
    fn encode(&self, v: f64) -> u16 {
        if v.is_nan() || v <= 0.0 {
            return 0;
        }
        if v >= 1.0 {
            return 65535;
        }
        let mut k = usize::from(if v < Self::FINE_LIMIT {
            self.fine[(v * 1_048_576.0) as usize]
        } else {
            self.coarse[(v * 65536.0) as usize]
        });
        // The bucket starts at code k; count the steps up to v. The
        // sentinels past the last threshold stop the count at 65535.
        for _ in 0..Self::UNROLLED_STEPS {
            k += usize::from(self.thresholds[k + 1] <= v);
        }
        if self.max_steps > Self::UNROLLED_STEPS {
            while self.thresholds[k + 1] <= v {
                k += 1;
            }
        }
        k as u16
    }
}

/// A gradient as a [`Field`]: geometry + colour ramp + bounding box.
/// Build with [`GradientOptions::field`].
#[derive(Clone, Debug)]
pub struct GradientField {
    width: u32,
    height: u32,
    geometry: Geometry,
    ramp: ColorRamp,
    bbox: Option<BoundingBox>,
}

impl GradientField {
    /// Gradient parameter `t` of pixel `(x, y)` before the spread
    /// method is applied (it may lie outside `[0, 1]`).
    pub fn parameter(&self, x: u32, y: u32) -> f64 {
        self.geometry.t(x, self.geometry.row_term(y))
    }

    /// Straight-alpha encoded colour of the ramp at parameter `t`.
    pub fn color_at(&self, t: f64) -> [f64; 4] {
        self.ramp.at(t)
    }

    /// Fill a row a batch at a time: the gradient parameters of up to
    /// 256 pixels, clamped to `[0, 1]` (NaN as 0), are handed to
    /// `shade` together with the pixels they belong to. For ramps whose
    /// colour is a plain function of the clamped parameter
    /// ([`ColorRamp::simple`]).
    #[inline]
    fn batch_rows<P: Copy>(
        &self,
        y: u32,
        x0: u32,
        out: &mut [P],
        outside: P,
        shade: impl Fn(&[f64], &mut [P]),
    ) {
        const BATCH: usize = 256;
        let row = self.geometry.row_term(y);
        let (lo, hi) = match self.bbox {
            None => (0, out.len()),
            Some(b) => {
                if y < b.y || y - b.y >= b.height {
                    out.fill(outside);
                    return;
                }
                let lo = (b.x.saturating_sub(x0) as usize).min(out.len());
                let hi = ((b.x + b.width).saturating_sub(x0) as usize).min(out.len());
                out[..lo].fill(outside);
                out[hi..].fill(outside);
                (lo, hi)
            }
        };
        let mut t = [0f64; BATCH];
        let mut first = x0 + lo as u32;
        for chunk in out[lo..hi].chunks_mut(BATCH) {
            let t = &mut t[..chunk.len()];
            macro_rules! parameters {
                ($geometry:expr) => {{
                    let g = $geometry;
                    for (i, v) in t.iter_mut().enumerate() {
                        *v = g.t(first + i as u32, row);
                    }
                }};
            }
            match self.geometry {
                g @ Geometry::Vector { .. } => parameters!(g),
                g @ Geometry::Angle { .. } => parameters!(g),
                g @ Geometry::Radial { .. } => parameters!(g),
                g @ Geometry::Conic { .. } => parameters!(g),
            }
            for v in t.iter_mut() {
                *v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            }
            shade(t, chunk);
            first += chunk.len() as u32;
        }
    }

    /// Fill a row with `shade(t)`, or `outside` beyond the bounding
    /// box. The geometry is matched once per row so each arm is a
    /// straight loop over `x`.
    #[inline]
    fn shade_row<P: Copy>(
        &self,
        y: u32,
        x0: u32,
        out: &mut [P],
        outside: P,
        shade: impl Fn(f64) -> P,
    ) {
        let row = self.geometry.row_term(y);
        let (lo, hi) = match self.bbox {
            None => (0, out.len()),
            Some(b) => {
                if y < b.y || y - b.y >= b.height {
                    out.fill(outside);
                    return;
                }
                let lo = (b.x.saturating_sub(x0) as usize).min(out.len());
                let hi = ((b.x + b.width).saturating_sub(x0) as usize).min(out.len());
                out[..lo].fill(outside);
                out[hi..].fill(outside);
                (lo, hi)
            }
        };
        let span = &mut out[lo..hi];
        let first = x0 + lo as u32;
        // One copy of the loop per geometry: `t` inlines into it.
        macro_rules! run {
            ($geometry:expr) => {{
                let g = $geometry;
                for (i, o) in span.iter_mut().enumerate() {
                    *o = shade(g.t(first + i as u32, row));
                }
            }};
        }
        match self.geometry {
            g @ Geometry::Vector { .. } => run!(g),
            g @ Geometry::Angle { .. } => run!(g),
            g @ Geometry::Radial { .. } => run!(g),
            g @ Geometry::Conic { .. } => run!(g),
        }
    }

    /// `t` does not depend on `x`.
    fn constant_along_x(&self) -> bool {
        match self.geometry {
            Geometry::Vector { dx, .. } | Geometry::Angle { dx, .. } => dx == 0.0,
            _ => false,
        }
    }

    /// `t` does not depend on `y`.
    fn constant_along_y(&self) -> bool {
        match self.geometry {
            Geometry::Vector { dy, .. } | Geometry::Angle { dy, .. } => dy == 0.0,
            _ => false,
        }
    }
}

impl Field for GradientField {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
        if let Some((c0, delta)) = self.ramp.simple() {
            self.batch_rows(y, x0, out, self.ramp.outside, |t, o| {
                for (o, t) in o.iter_mut().zip(t) {
                    *o = [
                        c0[0] + delta[0] * t,
                        c0[1] + delta[1] * t,
                        c0[2] + delta[2] * t,
                        1.0,
                    ];
                }
            });
            return;
        }
        self.shade_row(y, x0, out, self.ramp.outside, |t| self.ramp.at(t));
    }

    fn fill_row_q16(&self, y: u32, x0: u32, out: &mut [[u16; 4]]) -> bool {
        if self.ramp.space == GradientColorspace::Srgb {
            // Two opaque colours, padded: the parameter and the three
            // mixes are evaluated a batch at a time in flat loops the
            // compiler can vectorise. Every value is computed by the
            // same expression as the general path.
            let Some((c0, delta)) = self.ramp.simple() else {
                return false;
            };
            self.batch_rows(
                y,
                x0,
                out,
                self.ramp.outside.map(canvas::unit_to_q16),
                |t, o| {
                    for (o, t) in o.iter_mut().zip(t) {
                        *o = [
                            canvas::unit_to_q16(c0[0] + delta[0] * t),
                            canvas::unit_to_q16(c0[1] + delta[1] * t),
                            canvas::unit_to_q16(c0[2] + delta[2] * t),
                            65535,
                        ];
                    }
                },
            );
            return true;
        }
        // The other spaces go through the transfer function, which the
        // table replaces; building it is not worth it for a thumbnail
        // unless another render already did.
        let pixels = u64::from(self.width) * u64::from(self.height);
        if pixels < 1 << 16 && !EncodeTable::is_built() {
            return false;
        }
        let table = EncodeTable::get();
        self.shade_row(
            y,
            x0,
            out,
            self.ramp.outside.map(canvas::unit_to_q16),
            |t| self.ramp.at_q16(t, table),
        );
        true
    }

    fn uniform_row(&self, y: u32) -> Option<[f64; 4]> {
        if self.bbox.is_none() && self.constant_along_x() {
            Some(self.ramp.at(self.parameter(0, y)))
        } else {
            None
        }
    }

    fn row_alias(&self, y: u32) -> Option<u32> {
        if self.bbox.is_none() && self.constant_along_y() {
            y.checked_sub(1)
        } else {
            None
        }
    }
}

impl GradientOptions {
    /// The stops as `(offset, straight encoded colour)`, offsets
    /// resolved: a missing first / last offset is 0 / 1, an offset
    /// below an earlier one is raised to it, and runs of missing
    /// offsets are spaced evenly between their neighbours. With no
    /// explicit stops this is `from` at 0 and `to` at 1.
    pub fn resolved_stops(&self) -> Result<Vec<(f64, [f64; 4])>> {
        if self.stops.is_empty() {
            let c = |c: [u8; 4]| c.map(|v| f64::from(v) / 255.0);
            return Ok(vec![(0.0, c(self.from)), (1.0, c(self.to))]);
        }
        let n = self.stops.len();
        let mut offs: Vec<Option<f64>> = self.stops.iter().map(|s| s.offset).collect();
        for o in offs.iter().flatten() {
            if !o.is_finite() {
                return Err(Error::invalid("gradient: stop offsets must be finite"));
            }
        }
        if offs[0].is_none() {
            offs[0] = Some(0.0);
        }
        if offs[n - 1].is_none() {
            offs[n - 1] = Some(if n == 1 { 0.0 } else { 1.0 });
        }
        let mut floor = f64::NEG_INFINITY;
        for o in offs.iter_mut().flatten() {
            if *o < floor {
                *o = floor;
            }
            floor = *o;
        }
        let mut i = 0;
        while i < n {
            if offs[i].is_some() {
                i += 1;
                continue;
            }
            let start = i - 1;
            let mut end = i;
            while offs[end].is_none() {
                end += 1;
            }
            let (a, b) = (offs[start].unwrap(), offs[end].unwrap());
            for (k, slot) in offs[i..end].iter_mut().enumerate() {
                *slot = Some(a + (b - a) * (k + 1) as f64 / (end - start) as f64);
            }
            i = end;
        }
        Ok(self
            .stops
            .iter()
            .zip(offs)
            .map(|(s, o)| (o.unwrap(), s.color))
            .collect())
    }

    /// Build the gradient as a [`Field`], ready for
    /// [`canvas::render`] at any depth.
    pub fn field(&self, kind: GradientKind) -> Result<GradientField> {
        let bbox = self.checked_bbox()?;
        let ramp = ColorRamp::new(self)?;
        let w = self.width.max(1);
        let h = self.height.max(1);
        // Quantise a direction cosine to a 1e-12 grid. This snaps
        // floating-point dust (cos 90° ≈ 6e-17) to zero, so a cardinal
        // angle on a one-pixel axis stays degenerate (one colour), and
        // it makes the result independent of the platform's
        // trigonometric functions, which may differ in the last bit and
        // would flip exact-half cases such as t = 5/6 at 45° on 5x3.
        let snap = |v: f64| (v * 1e12).round() / 1e12;
        let geometry = match kind {
            GradientKind::Linear => match self.linear_vector() {
                Some([x1, y1, x2, y2]) => {
                    let dx = x2 - x1;
                    let dy = y2 - y1;
                    Geometry::Vector {
                        x1,
                        y1,
                        dx,
                        dy,
                        len2: dx * dx + dy * dy,
                    }
                }
                None => {
                    let theta = self.angle.unwrap_or(0.0).to_radians();
                    let dx = snap(theta.sin());
                    let dy = snap(-theta.cos());
                    let r = f64::from(w - 1);
                    let b = f64::from(h - 1);
                    let corners = [0.0, r * dx, b * dy, r * dx + b * dy];
                    let min = corners.iter().cloned().fold(f64::INFINITY, f64::min);
                    let max = corners.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    Geometry::Angle {
                        dx,
                        dy,
                        min,
                        range: max - min,
                    }
                }
            },
            GradientKind::Radial => {
                let (cx, cy) = self.radial_center();
                let (rx, ry) = self.radial_radii();
                let theta = self.angle.unwrap_or(0.0).to_radians();
                let (cos, sin) = (snap(theta.cos()), snap(theta.sin()));
                Geometry::Radial {
                    cx,
                    cy,
                    rx,
                    ry,
                    cos,
                    sin,
                    rotated: sin != 0.0 || cos != 1.0,
                }
            }
            GradientKind::Conic => {
                let (cx, cy) = self
                    .center
                    .unwrap_or((f64::from(w - 1) / 2.0, f64::from(h - 1) / 2.0));
                let start = self.angle.unwrap_or(0.0) / 360.0;
                Geometry::Conic {
                    cx,
                    cy,
                    start: start - start.floor(),
                }
            }
        };
        Ok(GradientField {
            width: w,
            height: h,
            geometry,
            ramp,
            bbox,
        })
    }
}

/// sRGB electro-optical transfer (CSS Color 4 §10.2 `lin_sRGB`).
#[inline]
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse of [`srgb_to_linear`] (CSS Color 4 §10.2 `gam_sRGB`).
#[inline]
fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
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

    /// Grey level of every pixel, row-major (the probes below were
    /// transcribed from `magick … -depth 8 txt:` and are all greys).
    fn greys(img: &Rgba8Image) -> Vec<u8> {
        (0..img.height)
            .flat_map(|y| (0..img.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = img.get(x, y);
                assert_eq!(p[0], p[1]);
                assert_eq!(p[1], p[2]);
                assert_eq!(p[3], 255);
                p[0]
            })
            .collect()
    }

    /// Every component within one level — used where magick keeps a
    /// fractional stop alpha (`rgba(…, 0.5)` is exactly ½ in its Q16
    /// HDRI pipeline, 128/255 in ours) so the last bit can differ.
    fn close(got: [u8; 4], want: [u8; 4]) {
        for i in 0..4 {
            assert!(
                (got[i] as i16 - want[i] as i16).abs() <= 1,
                "got {got:?}, want {want:?} (±1)"
            );
        }
    }

    fn bw(w: u32, h: u32) -> GradientOptions {
        GradientOptions::new(w, h).with_colors([0, 0, 0, 255], [255, 255, 255, 255])
    }

    #[test]
    fn linear_horizontal_endpoints() {
        let img = render(&map(&[
            ("from", "red"),
            ("to", "blue"),
            ("direction", "horizontal"),
            ("w", "100"),
            ("h", "10"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 5), [255, 0, 0, 255]);
        assert_eq!(img.get(99, 5), [0, 0, 255, 255]);
    }

    #[test]
    fn default_is_white_to_black_south() {
        // `magick -size 1x3 gradient:` → 255, 128, 0.
        let img = render(&map(&[("w", "1"), ("h", "3")])).unwrap();
        assert_eq!(greys(&img), vec![255, 128, 0]);
    }

    #[test]
    fn five_row_black_white_matches_magick() {
        // `magick -size 1x5 gradient:black-white` → 0 64 128 191 255.
        let img = render_linear(&bw(1, 5)).unwrap();
        assert_eq!(greys(&img), vec![0, 64, 128, 191, 255]);
        // 16-bit quantisation detail: t = 0.75 → 49151 → 191 (not 192).
        let img = render_linear(&bw(1, 4)).unwrap();
        assert_eq!(greys(&img), vec![0, 85, 170, 255]);
    }

    #[test]
    fn one_row_default_falls_back_to_east() {
        // `magick -size 5x1 gradient:black-white` ramps across x.
        let img = render_linear(&bw(5, 1)).unwrap();
        assert_eq!(greys(&img), vec![0, 64, 128, 191, 255]);
        // …but an explicit North on one row is a single colour.
        let img = render_linear(&bw(5, 1).with_direction(GradientDirection::North)).unwrap();
        assert_eq!(greys(&img), vec![0; 5]);
    }

    #[test]
    fn compass_directions_3x3_match_magick() {
        // Transcribed from `magick -size 3x3 -define gradient:direction=D
        // gradient:black-white`; West / NorthWest / SouthWest carry the
        // start-pixel artefact in magick's output, replaced here by the
        // defined value (see module docs).
        let cases: [(GradientDirection, [u8; 9]); 8] = [
            (
                GradientDirection::East,
                [0, 128, 255, 0, 128, 255, 0, 128, 255],
            ),
            (
                GradientDirection::West,
                [255, 128, 0, 255, 128, 0, 255, 128, 0],
            ),
            (
                GradientDirection::North,
                [255, 255, 255, 128, 128, 128, 0, 0, 0],
            ),
            (
                GradientDirection::South,
                [0, 0, 0, 128, 128, 128, 255, 255, 255],
            ),
            (
                GradientDirection::NorthEast,
                [128, 191, 255, 64, 128, 191, 0, 64, 128],
            ),
            (
                GradientDirection::NorthWest,
                [255, 191, 128, 191, 128, 64, 128, 64, 0],
            ),
            (
                GradientDirection::SouthEast,
                [0, 64, 128, 64, 128, 191, 128, 191, 255],
            ),
            (
                GradientDirection::SouthWest,
                [128, 64, 0, 191, 128, 64, 255, 191, 128],
            ),
        ];
        for (dir, want) in cases {
            let img = render_linear(&bw(3, 3).with_direction(dir)).unwrap();
            assert_eq!(greys(&img), want.to_vec(), "{}", dir.name());
        }
    }

    #[test]
    fn diagonal_direction_is_corner_to_corner_on_non_square() {
        // `magick -size 5x3 -define gradient:direction=NorthEast
        // gradient:black-white`.
        let img = render_linear(&bw(5, 3).with_direction(GradientDirection::NorthEast)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                51, 102, 153, 204, 255, 26, 77, 128, 179, 230, 0, 51, 102, 153, 204
            ]
        );
    }

    #[test]
    fn angle_mode_spans_corner_projections() {
        // `magick -size 5x3 -define gradient:angle=45 gradient:black-white`
        // — NOT corner to corner: t = (x + (2 − y)) / 6.
        let img = render_linear(&bw(5, 3).with_angle(45.0)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                85, 128, 170, 213, 255, 43, 85, 128, 170, 213, 0, 43, 85, 128, 170
            ]
        );
        // angle=30 on 5x3.
        let img = render_linear(&bw(5, 3).with_angle(30.0)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                118, 153, 187, 221, 255, 59, 93, 128, 162, 196, 0, 34, 68, 102, 137
            ]
        );
        // Cardinal angles coincide with the compass directions.
        for (angle, dir) in [
            (0.0, GradientDirection::North),
            (90.0, GradientDirection::East),
            (180.0, GradientDirection::South),
            (270.0, GradientDirection::West),
        ] {
            let a = render_linear(&bw(4, 4).with_angle(angle)).unwrap();
            let d = render_linear(&bw(4, 4).with_direction(dir)).unwrap();
            assert_eq!(a.pixels, d.pixels, "angle {angle}");
        }
    }

    #[test]
    fn vector_mode_projects_and_clamps() {
        // `-define gradient:vector=1,0,3,0` on 5x1 → 0 0 128 255 255.
        let img = render_linear(&bw(5, 1).with_vector(1.0, 0.0, 3.0, 0.0)).unwrap();
        assert_eq!(greys(&img), vec![0, 0, 128, 255, 255]);
        // Reversed: 255 255 128 0 0 (magick prints 255 255 128 128 0 —
        // the start-pixel artefact).
        let img = render_linear(&bw(5, 1).with_vector(3.0, 0.0, 1.0, 0.0)).unwrap();
        assert_eq!(greys(&img), vec![255, 255, 128, 0, 0]);
        // Oblique: `vector=0,0,4,2` on 5x5, t = (4x + 2y) / 20.
        let img = render_linear(&bw(5, 5).with_vector(0.0, 0.0, 4.0, 2.0)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                0, 51, 102, 153, 204, 26, 77, 128, 179, 230, 51, 102, 153, 204, 255, 77, 128, 179,
                230, 255, 102, 153, 204, 255, 255
            ]
        );
        // Zero-length vector → first colour everywhere.
        let img = render_linear(&bw(5, 1).with_vector(2.0, 0.0, 2.0, 0.0)).unwrap();
        assert_eq!(greys(&img), vec![0; 5]);
    }

    #[test]
    fn extent_without_direction_defaults_to_south_east() {
        // `magick -size 3x5 -define gradient:extent=Circle gradient:black-white`.
        let img = render_linear(&bw(3, 5).with_extent(GradientExtent::Circle)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                0, 26, 51, 51, 77, 102, 102, 128, 153, 153, 179, 204, 204, 230, 255
            ]
        );
        // With a direction the extent is irrelevant for linear.
        let a = render_linear(
            &bw(5, 3)
                .with_direction(GradientDirection::East)
                .with_extent(GradientExtent::Minimum),
        )
        .unwrap();
        let b = render_linear(&bw(5, 3).with_direction(GradientDirection::East)).unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn bounding_box_masks_with_first_colour() {
        // `-define gradient:bounding-box=3x3+1+1 gradient:red-blue` on 5x5.
        let img = render_linear(
            &GradientOptions::new(5, 5)
                .with_colors([255, 0, 0, 255], [0, 0, 255, 255])
                .with_bounding_box(BoundingBox::new(3, 3, 1, 1)),
        )
        .unwrap();
        assert_eq!(img.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(4, 4), [255, 0, 0, 255]);
        assert_eq!(img.get(0, 2), [255, 0, 0, 255]);
        assert_eq!(img.get(1, 1), [191, 0, 64, 255]);
        assert_eq!(img.get(2, 2), [128, 0, 128, 255]);
        assert_eq!(img.get(3, 3), [64, 0, 191, 255]);
        // With alpha in play the outside is transparent (first RGB, a=0).
        let img = render_linear(
            &GradientOptions::new(1, 5)
                .with_colors([0, 0, 255, 128], [255, 0, 0, 255])
                .with_bounding_box(BoundingBox::parse("1x3+0+1").unwrap()),
        )
        .unwrap();
        assert_eq!(img.get(0, 0), [0, 0, 255, 0]);
        close(img.get(0, 1), [102, 0, 153, 159]);
        close(img.get(0, 2), [170, 0, 85, 191]);
        close(img.get(0, 3), [219, 0, 36, 223]);
        assert_eq!(img.get(0, 4), [0, 0, 255, 0]);
    }

    #[test]
    fn bounding_box_must_fit() {
        for bad in ["9x9", "3x3+3+3", "3x3-1-1", "0x3", "3x3+-1+0"] {
            let r = render(&map(&[("w", "5"), ("h", "5"), ("bbox", bad)]));
            assert!(r.is_err(), "{bad} should be rejected");
        }
        assert_eq!(
            BoundingBox::parse("3x3").unwrap(),
            BoundingBox::new(3, 3, 0, 0)
        );
        assert_eq!(
            BoundingBox::parse("10x20+3+4").unwrap(),
            BoundingBox::new(10, 20, 3, 4)
        );
        // Form-decoded `+` (space) is accepted.
        assert_eq!(
            BoundingBox::parse("10x20 3 4").unwrap(),
            BoundingBox::new(10, 20, 3, 4)
        );
    }

    #[test]
    fn premultiplied_alpha_interpolation_matches_magick() {
        // `gradient:"rgba(255,0,0,0.5)-rgba(0,0,255,1)"` 1x3 → mid (85,0,170,191).
        let img = render(&map(&[
            ("from", "rgba(255,0,0,0.5)"),
            ("to", "rgba(0,0,255,1)"),
            ("w", "1"),
            ("h", "3"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0), [255, 0, 0, 128]);
        close(img.get(0, 1), [85, 0, 170, 191]);
        assert_eq!(img.get(0, 2), [0, 0, 255, 255]);
        // `gradient:"srgb(255,0,0)-#0000ff80"` → mid (170,0,85,192).
        let img = render(&map(&[
            ("from", "srgb(255,0,0)"),
            ("to", "#0000ff80"),
            ("w", "1"),
            ("h", "3"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 1), [170, 0, 85, 192]);
        // Fully transparent endpoint un-premultiplies to (0,0,0,0).
        let img = render(&map(&[
            ("from", "rgba(255,0,0,0)"),
            ("to", "red"),
            ("w", "1"),
            ("h", "3"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0), [0, 0, 0, 0]);
        assert_eq!(img.get(0, 1), [255, 0, 0, 128]);
    }

    #[test]
    fn q16_quantisation_matches_magick() {
        // `gradient:"gray(100)-gray(201)"` 1x4 → 100 134 167 201.
        let img = render(&map(&[
            ("from", "gray(100)"),
            ("to", "gray(201)"),
            ("w", "1"),
            ("h", "4"),
        ]))
        .unwrap();
        assert_eq!(greys(&img), vec![100, 134, 167, 201]);
    }

    #[test]
    fn linear_light_midpoint() {
        // `magick -size 1x3 gradient:black-white -set colorspace RGB
        // -colorspace sRGB` → 0 188 255.
        let img = render_linear(&bw(1, 3).with_colorspace(GradientColorspace::Linear)).unwrap();
        assert_eq!(greys(&img), vec![0, 188, 255]);
        let img = render(&map(&[
            ("from", "black"),
            ("to", "white"),
            ("colorspace", "rgb"),
            ("w", "1"),
            ("h", "3"),
        ]))
        .unwrap();
        assert_eq!(greys(&img), vec![0, 188, 255]);
    }

    #[test]
    fn radial_defaults_match_magick() {
        // `magick -size 5x5 radial-gradient:black-white`.
        let img = render_radial(&bw(5, 5)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                255, 255, 255, 255, 255, 255, 180, 128, 180, 255, 255, 128, 0, 128, 255, 255, 180,
                128, 180, 255, 255, 255, 255, 255, 255
            ]
        );
        // Even size: centre at (1.5, 1.5), radius 1.5.
        let img = render_radial(&bw(4, 4)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                255, 255, 255, 255, 255, 120, 120, 255, 255, 120, 120, 255, 255, 255, 255, 255
            ]
        );
        // Non-square: radius = max half-size (3 on 7x5).
        let img = render_radial(&bw(7, 5)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                255, 240, 190, 170, 190, 240, 255, 255, 190, 120, 85, 120, 190, 255, 255, 170, 85,
                0, 85, 170, 255, 255, 190, 120, 85, 120, 190, 255, 255, 240, 190, 170, 190, 240,
                255
            ]
        );
        // One row: centre (2, 0), radius 2.
        let img = render_radial(&bw(5, 1)).unwrap();
        assert_eq!(greys(&img), vec![255, 128, 0, 128, 255]);
        // Bare `radial-gradient:` is white centre → black.
        let img = render(&map(&[("type", "radial"), ("w", "3"), ("h", "3")])).unwrap();
        assert_eq!(greys(&img), vec![0, 0, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn radial_extents_match_magick() {
        // 7x5, from the `-define gradient:extent=…` probes.
        let img = render_radial(&bw(7, 5).with_extent(GradientExtent::Diagonal)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                255, 200, 158, 141, 158, 200, 255, 224, 158, 100, 71, 100, 158, 224, 212, 141, 71,
                0, 71, 141, 212, 224, 158, 100, 71, 100, 158, 224, 255, 200, 158, 141, 158, 200,
                255
            ]
        );
        let img = render_radial(&bw(7, 5).with_extent(GradientExtent::Ellipse)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                255, 255, 255, 255, 255, 255, 255, 255, 213, 153, 128, 153, 213, 255, 255, 170, 85,
                0, 85, 170, 255, 255, 213, 153, 128, 153, 213, 255, 255, 255, 255, 255, 255, 255,
                255
            ]
        );
        let img = render_radial(&bw(7, 5).with_extent(GradientExtent::Minimum)).unwrap();
        assert_eq!(
            greys(&img),
            vec![
                255, 255, 255, 255, 255, 255, 255, 255, 255, 180, 128, 180, 255, 255, 255, 255,
                128, 0, 128, 255, 255, 255, 255, 180, 128, 180, 255, 255, 255, 255, 255, 255, 255,
                255, 255
            ]
        );
        let max = render_radial(&bw(7, 5).with_extent(GradientExtent::Maximum)).unwrap();
        let circle = render_radial(&bw(7, 5).with_extent(GradientExtent::Circle)).unwrap();
        let dflt = render_radial(&bw(7, 5)).unwrap();
        assert_eq!(max.pixels, circle.pixels);
        assert_eq!(max.pixels, dflt.pixels);
    }

    #[test]
    fn radial_center_and_radii_overrides() {
        // `-define gradient:center=1,1` on 7x5 keeps the canvas radius 3.
        let img = render_radial(&bw(7, 5).with_center(1.0, 1.0)).unwrap();
        assert_eq!(img.get(1, 1)[0], 0);
        assert_eq!(img.get(0, 1)[0], 85);
        assert_eq!(img.get(1, 0)[0], 85);
        assert_eq!(img.get(4, 1)[0], 255);
        // …and Minimum with that centre still uses the canvas (r = 2).
        let img = render_radial(
            &bw(7, 5)
                .with_center(1.0, 1.0)
                .with_extent(GradientExtent::Minimum),
        )
        .unwrap();
        assert_eq!(img.get(0, 1)[0], 128);
        // `-define gradient:radii=3,1` — elliptical.
        let img = render_radial(&bw(7, 5).with_radii(3.0, Some(1.0))).unwrap();
        assert_eq!(img.get(2, 2)[0], 85);
        assert_eq!(img.get(3, 1)[0], 255);
        // Single radius keeps the extent's vertical radius (3 here).
        let img = render_radial(&bw(7, 5).with_radii(2.0, None)).unwrap();
        assert_eq!(img.get(2, 2)[0], 128);
        assert_eq!(img.get(3, 1)[0], 85);
        assert_eq!(img.get(2, 1)[0], 153);
        // …and with Ellipse / Minimum the vertical radius is 2.
        let img = render_radial(
            &bw(7, 5)
                .with_radii(1.0, None)
                .with_extent(GradientExtent::Ellipse),
        )
        .unwrap();
        assert_eq!(img.get(3, 1)[0], 128);
        // Zero radii: a single centre pixel.
        let img = render_radial(&bw(7, 5).with_radii(0.0, Some(0.0))).unwrap();
        let g = greys(&img);
        assert_eq!(g.iter().filter(|&&v| v == 0).count(), 1);
        assert_eq!(g[2 * 7 + 3], 0);
        // Fractional centre.
        let a = render_radial(&bw(7, 5).with_center(1.5, 1.5)).unwrap();
        assert_eq!(a.get(1, 1)[0], 60);
        assert_eq!(a.get(2, 2)[0], 60);
        assert_eq!(a.get(0, 0)[0], 180);
    }

    #[test]
    fn side_and_corner_extents_reduce_to_canvas_extents_when_centred() {
        for &(w, h) in &[(7u32, 5u32), (5, 9), (6, 6), (1, 4)] {
            let px = |e| greys(&render_radial(&bw(w, h).with_extent(e)).unwrap());
            use GradientExtent as E;
            assert_eq!(px(E::ClosestSide), px(E::Minimum), "{w}x{h}");
            assert_eq!(px(E::FarthestSide), px(E::Maximum), "{w}x{h}");
            assert_eq!(px(E::ClosestCorner), px(E::Diagonal), "{w}x{h}");
            assert_eq!(px(E::FarthestCorner), px(E::Diagonal), "{w}x{h}");
        }
    }

    #[test]
    fn side_and_corner_extents_follow_the_centre() {
        use GradientExtent as E;
        // 9x7 canvas (edges at x = 0, 8 and y = 0, 6), centre (2, 1):
        // sides 2, 6, 1, 5; corners √5, √37, √29, √61.
        let r = |e: E| e.radii_at(9, 7, 2.0, 1.0);
        assert_eq!(r(E::ClosestSide), (1.0, 1.0));
        assert_eq!(r(E::FarthestSide), (6.0, 6.0));
        assert_eq!(r(E::ClosestCorner), (5f64.sqrt(), 5f64.sqrt()));
        assert_eq!(r(E::FarthestCorner), (61f64.sqrt(), 61f64.sqrt()));
        // The canvas extents ignore the centre.
        assert_eq!(r(E::Ellipse), (4.0, 3.0));
        // A centre outside the canvas measures unsigned distances.
        assert_eq!(E::ClosestSide.radii_at(5, 5, -2.0, 2.0), (2.0, 2.0));

        // Rendered (black centre → white): closest-side from (2, 1)
        // reaches white one row up and one row down; farthest-corner
        // reaches white only at the opposite corner.
        let img =
            render_radial(&bw(9, 7).with_center(2.0, 1.0).with_extent(E::ClosestSide)).unwrap();
        assert_eq!(img.get(2, 1)[0], 0);
        assert_eq!(img.get(2, 0)[0], 255);
        assert_eq!(img.get(2, 2)[0], 255);
        let img = render_radial(
            &bw(9, 7)
                .with_center(2.0, 1.0)
                .with_extent(E::FarthestCorner),
        )
        .unwrap();
        assert_eq!(img.get(8, 6)[0], 255);
        assert!(img.get(7, 5)[0] < 255);
        // Explicit radii still win over the extent.
        let img = render_radial(
            &bw(9, 7)
                .with_center(2.0, 1.0)
                .with_extent(E::ClosestSide)
                .with_radii(4.0, Some(4.0)),
        )
        .unwrap();
        assert_eq!(img.get(2, 3)[0], 128);
    }

    #[test]
    fn side_and_corner_extent_names_parse() {
        use GradientExtent as E;
        for (s, e) in [
            ("closest-side", E::ClosestSide),
            ("FarthestSide", E::FarthestSide),
            ("closest_corner", E::ClosestCorner),
            ("Farthest-Corner", E::FarthestCorner),
        ] {
            assert_eq!(E::parse(s), Some(e), "{s}");
            assert_eq!(E::parse(e.name()), Some(e));
        }
    }

    #[test]
    fn radial_center_follows_half_vector_end() {
        // `-define gradient:direction=East radial-gradient:` on 7x5 →
        // centre (3, 0); South → (0, 2); North / West → (0, 0).
        let img = render_radial(&bw(7, 5).with_direction(GradientDirection::East)).unwrap();
        assert_eq!(img.get(3, 0)[0], 0);
        assert_eq!(img.get(2, 0)[0], 85);
        assert_eq!(img.get(3, 1)[0], 85);
        let img = render_radial(&bw(7, 5).with_direction(GradientDirection::South)).unwrap();
        assert_eq!(img.get(0, 2)[0], 0);
        assert_eq!(img.get(1, 2)[0], 85);
        let img = render_radial(&bw(7, 5).with_direction(GradientDirection::North)).unwrap();
        assert_eq!(img.get(0, 0)[0], 0);
        // `vector=3,1,6,1` → centre (3, 0.5): (3,0) and (3,1) both 43.
        let img = render_radial(&bw(7, 5).with_vector(3.0, 1.0, 6.0, 1.0)).unwrap();
        assert_eq!(img.get(3, 0)[0], 43);
        assert_eq!(img.get(3, 1)[0], 43);
        assert_eq!(img.get(2, 0)[0], 95);
        // An explicit centre wins over the vector.
        let img = render_radial(
            &bw(7, 5)
                .with_center(1.0, 1.0)
                .with_vector(3.0, 2.0, 6.0, 2.0),
        )
        .unwrap();
        assert_eq!(img.get(1, 1)[0], 0);
        // Angle does not move the radial centre.
        let a = render_radial(&bw(7, 5).with_angle(45.0)).unwrap();
        assert_eq!(a.pixels, render_radial(&bw(7, 5)).unwrap().pixels);
    }

    #[test]
    fn radial_bounding_box_masks_canvas_gradient() {
        // `-define gradient:bounding-box=3x3+1+1 radial-gradient:red-blue` 5x5.
        let img = render(&map(&[
            ("type", "radial"),
            ("from", "red"),
            ("to", "blue"),
            ("bbox", "3x3+1+1"),
            ("w", "5"),
            ("h", "5"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(1, 1), [75, 0, 180, 255]);
        assert_eq!(img.get(2, 1), [128, 0, 128, 255]);
        assert_eq!(img.get(2, 2), [255, 0, 0, 255]);
    }

    #[test]
    fn query_surface_parses_every_define() {
        let q = map(&[
            ("w", "7"),
            ("h", "5"),
            ("from", "red"),
            ("to", "blue"),
            ("direction", "north-west"),
            ("angle", "33.5"),
            ("vector", "1,2,3,4"),
            ("extent", "ellipse"),
            ("bounding-box", "3x2+1+1"),
            ("center", "1.5,2"),
            ("radii", "2"),
            ("colorspace", "linear"),
        ]);
        let o = GradientOptions::from_query(&q).unwrap();
        assert_eq!(o.width, 7);
        assert_eq!(o.height, 5);
        assert_eq!(o.from, [255, 0, 0, 255]);
        assert_eq!(o.to, [0, 0, 255, 255]);
        assert_eq!(o.direction, Some(GradientDirection::NorthWest));
        assert_eq!(o.angle, Some(33.5));
        assert_eq!(o.vector, Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(o.extent, Some(GradientExtent::Ellipse));
        assert_eq!(o.bounding_box, Some(BoundingBox::new(3, 2, 1, 1)));
        assert_eq!(o.center, Some((1.5, 2.0)));
        assert_eq!(o.radii, Some((2.0, None)));
        assert_eq!(o.colorspace, GradientColorspace::Linear);
    }

    #[test]
    fn bad_options_error_clearly() {
        let err = render(&map(&[("direction", "spiral")])).unwrap_err();
        assert!(format!("{err}").contains("spiral"));
        let err = render(&map(&[("extent", "square")])).unwrap_err();
        assert!(format!("{err}").contains("square"));
        let err = render(&map(&[("vector", "1,2,3")])).unwrap_err();
        assert!(format!("{err}").contains("x1,y1,x2,y2"));
        let err = render(&map(&[("radii", "1,2,3")])).unwrap_err();
        assert!(format!("{err}").contains("radii"));
        let err = render(&map(&[("type", "spiral")])).unwrap_err();
        assert!(format!("{err}").contains("spiral"));
        let err = render(&map(&[("colorspace", "cmyk")])).unwrap_err();
        assert!(format!("{err}").contains("cmyk"));
        let err = render(&map(&[("spread", "wrap")])).unwrap_err();
        assert!(format!("{err}").contains("wrap"));
        let err = render(&map(&[("stops", "red,nocolour")])).unwrap_err();
        assert!(format!("{err}").contains("nocolour"));
    }

    #[test]
    fn single_pixel_canvas_is_first_colour() {
        let img = render_linear(&bw(1, 1)).unwrap();
        assert_eq!(greys(&img), vec![0]);
        let img = render_linear(&bw(1, 1).with_angle(45.0)).unwrap();
        assert_eq!(greys(&img), vec![0]);
        // `magick -size 1x5 -define gradient:angle=90` is one colour.
        let img = render_linear(&bw(1, 5).with_angle(90.0)).unwrap();
        assert_eq!(greys(&img), vec![0; 5]);
        let img = render_linear(&bw(5, 1).with_angle(0.0)).unwrap();
        assert_eq!(greys(&img), vec![0; 5]);
        let img = render_radial(&bw(1, 1)).unwrap();
        assert_eq!(greys(&img), vec![0]);
    }

    // ── multi-stop, spread, conic, colour spaces, depths ──

    use crate::image::canvas::{Channels, Dither, SampleDepth};

    fn rgb(r: u8, g: u8, b: u8) -> GradientStop {
        GradientStop::from_rgba8([r, g, b, 255])
    }

    fn east(w: u32, h: u32) -> GradientOptions {
        GradientOptions::new(w, h).with_direction(GradientDirection::East)
    }

    #[test]
    fn three_stops_are_exact_at_the_stops_and_linear_between() {
        let opts = east(5, 1).with_stops(vec![rgb(255, 0, 0), rgb(0, 255, 0), rgb(0, 0, 255)]);
        assert_eq!(
            opts.resolved_stops()
                .unwrap()
                .iter()
                .map(|s| s.0)
                .collect::<Vec<_>>(),
            vec![0.0, 0.5, 1.0]
        );
        let img = render_linear(&opts).unwrap();
        assert_eq!(img.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(1, 0), [128, 128, 0, 255]);
        assert_eq!(img.get(2, 0), [0, 255, 0, 255]);
        assert_eq!(img.get(3, 0), [0, 128, 128, 255]);
        assert_eq!(img.get(4, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn stop_offsets_resolve_like_the_documented_fix_up() {
        let stops = GradientStop::parse_list("red, white 60%, rgb(0,0,255), lime, black 40%, gray")
            .unwrap();
        let opts = east(8, 1).with_stops(stops);
        let offs: Vec<f64> = opts.resolved_stops().unwrap().iter().map(|s| s.0).collect();
        // 0, 0.6, (0.6 + ⅓·0)…: the 40 % stop is raised to 0.6, so the
        // two automatic stops before it sit at 0.6 as well.
        assert_eq!(offs, vec![0.0, 0.6, 0.6, 0.6, 0.6, 1.0]);
        let stops = GradientStop::parse_list("red;white;blue@0.5;black").unwrap();
        let offs: Vec<f64> = east(4, 1)
            .with_stops(stops)
            .resolved_stops()
            .unwrap()
            .iter()
            .map(|s| s.0)
            .collect();
        assert_eq!(offs, vec![0.0, 0.25, 0.5, 1.0]);
    }

    #[test]
    fn coincident_stops_make_a_hard_edge() {
        let stops = GradientStop::parse_list("red 50%, blue 50%").unwrap();
        let img = render_linear(&east(5, 1).with_stops(stops)).unwrap();
        // t = 0, ¼ → red; t = ½, ¾, 1 → blue (the later stop wins at
        // the edge itself).
        assert_eq!(img.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(1, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(2, 0), [0, 0, 255, 255]);
        assert_eq!(img.get(4, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn spread_methods_follow_their_closed_forms() {
        // Vector covers x = 0..=4; the canvas runs on to x = 12.
        let base = bw(13, 1).with_vector(0.0, 0.0, 4.0, 0.0);
        let pad = greys(&render_linear(&base).unwrap());
        assert_eq!(pad[..5], [0, 64, 128, 191, 255]);
        assert!(pad[5..].iter().all(|v| *v == 255));
        let rep = greys(&render_linear(&base.clone().with_spread(GradientSpread::Repeat)).unwrap());
        // t = x/4; t = 1 keeps the end colour, then the ramp restarts.
        assert_eq!(
            rep,
            vec![0, 64, 128, 191, 255, 64, 128, 191, 0, 64, 128, 191, 0]
        );
        let refl =
            greys(&render_linear(&base.clone().with_spread(GradientSpread::Reflect)).unwrap());
        assert_eq!(
            refl,
            vec![0, 64, 128, 191, 255, 191, 128, 64, 0, 64, 128, 191, 255]
        );
        // Negative parameters reflect too.
        let neg = bw(5, 1)
            .with_vector(4.0, 0.0, 8.0, 0.0)
            .with_spread(GradientSpread::Reflect);
        assert_eq!(
            greys(&render_linear(&neg).unwrap()),
            vec![255, 191, 128, 64, 0]
        );
    }

    #[test]
    fn turn_fraction_is_exact_on_the_principal_directions_and_tracks_atan2() {
        assert_eq!(turn_fraction(0.0, 0.0), 0.0);
        assert_eq!(turn_fraction(3.0, 0.0), 0.0);
        assert_eq!(turn_fraction(2.0, 2.0), 0.125);
        assert_eq!(turn_fraction(0.0, 5.0), 0.25);
        assert_eq!(turn_fraction(-1.5, 1.5), 0.375);
        assert_eq!(turn_fraction(-7.0, 0.0), 0.5);
        assert_eq!(turn_fraction(-4.0, -4.0), 0.625);
        assert_eq!(turn_fraction(0.0, -1.0), 0.75);
        assert_eq!(turn_fraction(9.0, -9.0), 0.875);
        let mut state = 0x1234_5678_9ABC_DEF1u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 11) as f64 / (1u64 << 53) as f64) * 2000.0 - 1000.0
        };
        for _ in 0..20_000 {
            let (x, y) = (next(), next());
            let want = y.atan2(x) / std::f64::consts::TAU;
            let want = if want < 0.0 { want + 1.0 } else { want };
            let got = turn_fraction(x, y);
            assert!((0.0..1.0).contains(&got));
            assert!((got - want).abs() < 4e-16, "({x}, {y}): {got} vs {want}");
        }
    }

    #[test]
    fn conic_parameter_runs_clockwise_from_north() {
        let f = bw(5, 5).field(GradientKind::Conic).unwrap();
        assert_eq!(f.parameter(2, 2), 0.0); // centre
        assert_eq!(f.parameter(2, 0), 0.0); // North
        assert_eq!(f.parameter(4, 0), 0.125); // NorthEast
        assert_eq!(f.parameter(4, 2), 0.25); // East
        assert_eq!(f.parameter(4, 4), 0.375);
        assert_eq!(f.parameter(2, 4), 0.5); // South
        assert_eq!(f.parameter(0, 4), 0.625);
        assert_eq!(f.parameter(0, 2), 0.75); // West
        assert_eq!(f.parameter(0, 0), 0.875);
        // A start angle of 90° puts t = 0 on East.
        let f = bw(5, 5)
            .with_angle(90.0)
            .field(GradientKind::Conic)
            .unwrap();
        assert_eq!(f.parameter(4, 2), 0.0);
        assert_eq!(f.parameter(2, 4), 0.25);
        assert_eq!(f.parameter(2, 0), 0.75);
        // An explicit centre moves the apex.
        let f = bw(5, 5)
            .with_center(0.0, 0.0)
            .field(GradientKind::Conic)
            .unwrap();
        assert_eq!(f.parameter(0, 0), 0.0);
        assert_eq!(f.parameter(3, 0), 0.25);
        assert_eq!(f.parameter(3, 3), 0.375);
        assert_eq!(f.parameter(0, 2), 0.5);
    }

    #[test]
    fn conic_image_has_the_stop_colours_on_the_axes() {
        let stops = GradientStop::parse_list("red, yellow 25%, lime 50%, blue 75%, red").unwrap();
        let img = render_conic(&GradientOptions::new(9, 9).with_stops(stops)).unwrap();
        assert_eq!(img.get(4, 0), [255, 0, 0, 255]);
        assert_eq!(img.get(8, 4), [255, 255, 0, 255]);
        assert_eq!(img.get(4, 8), [0, 255, 0, 255]);
        assert_eq!(img.get(0, 4), [0, 0, 255, 255]);
        // Half-way between East and South: yellow → lime midpoint.
        assert_eq!(img.get(8, 8), [128, 255, 0, 255]);
        // Through the URI.
        let uri = render(&map(&[
            ("type", "conic"),
            ("stops", "red, yellow 25%, lime 50%, blue 75%, red"),
            ("w", "9"),
            ("h", "9"),
        ]))
        .unwrap();
        assert_eq!(uri.pixels, img.pixels);
    }

    #[test]
    fn radial_angle_rotates_the_ellipse_clockwise() {
        // Rows 2..=4 of `magick -size 9x9 -define gradient:radii=4,2
        // -define gradient:angle=30 radial-gradient:white-black`.
        let img = render_radial(
            &GradientOptions::new(9, 9)
                .with_radii(4.0, Some(2.0))
                .with_angle(30.0),
        )
        .unwrap();
        let rows: [[u8; 9]; 3] = [
            [0x00, 0x18, 0x39, 0x3a, 0x19, 0x00, 0x00, 0x00, 0x00],
            [0x00, 0x2a, 0x70, 0x9c, 0x8c, 0x4f, 0x04, 0x00, 0x00],
            [0x00, 0x02, 0x56, 0xab, 0xff, 0xab, 0x56, 0x02, 0x00],
        ];
        for (r, row) in rows.iter().enumerate() {
            for (x, want) in row.iter().enumerate() {
                let got = img.get(x as u32, r as u32 + 2)[0];
                assert!(
                    (i16::from(got) - i16::from(*want)).abs() <= 1,
                    "({x}, {}): {got} vs {want}",
                    r + 2
                );
            }
        }
        // A quarter turn is the same as swapping the radii.
        let a = render_radial(
            &GradientOptions::new(9, 7)
                .with_radii(4.0, Some(2.0))
                .with_angle(90.0),
        )
        .unwrap();
        let b = render_radial(&GradientOptions::new(9, 7).with_radii(2.0, Some(4.0))).unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn radial_default_is_mirror_symmetric() {
        let stops = GradientStop::parse_list("white, red 30%, navy").unwrap();
        let img = render_radial(
            &GradientOptions::new(12, 9)
                .with_stops(stops)
                .with_extent(GradientExtent::Ellipse),
        )
        .unwrap();
        for y in 0..9 {
            for x in 0..12 {
                assert_eq!(img.get(x, y), img.get(11 - x, y));
                assert_eq!(img.get(x, y), img.get(x, 8 - y));
            }
        }
    }

    #[test]
    fn opposite_directions_are_mirror_images() {
        let a = render_linear(&bw(17, 3).with_direction(GradientDirection::East)).unwrap();
        let b = render_linear(&bw(17, 3).with_direction(GradientDirection::West)).unwrap();
        for y in 0..3 {
            for x in 0..17 {
                assert_eq!(a.get(x, y), b.get(16 - x, y));
            }
        }
    }

    fn midpoint(space: GradientColorspace) -> [u8; 4] {
        render_linear(
            &east(3, 1)
                .with_colors([0, 0, 0, 255], [255, 255, 255, 255])
                .with_colorspace(space),
        )
        .unwrap()
        .get(1, 0)
    }

    #[test]
    fn black_to_white_midpoints_per_colour_space() {
        assert_eq!(midpoint(GradientColorspace::Srgb), [128, 128, 128, 255]);
        // Linear light ½ encodes to 0.7354 → 188.
        assert_eq!(midpoint(GradientColorspace::Linear), [188, 188, 188, 255]);
        // Oklab L = ½ is linear luminance ⅛, encoded 0.3886 → 99.
        assert_eq!(midpoint(GradientColorspace::Oklab), [99, 99, 99, 255]);
        assert_eq!(midpoint(GradientColorspace::Oklch), [99, 99, 99, 255]);
        // CIELAB L* = 50 is luminance 0.1842, encoded 0.4663 → 119.
        assert_eq!(midpoint(GradientColorspace::Lab), [119, 119, 119, 255]);
        assert_eq!(midpoint(GradientColorspace::Lch), [119, 119, 119, 255]);
    }

    #[test]
    fn perceptual_spaces_return_the_stop_colours_exactly() {
        for space in [
            GradientColorspace::Oklab,
            GradientColorspace::Oklch,
            GradientColorspace::Lab,
            GradientColorspace::Lch,
        ] {
            let opts = east(5, 1)
                .with_stops(vec![rgb(200, 30, 60), rgb(10, 180, 90), rgb(40, 50, 250)])
                .with_colorspace(space);
            let f = opts.field(GradientKind::Linear).unwrap();
            assert_eq!(
                f.color_at(0.0),
                [200.0 / 255.0, 30.0 / 255.0, 60.0 / 255.0, 1.0]
            );
            assert_eq!(
                f.color_at(0.5),
                [10.0 / 255.0, 180.0 / 255.0, 90.0 / 255.0, 1.0]
            );
            assert_eq!(
                f.color_at(1.0),
                [40.0 / 255.0, 50.0 / 255.0, 250.0 / 255.0, 1.0]
            );
            // Approaching a stop converges on it.
            let near = f.color_at(0.5 - 1e-9);
            for (a, b) in near.iter().zip(f.color_at(0.5)) {
                assert!((a - b).abs() < 1e-6, "{space:?}");
            }
            for t in [0.1, 0.3, 0.7, 0.9] {
                for v in f.color_at(t) {
                    assert!((0.0..=1.0).contains(&v));
                }
            }
        }
    }

    #[test]
    fn polar_hue_arcs_differ_and_grey_borrows_its_neighbours_hue() {
        let ramp = |arc: HueArc| {
            east(3, 1)
                .with_colors([255, 0, 0, 255], [0, 0, 255, 255])
                .with_colorspace(GradientColorspace::Oklch)
                .with_hue_arc(arc)
                .field(GradientKind::Linear)
                .unwrap()
                .color_at(0.5)
        };
        // Red (≈29°) to blue (≈264°): the short way passes magenta
        // (green lowest), the long way passes green.
        let short = ramp(HueArc::Shorter);
        assert!(short[1] < short[0] && short[1] < short[2], "{short:?}");
        let long = ramp(HueArc::Longer);
        assert!(long[1] > long[0] && long[1] > long[2], "{long:?}");
        assert_eq!(ramp(HueArc::Decreasing), short);
        assert_eq!(ramp(HueArc::Increasing), long);
        // White → red in a polar space fades straight to red: green and
        // blue stay equal-ish and below red (no detour through other
        // hues).
        let f = east(3, 1)
            .with_colors([255, 255, 255, 255], [255, 0, 0, 255])
            .with_colorspace(GradientColorspace::Oklch)
            .field(GradientKind::Linear)
            .unwrap();
        let mid = f.color_at(0.5);
        assert!(mid[0] > mid[1] && mid[0] > mid[2]);
        assert!((mid[1] - mid[2]).abs() < 0.08, "{mid:?}");
    }

    #[test]
    fn sixteen_bit_ramp_hits_every_level_and_eight_bit_is_its_rounding() {
        let opts = east(65536, 1).with_colors([0, 0, 0, 255], [255, 255, 255, 255]);
        let f = opts.field(GradientKind::Linear).unwrap();
        let cfg = RenderConfig::new().with_channels(Channels::Gray);
        let deep = canvas::render(&f, &cfg.with_depth(SampleDepth::U16));
        let deep = deep.as_u16().unwrap();
        for (x, v) in deep.iter().enumerate() {
            assert_eq!(usize::from(*v), x);
        }
        let low = canvas::render(&f, &cfg);
        for (a, b) in low.as_u8().unwrap().iter().zip(deep) {
            assert_eq!(u32::from(*a), (u32::from(*b) + 128) / 257);
        }
        let float = canvas::render(&f, &cfg.with_depth(SampleDepth::F32));
        for (x, v) in float.as_f32().unwrap().iter().enumerate() {
            assert_eq!(*v, (x as f64 / 65535.0) as f32);
        }
    }

    #[test]
    fn eight_bit_gradient_equals_the_rounded_float_gradient() {
        let stops = GradientStop::parse_list("#102030, gold 40%, rgba(0,128,255,0.5)").unwrap();
        for kind in [
            GradientKind::Linear,
            GradientKind::Radial,
            GradientKind::Conic,
        ] {
            let f = GradientOptions::new(61, 47)
                .with_stops(stops.clone())
                .with_angle(33.0)
                .field(kind)
                .unwrap();
            let low = canvas::render(&f, &RenderConfig::new());
            let float = canvas::render(&f, &RenderConfig::new().with_depth(SampleDepth::F32));
            let mut checked = 0;
            for (a, v) in low.as_u8().unwrap().iter().zip(float.as_f32().unwrap()) {
                let scaled = f64::from(*v) * 255.0;
                // Skip samples within f32 rounding of a tie.
                if (scaled - scaled.floor() - 0.5).abs() > 1e-4 {
                    assert_eq!(f64::from(*a), scaled.round(), "{kind:?}");
                    checked += 1;
                }
            }
            assert!(checked > 61 * 47 * 4 * 9 / 10);
        }
    }

    #[test]
    fn row_shortcuts_match_per_pixel_evaluation() {
        let stops = GradientStop::parse_list("red, white 30%, blue").unwrap();
        for dir in [
            GradientDirection::South,
            GradientDirection::North,
            GradientDirection::East,
            GradientDirection::West,
            GradientDirection::SouthEast,
        ] {
            let f = GradientOptions::new(19, 11)
                .with_stops(stops.clone())
                .with_direction(dir)
                .with_colorspace(GradientColorspace::Linear)
                .field(GradientKind::Linear)
                .unwrap();
            let img = canvas::render(&f, &RenderConfig::new().with_depth(SampleDepth::F32));
            for y in 0..11 {
                for x in 0..19 {
                    let want = f.color_at(f.parameter(x, y));
                    let got = img.pixel(x, y);
                    for c in 0..4 {
                        assert_eq!(got[c], f64::from(want[c] as f32), "{dir:?} ({x}, {y})");
                    }
                }
            }
        }
    }

    #[test]
    fn threads_and_bands_do_not_change_any_gradient() {
        let stops = GradientStop::parse_list("red, white 30%, blue").unwrap();
        for kind in [
            GradientKind::Linear,
            GradientKind::Radial,
            GradientKind::Conic,
        ] {
            for angle in [0.0, 90.0, 37.0] {
                let f = GradientOptions::new(33, 29)
                    .with_stops(stops.clone())
                    .with_angle(angle)
                    .with_spread(GradientSpread::Reflect)
                    .field(kind)
                    .unwrap();
                for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F32] {
                    let cfg = RenderConfig::new()
                        .with_depth(depth)
                        .with_dither(Dither::Ordered);
                    let serial = canvas::render(&f, &cfg);
                    assert_eq!(serial, canvas::render(&f, &cfg.with_threads(4)));
                }
            }
        }
    }

    #[test]
    fn dithered_ramp_keeps_the_column_means() {
        // A 0 → 4/255 ramp over 256 columns: undithered it is five flat
        // bands; dithered, each column's mean follows the ramp.
        let f = east(256, 64)
            .with_colors([0, 0, 0, 255], [4, 4, 4, 255])
            .field(GradientKind::Linear)
            .unwrap();
        let cfg = RenderConfig::new().with_channels(Channels::Gray);
        for dither in [Dither::Ordered, Dither::Noise] {
            let img = canvas::render(&f, &cfg.with_dither(dither));
            let v = img.as_u8().unwrap();
            for x in (0..256).step_by(8) {
                // Mean over an 8-wide strip, all 64 rows.
                let mut sum = 0.0;
                for y in 0..64 {
                    for dx in 0..8 {
                        sum += f64::from(v[y * 256 + x + dx]);
                    }
                }
                let want = (x as f64 + 3.5) / 255.0 * 4.0;
                assert!((sum / 512.0 - want).abs() < 0.08, "{dither:?} x={x}");
            }
        }
    }

    #[test]
    fn uri_keys_select_depth_layout_and_stops() {
        let img = render_image(&map(&[
            ("stops", "black,white"),
            ("direction", "East"),
            ("w", "3"),
            ("h", "1"),
            ("depth", "16"),
            ("channels", "gray"),
        ]))
        .unwrap();
        assert_eq!(img.as_u16().unwrap(), &[0, 32768, 65535]);
        let img = render_image(&map(&[
            ("from", "black"),
            ("to", "white"),
            ("direction", "East"),
            ("w", "3"),
            ("h", "1"),
            ("depth", "float"),
            ("channels", "rgb"),
        ]))
        .unwrap();
        assert_eq!(
            img.as_f32().unwrap(),
            &[0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0]
        );
    }

    // ── the table-encoded integer path ──

    #[test]
    fn encode_table_equals_the_reference_function() {
        let table = EncodeTable::get();
        // Every step position, and the doubles either side of it.
        for k in 1..=65535usize {
            let v = table.thresholds[k];
            for probe in [v.next_down(), v, v.next_up()] {
                assert_eq!(
                    table.encode(probe),
                    EncodeTable::reference(probe),
                    "around threshold {k}"
                );
            }
            assert_eq!(EncodeTable::reference(v), k as u16);
            assert_eq!(EncodeTable::reference(v.next_down()), k as u16 - 1);
        }
        // Bucket edges and out-of-range inputs.
        for b in 0..=65536u32 {
            for v in [f64::from(b) / 65536.0, f64::from(b) / 1_048_576.0] {
                assert_eq!(table.encode(v), EncodeTable::reference(v));
                assert_eq!(
                    table.encode(v.next_down()),
                    EncodeTable::reference(v.next_down())
                );
            }
        }
        // No bucket spans more codes than a lookup tests without a loop.
        assert!(
            table.max_steps <= EncodeTable::UNROLLED_STEPS,
            "{}",
            table.max_steps
        );
        for v in [-1.0, -0.0, 0.0, 1e-300, 1.0, 1.0000001, 7.0, f64::NAN] {
            assert_eq!(table.encode(v), EncodeTable::reference(v), "{v}");
        }
        // A dense pseudo-random sweep.
        let mut state = 0x0123_4567_89AB_CDEFu64;
        for _ in 0..400_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let v = (state >> 11) as f64 / (1u64 << 53) as f64;
            // Half the probes crowd the steep dark end.
            for probe in [v, v * v * v] {
                assert_eq!(
                    table.encode(probe),
                    EncodeTable::reference(probe),
                    "{probe}"
                );
            }
        }
    }

    #[test]
    fn table_path_renders_what_the_float_path_quantises_to() {
        let stops =
            GradientStop::parse_list("#102030, gold 40%, rgba(0,128,255,0.5), white").unwrap();
        for space in [
            GradientColorspace::Linear,
            GradientColorspace::Oklab,
            GradientColorspace::Oklch,
            GradientColorspace::Lab,
            GradientColorspace::Lch,
        ] {
            for kind in [
                GradientKind::Linear,
                GradientKind::Radial,
                GradientKind::Conic,
            ] {
                // 300 × 240 is above the size at which the table is used.
                let f = GradientOptions::new(300, 240)
                    .with_stops(stops.clone())
                    .with_angle(27.0)
                    .with_colorspace(space)
                    .with_spread(GradientSpread::Reflect)
                    .with_bounding_box(BoundingBox::new(280, 200, 7, 11))
                    .field(kind)
                    .unwrap();
                let mut probe = vec![[0u16; 4]; 300];
                assert!(f.fill_row_q16(0, 0, &mut probe), "table path not taken");
                for depth in [SampleDepth::U8, SampleDepth::U16] {
                    let cfg = RenderConfig::new().with_depth(depth);
                    let img = canvas::render(&f, &cfg);
                    for y in 0..240 {
                        for x in 0..300 {
                            let want = if (7..287).contains(&x) && (11..211).contains(&y) {
                                f.color_at(f.parameter(x, y))
                            } else {
                                f.ramp.outside
                            };
                            let i = ((y * 300 + x) * 4) as usize;
                            for (c, component) in want.iter().enumerate() {
                                let q = canvas::unit_to_q16(*component);
                                match depth {
                                    SampleDepth::U16 => {
                                        assert_eq!(img.as_u16().unwrap()[i + c], q)
                                    }
                                    _ => assert_eq!(
                                        u32::from(img.as_u8().unwrap()[i + c]),
                                        (u32::from(q) + 128) / 257
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn sixteen_bit_hex_stops_keep_their_precision() {
        let q = map(&[
            ("from", "#000100020003"),
            ("to", "#fffefffdfffc"),
            ("direction", "East"),
            ("w", "2"),
            ("h", "1"),
            ("depth", "16"),
            ("channels", "rgb"),
        ]);
        let img = render_image(&q).unwrap();
        assert_eq!(img.as_u16().unwrap(), &[1, 2, 3, 0xfffe, 0xfffd, 0xfffc]);
        let stops = GradientStop::parse_list("#0000ffff8000 25%, #ffff").unwrap();
        assert_eq!(stops[0].color, [0.0, 1.0, 32768.0 / 65535.0, 1.0]);
        assert_eq!(stops[0].offset, Some(0.25));
        // 8-bit colours stay in `from` / `to`.
        let plain =
            GradientOptions::from_query(&map(&[("from", "red"), ("to", "#0000ff")])).unwrap();
        assert!(plain.stops.is_empty());
    }
}

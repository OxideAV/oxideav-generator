//! Linear and radial two-stop gradients with the ImageMagick 7
//! pseudo-image option surface (`gradient:` / `radial-gradient:` and
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

use super::Rgba8Image;
use super::palette::parse_color;
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
}

impl GradientExtent {
    /// Case-insensitive parse of the ImageMagick names.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "circle" => Self::Circle,
            "diagonal" => Self::Diagonal,
            "ellipse" => Self::Ellipse,
            "maximum" | "max" => Self::Maximum,
            "minimum" | "min" => Self::Minimum,
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
        }
    }

    /// `(rx, ry)` for a `w × h` canvas.
    pub fn radii(self, w: u32, h: u32) -> (f64, f64) {
        let hx = (w.max(1) - 1) as f64 / 2.0;
        let hy = (h.max(1) - 1) as f64 / 2.0;
        match self {
            Self::Circle | Self::Maximum => (hx.max(hy), hx.max(hy)),
            Self::Minimum => (hx.min(hy), hx.min(hy)),
            Self::Ellipse => (hx, hy),
            Self::Diagonal => {
                let d = (hx * hx + hy * hy).sqrt();
                (d, d)
            }
        }
    }
}

/// Colour space the two stops are interpolated in.
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
}

impl GradientColorspace {
    /// `srgb` (default) or `rgb` / `linear` (ImageMagick calls the
    /// linear-light space `RGB`).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "srgb" | "gamma" => Self::Srgb,
            "rgb" | "linear" | "linear-rgb" | "linearrgb" | "linear_rgb" => Self::Linear,
            _ => return None,
        })
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

    fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
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
        }
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
                    "gradient: unknown extent {e:?} (expected Circle|Diagonal|Ellipse|Maximum|Minimum)"
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
                    "gradient: unknown colorspace {cs:?} (expected srgb|rgb|linear)"
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
        let (ex, ey) = self
            .extent
            .unwrap_or(GradientExtent::Circle)
            .radii(self.width, self.height);
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

/// `generate://gradient?…` entry point: `type=linear` (default) or
/// `type=radial`, every other key per [`GradientOptions::from_query`].
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let opts = GradientOptions::from_query(query)?;
    match q_str(query, "type", "linear") {
        "linear" => render_linear(&opts),
        "radial" | "radial-gradient" | "radial_gradient" => render_radial(&opts),
        other => Err(Error::invalid(format!(
            "gradient: unknown type {other:?} (expected linear|radial)"
        ))),
    }
}

/// Render a linear two-stop gradient.
pub fn render_linear(opts: &GradientOptions) -> Result<Rgba8Image> {
    let bbox = opts.checked_bbox()?;
    let mixer = Mixer::new(opts);
    let w = opts.width;
    let h = opts.height;
    let mut img = Rgba8Image::new(w, h);

    enum Mode {
        Vector {
            x1: f64,
            y1: f64,
            dx: f64,
            dy: f64,
            len2: f64,
        },
        Angle {
            dx: f64,
            dy: f64,
            min: f64,
            range: f64,
        },
    }
    let mode = match opts.linear_vector() {
        Some([x1, y1, x2, y2]) => {
            let dx = x2 - x1;
            let dy = y2 - y1;
            let len2 = dx * dx + dy * dy;
            Mode::Vector {
                x1,
                y1,
                dx,
                dy,
                len2,
            }
        }
        None => {
            let theta = opts.angle.unwrap_or(0.0).to_radians();
            // Quantise the direction to a 1e-12 grid. This snaps
            // floating-point dust (cos 90° ≈ 6e-17) to zero, so a cardinal
            // angle on a one-pixel axis stays degenerate (one colour), and
            // it makes the result independent of the platform libm: sin /
            // cos may differ by an ulp between targets, which would flip
            // exact-half cases such as t = 5/6 at 45° on 5x3.
            let snap = |v: f64| (v * 1e12).round() / 1e12;
            let dx = snap(theta.sin());
            let dy = snap(-theta.cos());
            let r = (w - 1) as f64;
            let b = (h - 1) as f64;
            let corners = [0.0, r * dx, b * dy, r * dx + b * dy];
            let min = corners.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = corners.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let range = max - min;
            Mode::Angle { dx, dy, min, range }
        }
    };

    for y in 0..h {
        for x in 0..w {
            if let Some(b) = bbox
                && !b.contains(x, y)
            {
                img.put(x, y, mixer.outside);
                continue;
            }
            let px = x as f64;
            let py = y as f64;
            // Plain division (not a cached reciprocal) keeps the
            // exact-half cases on the same side magick lands them.
            let t = match mode {
                Mode::Vector {
                    x1,
                    y1,
                    dx,
                    dy,
                    len2,
                } => {
                    if len2 > 0.0 {
                        ((px - x1) * dx + (py - y1) * dy) / len2
                    } else {
                        0.0
                    }
                }
                Mode::Angle { dx, dy, min, range } => {
                    if range > 0.0 {
                        (px * dx + py * dy - min) / range
                    } else {
                        0.0
                    }
                }
            };
            img.put(x, y, mixer.at(t));
        }
    }
    Ok(img)
}

/// Render a radial two-stop gradient.
pub fn render_radial(opts: &GradientOptions) -> Result<Rgba8Image> {
    let bbox = opts.checked_bbox()?;
    let mixer = Mixer::new(opts);
    let w = opts.width;
    let h = opts.height;
    let (cx, cy) = opts.radial_center();
    let (rx, ry) = opts.radial_radii();
    let mut img = Rgba8Image::new(w, h);

    // `d / r` with the zero-radius step rule.
    let norm = |d: f64, r: f64| -> f64 {
        if r > 0.0 {
            d / r
        } else if d == 0.0 {
            0.0
        } else {
            f64::INFINITY
        }
    };

    for y in 0..h {
        for x in 0..w {
            if let Some(b) = bbox
                && !b.contains(x, y)
            {
                img.put(x, y, mixer.outside);
                continue;
            }
            let nx = norm(x as f64 - cx, rx);
            let ny = norm(y as f64 - cy, ry);
            let t = (nx * nx + ny * ny).sqrt();
            img.put(x, y, mixer.at(t));
        }
    }
    Ok(img)
}

/// Two-stop colour interpolator (premultiplied alpha, optional
/// linear-light space, Q16 → 8-bit quantisation).
struct Mixer {
    /// Premultiplied components of each stop, `[r, g, b, a]` in `[0, 1]`
    /// (linear light when `linear`).
    from: [f64; 4],
    to: [f64; 4],
    linear: bool,
    /// Colour painted outside the bounding box.
    outside: [u8; 4],
}

impl Mixer {
    fn new(opts: &GradientOptions) -> Self {
        let linear = opts.colorspace == GradientColorspace::Linear;
        let prep = |c: [u8; 4]| -> [f64; 4] {
            let a = c[3] as f64 / 255.0;
            let ch = |v: u8| {
                let f = v as f64 / 255.0;
                if linear { srgb_to_linear(f) } else { f }
            };
            [ch(c[0]) * a, ch(c[1]) * a, ch(c[2]) * a, a]
        };
        let opaque = opts.from[3] == 255 && opts.to[3] == 255;
        let outside = [
            opts.from[0],
            opts.from[1],
            opts.from[2],
            if opaque { 255 } else { 0 },
        ];
        Self {
            from: prep(opts.from),
            to: prep(opts.to),
            linear,
            outside,
        }
    }

    #[inline]
    fn at(&self, t: f64) -> [u8; 4] {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        let mut p = [0f64; 4];
        for (i, v) in p.iter_mut().enumerate() {
            *v = self.from[i] + (self.to[i] - self.from[i]) * t;
        }
        let a = p[3];
        let mut out = [0u8; 4];
        for i in 0..3 {
            let v = if a > 0.0 { p[i] / a } else { 0.0 };
            let v = if self.linear { linear_to_srgb(v) } else { v };
            out[i] = quantize(v);
        }
        out[3] = quantize(a);
        out
    }
}

/// `[0, 1]` → Q16 → 8-bit, both roundings half-up (the ImageMagick
/// `-depth 8` export path).
#[inline]
fn quantize(v: f64) -> u8 {
    let q16 = (v.clamp(0.0, 1.0) * 65535.0).round() as u32;
    ((q16 + 128) / 257) as u8
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
        let err = render(&map(&[("type", "conic")])).unwrap_err();
        assert!(format!("{err}").contains("conic"));
        let err = render(&map(&[("colorspace", "lab")])).unwrap_err();
        assert!(format!("{err}").contains("lab"));
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
}

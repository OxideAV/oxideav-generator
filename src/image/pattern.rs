//! Two-colour geometric patterns.
//!
//! Every pattern is an integer predicate on the pixel coordinates —
//! `color2` where it holds, `color1` elsewhere — so the output is exact
//! at any depth and tiles seamlessly with period `size` (or `2·size`)
//! in both directions. With cell size `s` (`size`, default 32) and line
//! thickness `l` (`line`, default `max(1, s / 8)`, at most `s`):
//!
//! | `type`           | `color2` where                                               |
//! |------------------|--------------------------------------------------------------|
//! | `checkerboard`   | `⌊x/s⌋ + ⌊y/s⌋` is odd                                       |
//! | `horizontal`     | `⌊y/s⌋` is odd (horizontal bands)                            |
//! | `vertical`       | `⌊x/s⌋` is odd (vertical bands)                              |
//! | `right45`        | `⌊(x + y)/s⌋` is odd (bands rising to the right)             |
//! | `left45`         | `⌊((x − y) mod 2s)/s⌋` is odd (bands falling to the right)   |
//! | `crosshatch`     | `x mod s < l` or `y mod s < l` (square grid lines)           |
//! | `crosshatch45`   | `(x + y) mod s < l` or `(x − y) mod s < l` (diagonal grid)   |
//! | `bricks`         | mortar: `y mod c < l`, or `(x + o) mod s < l`, with course height `c = max(1, s/2)` and offset `o = s/2` on odd courses |
//! | `verticalbricks` | `bricks` with `x` and `y` exchanged                          |
//! | `dots`           | inside the disc of diameter `round(0.6·s)` centred in the cell |
//! | `circles`        | on the ring of that disc, `l` thick                          |
//! | `horizontalsaw`  | `(y + z(x)) mod s < l`, `z` the zigzag `min(t, s − 1 − t)`, `t = x mod s` |
//! | `verticalsaw`    | `horizontalsaw` with `x` and `y` exchanged                   |
//! | `gray0`…`gray100`| `M₈[y mod 8][x mod 8] < round(64·N/100)`: an ordered `N` % fill |
//!
//! `M₈` is the 8 × 8 dispersed-dot index matrix (see
//! [`canvas`](super::canvas)), so `grayN` covers exactly
//! `round(64·N/100) / 64` of the pixels of every aligned 8 × 8 block.
//!
//! Historical spellings stay valid: `checker`,
//! `horizontal_stripes` / `hstripes` / `hbars`,
//! `vertical_stripes` / `vstripes` / `vbars`.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::Rgba8Image;
use super::canvas::{self, Field, Image, RenderConfig, dispersed_dot};
use super::palette::parse_color;
use crate::source::{q_str, q_u32};

/// Pattern geometry (see the module table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternKind {
    Checkerboard,
    Horizontal,
    Vertical,
    Right45,
    Left45,
    Crosshatch,
    Crosshatch45,
    Bricks,
    VerticalBricks,
    Dots,
    Circles,
    HorizontalSaw,
    VerticalSaw,
    /// Ordered fill of this many 64ths of the pixels.
    Gray(u8),
}

impl PatternKind {
    /// Parse a pattern name (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        let k = s.trim().to_ascii_lowercase();
        Some(match k.as_str() {
            "checkerboard" | "checker" => Self::Checkerboard,
            "horizontal" | "horizontal_stripes" | "hstripes" | "hbars" => Self::Horizontal,
            "vertical" | "vertical_stripes" | "vstripes" | "vbars" => Self::Vertical,
            "right45" => Self::Right45,
            "left45" => Self::Left45,
            "crosshatch" | "grid" => Self::Crosshatch,
            "crosshatch45" => Self::Crosshatch45,
            "bricks" => Self::Bricks,
            "verticalbricks" => Self::VerticalBricks,
            "dots" => Self::Dots,
            "circles" => Self::Circles,
            "horizontalsaw" => Self::HorizontalSaw,
            "verticalsaw" => Self::VerticalSaw,
            _ => {
                let digits = k.strip_prefix("gray").or_else(|| k.strip_prefix("grey"))?;
                if digits.is_empty() || digits.len() > 3 {
                    return None;
                }
                let percent: u32 = digits.parse().ok()?;
                if percent > 100 {
                    return None;
                }
                Self::Gray(((64 * percent + 50) / 100) as u8)
            }
        })
    }

    /// Names accepted by [`PatternKind::parse`] (one per geometry;
    /// `grayN` stands for `gray0` … `gray100`).
    pub const NAMES: &'static [&'static str] = &[
        "checkerboard",
        "horizontal",
        "vertical",
        "right45",
        "left45",
        "crosshatch",
        "crosshatch45",
        "bricks",
        "verticalbricks",
        "dots",
        "circles",
        "horizontalsaw",
        "verticalsaw",
        "grayN",
    ];
}

/// A two-colour pattern as a [`Field`]. Build with
/// [`PatternField::new`] or [`PatternField::from_query`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct PatternField {
    pub width: u32,
    pub height: u32,
    pub kind: PatternKind,
    /// Cell size `s` in pixels (at least 1).
    pub size: u32,
    /// Line thickness `l` in pixels (`1..=size`).
    pub line: u32,
    /// Background, straight-alpha unit-range RGBA.
    pub color1: [f64; 4],
    /// Foreground.
    pub color2: [f64; 4],
}

impl PatternField {
    /// Black / white pattern with the default line thickness.
    pub fn new(width: u32, height: u32, kind: PatternKind, size: u32) -> Self {
        let size = size.max(1);
        Self {
            width: width.max(1),
            height: height.max(1),
            kind,
            size,
            line: (size / 8).max(1),
            color1: [0.0, 0.0, 0.0, 1.0],
            color2: [1.0, 1.0, 1.0, 1.0],
        }
    }

    pub fn with_colors(mut self, color1: [f64; 4], color2: [f64; 4]) -> Self {
        self.color1 = color1;
        self.color2 = color2;
        self
    }

    pub fn with_line(mut self, line: u32) -> Self {
        self.line = line.clamp(1, self.size);
        self
    }

    /// `w`, `h`, `type`, `size`, `line`, `color1`, `color2`.
    pub fn from_query(query: &BTreeMap<String, String>) -> Result<Self> {
        let kind = q_str(query, "type", "checkerboard");
        let kind = PatternKind::parse(kind).ok_or_else(|| {
            Error::invalid(format!(
                "pattern: unknown type {kind:?} (expected one of {})",
                PatternKind::NAMES.join("|")
            ))
        })?;
        let mut f = Self::new(
            q_u32(query, "w", 640)?,
            q_u32(query, "h", 480)?,
            kind,
            q_u32(query, "size", 32)?,
        );
        if query.contains_key("line") {
            f = f.with_line(q_u32(query, "line", 1)?);
        }
        let unit = |c: [u8; 4]| c.map(|v| f64::from(v) / 255.0);
        Ok(f.with_colors(
            unit(parse_color(q_str(query, "color1", "black"))?),
            unit(parse_color(q_str(query, "color2", "white"))?),
        ))
    }

    /// The pattern predicate: `true` where `color2` is painted.
    pub fn is_foreground(&self, x: u32, y: u32) -> bool {
        let (x, y) = (u64::from(x), u64::from(y));
        let s = u64::from(self.size);
        let l = u64::from(self.line);
        let bricks = |x: u64, y: u64| {
            let course_height = (s / 2).max(1);
            let course = y / course_height;
            y % course_height < l || (x + (course & 1) * (s / 2)) % s < l
        };
        let saw = |x: u64, y: u64| {
            let t = x % s;
            (y + t.min(s - 1 - t)) % s < l
        };
        // Doubled offset from the cell centre, squared.
        let centre_distance2 = || {
            let d = |v: u64| (2 * (v % s)) as i64 - (s as i64 - 1);
            let (dx, dy) = (d(x), d(y));
            (dx * dx + dy * dy) as u64
        };
        // Doubled disc radius = the diameter in pixels.
        let diameter = (6 * s + 5) / 10;
        match self.kind {
            PatternKind::Checkerboard => (x / s + y / s) & 1 == 1,
            PatternKind::Horizontal => (y / s) & 1 == 1,
            PatternKind::Vertical => (x / s) & 1 == 1,
            PatternKind::Right45 => ((x + y) / s) & 1 == 1,
            // x − y folded into [0, 2s): add a multiple of 2s that
            // exceeds y.
            PatternKind::Left45 => ((x + (2 * s - y % (2 * s))) % (2 * s)) / s == 1,
            PatternKind::Crosshatch => x % s < l || y % s < l,
            PatternKind::Crosshatch45 => (x + y) % s < l || (x + (s - y % s)) % s < l,
            PatternKind::Bricks => bricks(x, y),
            PatternKind::VerticalBricks => bricks(y, x),
            PatternKind::Dots => centre_distance2() <= diameter * diameter,
            PatternKind::Circles => {
                let d2 = centre_distance2();
                let inner = diameter.saturating_sub(2 * l);
                d2 <= diameter * diameter && (d2 > inner * inner || inner == 0)
            }
            PatternKind::HorizontalSaw => saw(x, y),
            PatternKind::VerticalSaw => saw(y, x),
            PatternKind::Gray(n) => dispersed_dot(x as u32, y as u32) < n,
        }
    }

    /// Smallest shift in `y` that maps the pattern onto itself.
    fn period_y(&self) -> u32 {
        let s = self.size;
        match self.kind {
            PatternKind::Checkerboard
            | PatternKind::Horizontal
            | PatternKind::Right45
            | PatternKind::Left45 => s.saturating_mul(2),
            PatternKind::Vertical => 1,
            PatternKind::Bricks => (s / 2).max(1).saturating_mul(2),
            PatternKind::Gray(_) => 8,
            _ => s,
        }
    }
}

impl Field for PatternField {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = if self.is_foreground(x0 + i as u32, y) {
                self.color2
            } else {
                self.color1
            };
        }
    }

    fn uniform_row(&self, y: u32) -> Option<[f64; 4]> {
        match self.kind {
            PatternKind::Horizontal => Some(if self.is_foreground(0, y) {
                self.color2
            } else {
                self.color1
            }),
            _ => None,
        }
    }

    fn row_alias(&self, y: u32) -> Option<u32> {
        y.checked_sub(self.period_y())
    }
}

/// `generate://pattern?…` as 8-bit RGBA.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let field = PatternField::from_query(query)?;
    Ok(canvas::render(&field, &RenderConfig::new()).into_rgba8())
}

/// `generate://pattern?…` in the depth / layout the query asks for.
pub fn render_image(query: &BTreeMap<String, String>) -> Result<Image> {
    let field = PatternField::from_query(query)?;
    Ok(canvas::render(&field, &RenderConfig::from_query(query)?))
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
        assert!(render(&map(&[("type", "gray101")])).is_err());
    }

    /// `#` for foreground, `.` for background.
    fn ascii(kind: &str, w: u32, h: u32, size: u32, line: u32) -> Vec<String> {
        let f = PatternField::new(w, h, PatternKind::parse(kind).unwrap(), size).with_line(line);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| if f.is_foreground(x, y) { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn small_renders_match_their_drawings() {
        assert_eq!(
            ascii("right45", 6, 4, 2, 1),
            ["..##..", ".##..#", "##..##", "#..##."]
        );
        assert_eq!(
            ascii("left45", 6, 4, 2, 1),
            ["..##..", "#..##.", "##..##", ".##..#"]
        );
        assert_eq!(
            ascii("crosshatch", 8, 4, 4, 1),
            ["########", "#...#...", "#...#...", "#...#..."]
        );
        assert_eq!(
            ascii("crosshatch45", 8, 4, 4, 1),
            ["#...#...", ".#.#.#.#", "..#...#.", ".#.#.#.#"]
        );
        assert_eq!(
            ascii("bricks", 8, 8, 8, 1),
            [
                "########", "#.......", "#.......", "#.......", "########", "....#...", "....#...",
                "....#...",
            ]
        );
        assert_eq!(
            ascii("horizontalsaw", 8, 4, 4, 1),
            ["#..##..#", "........", "........", ".##..##."]
        );
        // Diameter round(0.6 · 10) = 6: a disc of radius 3 about the
        // cell centre (4.5, 4.5).
        assert_eq!(
            ascii("dots", 10, 10, 10, 1),
            [
                "..........",
                "..........",
                "...####...",
                "..######..",
                "..######..",
                "..######..",
                "..######..",
                "...####...",
                "..........",
                "..........",
            ]
        );
        // Diameter round(0.6 · 7) = 4: the one-pixel ring of radius 2.
        assert_eq!(
            ascii("circles", 14, 7, 7, 1),
            [
                "..............",
                "...#......#...",
                "..#.#....#.#..",
                ".#...#..#...#.",
                "..#.#....#.#..",
                "...#......#...",
                "..............",
            ]
        );
    }

    #[test]
    fn transposed_kinds_are_transposes() {
        for (a, b) in [
            ("bricks", "verticalbricks"),
            ("horizontalsaw", "verticalsaw"),
            ("horizontal", "vertical"),
        ] {
            let fa = PatternField::new(24, 24, PatternKind::parse(a).unwrap(), 6);
            let fb = PatternField::new(24, 24, PatternKind::parse(b).unwrap(), 6);
            for y in 0..24 {
                for x in 0..24 {
                    assert_eq!(fa.is_foreground(x, y), fb.is_foreground(y, x), "{a}/{b}");
                }
            }
        }
    }

    #[test]
    fn gray_levels_cover_the_stated_fraction() {
        for percent in [0u32, 1, 10, 25, 50, 75, 99, 100] {
            let name = format!("gray{percent}");
            let f = PatternField::new(16, 16, PatternKind::parse(&name).unwrap(), 32);
            let on = (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|&(x, y)| f.is_foreground(x, y))
                .count() as u32;
            assert_eq!(on, 4 * ((64 * percent + 50) / 100), "{name}");
        }
        // gray50 is the one-pixel checkerboard.
        let half = PatternField::new(8, 8, PatternKind::parse("gray50").unwrap(), 1);
        let checker = PatternField::new(8, 8, PatternKind::Checkerboard, 1);
        let same = (0..64)
            .all(|i| half.is_foreground(i % 8, i / 8) == checker.is_foreground(i % 8, i / 8));
        let inverse = (0..64)
            .all(|i| half.is_foreground(i % 8, i / 8) != checker.is_foreground(i % 8, i / 8));
        assert!(same || inverse);
    }

    #[test]
    fn every_pattern_is_periodic_and_identical_at_any_depth_and_budget() {
        use crate::image::canvas::SampleDepth;
        for name in PatternKind::NAMES {
            let name = if *name == "grayN" { "gray37" } else { name };
            let kind = PatternKind::parse(name).unwrap();
            let f = PatternField::new(41, 67, kind, 6)
                .with_colors([0.25, 0.5, 0.75, 1.0], [1.0, 0.0, 0.5, 0.5]);
            // The declared row period really is one.
            for y in f.period_y()..67 {
                for x in 0..41 {
                    assert_eq!(
                        f.is_foreground(x, y),
                        f.is_foreground(x, y - f.period_y()),
                        "{name} at ({x}, {y})"
                    );
                }
            }
            for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F32] {
                let cfg = RenderConfig::new().with_depth(depth);
                let img = canvas::render(&f, &cfg);
                assert_eq!(img, canvas::render(&f, &cfg.with_threads(5)), "{name}");
                for y in 0..67 {
                    for x in 0..41 {
                        let want = if f.is_foreground(x, y) {
                            f.color2
                        } else {
                            f.color1
                        };
                        let got = img.pixel(x, y);
                        for c in 0..4 {
                            assert!((got[c] - want[c]).abs() <= 0.5 / 255.0 + 1e-9, "{name}");
                            if depth == SampleDepth::F32 {
                                assert_eq!(got[c], want[c], "{name}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn colours_and_line_come_from_the_query() {
        let img = render(&map(&[
            ("type", "crosshatch"),
            ("w", "8"),
            ("h", "8"),
            ("size", "4"),
            ("line", "2"),
            ("color1", "navy"),
            ("color2", "gold"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0), [255, 215, 0, 255]);
        assert_eq!(img.get(1, 3), [255, 215, 0, 255]);
        assert_eq!(img.get(2, 2), [0, 0, 128, 255]);
        assert_eq!(img.get(3, 3), [0, 0, 128, 255]);
    }
}

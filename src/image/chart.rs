//! Test charts for codec and pipeline work: colour bars, grey
//! staircases, ramps, a zone plate, line bursts, and a labelled
//! composite of all of them.
//!
//! Every chart is defined on unit-range values, so it is exact at 8
//! bits, 16 bits and float alike. With `x`, `y` the pixel coordinates
//! on a `w × h` canvas:
//!
//! - **`bars`** — eight equal vertical bars, in falling luminance:
//!   white, yellow, cyan, green, magenta, red, blue, black; bar
//!   `⌊8x / w⌋`. Each lit component is `level / 100` (`level=75`, the
//!   classic 75 % bars, by default; `level=100` for full amplitude).
//! - **`steps`** — a grey staircase of `steps` (default 11) flat
//!   treads: tread `i = ⌊steps · x / w⌋` has the value
//!   `i / (steps − 1)`.
//! - **`ramp`** — the continuous grey ramp `x / (w − 1)`; at 8 bits and
//!   `w = 256`, or 16 bits and `w = 65536`, column `x` holds code `x`.
//! - **`zoneplate`** — `½ + ½ cos(π r² / N)` with `r` the distance from
//!   the centre `((w − 1)/2, (h − 1)/2)` and `N = max(w, h)`: the local
//!   spatial frequency grows linearly with `r` and reaches the Nyquist
//!   limit (one cycle per two pixels) at `r = N/2`, the edge of the
//!   longer side. The cosine is evaluated without the platform's
//!   trigonometric library, so the chart is bit-identical everywhere.
//! - **`bursts`** — four equal blocks of vertical black / white lines
//!   with periods 2, 4, 8 and 16 pixels (line widths 1, 2, 4, 8).
//! - **`composite`** (default) — bars over the top 55 % of the height,
//!   then a staircase band, a ramp band (12 % each), and a bottom band
//!   split in three: zone plate, the six full-amplitude primaries and
//!   secondaries (red, green, blue, cyan, magenta, yellow), and the
//!   bursts.
//!
//! With the `label` feature, `labels=1` (the default for the composite
//! on canvases at least 160 pixels tall) captions each region; the
//! caption boxes are the only anti-aliased pixels of the chart.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::Rgba8Image;
use super::canvas::{self, Field, Image, RenderConfig};
use super::detmath;
use crate::source::{q_str, q_u32};

/// Which chart to draw (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ChartKind {
    Bars,
    Steps,
    Ramp,
    ZonePlate,
    Bursts,
    #[default]
    Composite,
}

impl ChartKind {
    /// `bars` (`colorbars`), `steps` (`staircase`), `ramp`,
    /// `zoneplate`, `bursts` (`multiburst`), `composite`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "bars" | "colorbars" | "colourbars" | "colour-bars" | "color-bars" => Self::Bars,
            "steps" | "staircase" | "grayscale" | "greyscale" => Self::Steps,
            "ramp" => Self::Ramp,
            "zoneplate" | "zone-plate" | "zone_plate" => Self::ZonePlate,
            "bursts" | "multiburst" => Self::Bursts,
            "composite" | "chart" | "testchart" => Self::Composite,
            _ => return None,
        })
    }
}

/// A caption box placed on the chart.
#[derive(Clone, Debug)]
struct Caption {
    x: u32,
    y: u32,
    image: Rgba8Image,
}

/// A test chart as a [`Field`]. Build with [`ChartField::new`] or
/// [`ChartField::from_query`].
#[derive(Clone, Debug)]
pub struct ChartField {
    width: u32,
    height: u32,
    kind: ChartKind,
    /// Bar amplitude, unit range.
    level: f64,
    steps: u32,
    captions: Vec<Caption>,
}

/// A sub-rectangle of the canvas.
#[derive(Clone, Copy)]
struct Region {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

const WHITE: [f64; 4] = [1.0, 1.0, 1.0, 1.0];
const BLACK: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

fn grey(v: f64) -> [f64; 4] {
    [v, v, v, 1.0]
}

impl ChartField {
    /// An uncaptioned chart with 75 % bars and an 11-step staircase.
    pub fn new(width: u32, height: u32, kind: ChartKind) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            kind,
            level: 0.75,
            steps: 11,
            captions: Vec::new(),
        }
    }

    /// Bar amplitude as a fraction (`0.75` for 75 % bars).
    pub fn with_level(mut self, level: f64) -> Self {
        self.level = level.clamp(0.0, 1.0);
        self
    }

    /// Number of staircase treads (2–65536).
    pub fn with_steps(mut self, steps: u32) -> Self {
        self.steps = steps.clamp(2, 65536);
        self
    }

    /// Caption each region (needs the `label` feature; a chart too
    /// small for a caption simply has none).
    #[cfg(feature = "label")]
    pub fn with_captions(mut self) -> Result<Self> {
        let size = (self.height / 36).clamp(9, 28);
        let mut captions = Vec::new();
        for (region, text) in self.caption_plan() {
            let mut q = BTreeMap::new();
            q.insert("text".to_string(), text);
            q.insert("size".to_string(), size.to_string());
            q.insert("color".to_string(), "white".to_string());
            q.insert("bg".to_string(), "rgba(0,0,0,0.6)".to_string());
            q.insert("padding".to_string(), "2".to_string());
            let image = super::label::render(&q)?;
            if image.width + 4 <= region.w && image.height + 4 <= region.h {
                captions.push(Caption {
                    x: region.x + 2,
                    y: region.y + 2,
                    image,
                });
            }
        }
        self.captions = captions;
        Ok(self)
    }

    /// `w`, `h`, `type`, `level` (percent), `steps`, `labels`.
    pub fn from_query(query: &BTreeMap<String, String>) -> Result<Self> {
        let t = q_str(query, "type", "composite");
        let kind = ChartKind::parse(t).ok_or_else(|| {
            Error::invalid(format!(
                "chart: unknown type {t:?} (expected bars|steps|ramp|zoneplate|bursts|composite)"
            ))
        })?;
        let level = q_u32(query, "level", 75)?;
        if !(1..=100).contains(&level) {
            return Err(Error::invalid(format!(
                "chart: level must be 1..=100 percent, got {level}"
            )));
        }
        let steps = q_u32(query, "steps", 11)?;
        if !(2..=65536).contains(&steps) {
            return Err(Error::invalid(format!(
                "chart: steps must be 2..=65536, got {steps}"
            )));
        }
        let field = Self::new(q_u32(query, "w", 640)?, q_u32(query, "h", 480)?, kind)
            .with_level(f64::from(level) / 100.0)
            .with_steps(steps);
        let default_labels = kind == ChartKind::Composite && field.height >= 160;
        let labels = match query.get("labels").map(|s| s.as_str()) {
            None => default_labels,
            Some("1" | "true" | "yes" | "on") => true,
            Some("0" | "false" | "no" | "off") => false,
            Some(other) => {
                return Err(Error::invalid(format!(
                    "chart: labels must be 0 or 1, got {other:?}"
                )));
            }
        };
        #[cfg(feature = "label")]
        if labels {
            return field.with_captions();
        }
        let _ = labels;
        Ok(field)
    }

    fn whole(&self) -> Region {
        Region {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        }
    }

    /// The composite's regions: bars, steps, ramp, zone plate, patches,
    /// bursts. Boundaries are `⌊h · p / 100⌋` and `⌊w · k / 3⌋`.
    fn composite_regions(&self) -> [Region; 6] {
        let (w, h) = (u64::from(self.width), u64::from(self.height));
        let row = |p: u64| (h * p / 100) as u32;
        let col = |k: u64| (w * k / 3) as u32;
        let band = |y0: u32, y1: u32, x0: u32, x1: u32| Region {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        };
        [
            band(0, row(55), 0, self.width),
            band(row(55), row(67), 0, self.width),
            band(row(67), row(79), 0, self.width),
            band(row(79), self.height, 0, col(1)),
            band(row(79), self.height, col(1), col(2)),
            band(row(79), self.height, col(2), self.width),
        ]
    }

    #[cfg(feature = "label")]
    fn caption_plan(&self) -> Vec<(Region, String)> {
        let percent = (self.level * 100.0).round() as u32;
        let bars = format!("{percent}% bars");
        let steps = format!("{} steps", self.steps);
        match self.kind {
            ChartKind::Bars => vec![(self.whole(), bars)],
            ChartKind::Steps => vec![(self.whole(), steps)],
            ChartKind::Ramp => vec![(self.whole(), "ramp".to_string())],
            ChartKind::ZonePlate => vec![(self.whole(), "zone plate".to_string())],
            ChartKind::Bursts => vec![(self.whole(), "bursts 2 4 8 16".to_string())],
            ChartKind::Composite => {
                let r = self.composite_regions();
                vec![
                    (r[0], format!("{bars}  {}x{}", self.width, self.height)),
                    (r[1], steps),
                    (r[2], "ramp".to_string()),
                    (r[3], "zone plate".to_string()),
                    (r[4], "100%".to_string()),
                    (r[5], "bursts".to_string()),
                ]
            }
        }
    }

    fn bars(&self, r: Region, x: u32) -> [f64; 4] {
        // Bit 2 = green, bit 1 = red, bit 0 = blue, counting down from
        // white: the order of falling luminance.
        let bar = 7 - (u64::from(x - r.x) * 8 / u64::from(r.w)) as u32;
        let on = |bit: u32| {
            if bar >> bit & 1 == 1 { self.level } else { 0.0 }
        };
        [on(1), on(2), on(0), 1.0]
    }

    fn steps(&self, r: Region, x: u32) -> [f64; 4] {
        let tread = u64::from(x - r.x) * u64::from(self.steps) / u64::from(r.w);
        grey(tread as f64 / f64::from(self.steps - 1))
    }

    fn ramp(r: Region, x: u32) -> [f64; 4] {
        if r.w <= 1 {
            return BLACK;
        }
        grey(f64::from(x - r.x) / f64::from(r.w - 1))
    }

    fn zone_plate(r: Region, x: u32, y: u32) -> [f64; 4] {
        let dx = f64::from(x - r.x) - f64::from(r.w - 1) / 2.0;
        let dy = f64::from(y - r.y) - f64::from(r.h - 1) / 2.0;
        // Phase π r² / N, as a fraction of a turn: r² / 2N.
        let turns = (dx * dx + dy * dy) / (2.0 * f64::from(r.w.max(r.h)));
        grey(0.5 + 0.5 * detmath::cos_sin_turns(turns).0)
    }

    fn patches(r: Region, x: u32) -> [f64; 4] {
        const PATCHES: [[f64; 4]; 6] = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        ];
        PATCHES[(u64::from(x - r.x) * 6 / u64::from(r.w)) as usize]
    }

    fn bursts(r: Region, x: u32) -> [f64; 4] {
        let local = x - r.x;
        let block = (u64::from(local) * 4 / u64::from(r.w)) as u32;
        let block_start = (u64::from(r.w) * u64::from(block)).div_ceil(4) as u32;
        // Line width 1, 2, 4, 8.
        if ((local - block_start) >> block) & 1 == 1 {
            WHITE
        } else {
            BLACK
        }
    }

    /// The chart colour at `(x, y)` before captions.
    pub fn base(&self, x: u32, y: u32) -> [f64; 4] {
        let whole = self.whole();
        match self.kind {
            ChartKind::Bars => self.bars(whole, x),
            ChartKind::Steps => self.steps(whole, x),
            ChartKind::Ramp => Self::ramp(whole, x),
            ChartKind::ZonePlate => Self::zone_plate(whole, x, y),
            ChartKind::Bursts => Self::bursts(whole, x),
            ChartKind::Composite => {
                let regions = self.composite_regions();
                let i = regions
                    .iter()
                    .position(|r| x >= r.x && x - r.x < r.w && y >= r.y && y - r.y < r.h)
                    .unwrap_or(0);
                let r = regions[i];
                if r.w == 0 || r.h == 0 {
                    return BLACK;
                }
                match i {
                    0 => self.bars(r, x),
                    1 => self.steps(r, x),
                    2 => Self::ramp(r, x),
                    3 => Self::zone_plate(r, x, y),
                    4 => Self::patches(r, x),
                    _ => Self::bursts(r, x),
                }
            }
        }
    }

    /// No caption touches row `y`.
    fn row_is_plain(&self, y: u32) -> bool {
        !self
            .captions
            .iter()
            .any(|c| y >= c.y && y - c.y < c.image.height)
    }
}

impl Field for ChartField {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.base(x0 + i as u32, y);
        }
        for c in &self.captions {
            if y < c.y || y - c.y >= c.image.height {
                continue;
            }
            for cx in 0..c.image.width {
                let x = c.x + cx;
                if x < x0 || (x - x0) as usize >= out.len() {
                    continue;
                }
                let p = c.image.get(cx, y - c.y);
                let a = f64::from(p[3]) / 255.0;
                let o = &mut out[(x - x0) as usize];
                for k in 0..3 {
                    o[k] = o[k] * (1.0 - a) + f64::from(p[k]) / 255.0 * a;
                }
            }
        }
    }

    fn row_alias(&self, y: u32) -> Option<u32> {
        // Every region but the zone plate is constant down its
        // columns.
        let prev = y.checked_sub(1)?;
        let same_band = match self.kind {
            ChartKind::ZonePlate => false,
            ChartKind::Composite => {
                let r = self.composite_regions();
                let band = |row: u32| r.iter().take(4).rposition(|b| row >= b.y);
                band(y) == band(prev) && y < r[3].y
            }
            _ => true,
        };
        (same_band && self.row_is_plain(y) && self.row_is_plain(prev)).then_some(prev)
    }
}

/// `generate://chart?…` as 8-bit RGBA.
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let field = ChartField::from_query(query)?;
    Ok(canvas::render(&field, &RenderConfig::new()).into_rgba8())
}

/// `generate://chart?…` in the depth / layout the query asks for.
pub fn render_image(query: &BTreeMap<String, String>) -> Result<Image> {
    let field = ChartField::from_query(query)?;
    Ok(canvas::render(&field, &RenderConfig::from_query(query)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::canvas::{Channels, SampleDepth};

    fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn bars_have_the_documented_order_and_levels() {
        let img = render(&map(&[("type", "bars"), ("w", "16"), ("h", "2")])).unwrap();
        let want: [[u8; 3]; 8] = [
            [191, 191, 191],
            [191, 191, 0],
            [0, 191, 191],
            [0, 191, 0],
            [191, 0, 191],
            [191, 0, 0],
            [0, 0, 191],
            [0, 0, 0],
        ];
        for (bar, rgb) in want.iter().enumerate() {
            for x in [2 * bar as u32, 2 * bar as u32 + 1] {
                assert_eq!(img.get(x, 1)[..3], rgb[..]);
            }
        }
        // Luminance falls from bar to bar.
        let luma = |p: &[u8; 3]| {
            0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
        };
        assert!(want.windows(2).all(|w| luma(&w[0]) > luma(&w[1])));
        // Full amplitude, 16-bit: 65535; 75 % at 16-bit: round(49151.25).
        let q = |level: &str| {
            render_image(&map(&[
                ("type", "bars"),
                ("level", level),
                ("w", "8"),
                ("h", "1"),
                ("depth", "16"),
                ("channels", "rgb"),
            ]))
            .unwrap()
        };
        assert_eq!(
            q("100").as_u16().unwrap()[..6],
            [65535, 65535, 65535, 65535, 65535, 0]
        );
        assert_eq!(q("75").as_u16().unwrap()[..3], [49151; 3][..]);
        let float = render_image(&map(&[
            ("type", "bars"),
            ("w", "8"),
            ("h", "1"),
            ("depth", "float"),
            ("channels", "rgb"),
        ]))
        .unwrap();
        assert_eq!(
            float.as_f32().unwrap()[..6],
            [0.75, 0.75, 0.75, 0.75, 0.75, 0.0]
        );
    }

    #[test]
    fn staircase_and_ramp_hit_exact_codes() {
        let steps = canvas::render(
            &ChartField::new(22, 1, ChartKind::Steps),
            &RenderConfig::new().with_channels(Channels::Gray),
        );
        // Eleven treads of two pixels: 0, 10 %, …, 100 %.
        let want: Vec<u8> = (0..11u32)
            .flat_map(|i| [((i * 255 + 5) / 10) as u8; 2])
            .collect();
        assert_eq!(steps.as_u8().unwrap(), &want[..]);
        let ramp = canvas::render(
            &ChartField::new(256, 2, ChartKind::Ramp),
            &RenderConfig::new().with_channels(Channels::Gray),
        );
        for (x, v) in ramp.as_u8().unwrap()[..256].iter().enumerate() {
            assert_eq!(usize::from(*v), x);
        }
        let deep = canvas::render(
            &ChartField::new(65536, 1, ChartKind::Ramp),
            &RenderConfig::new()
                .with_channels(Channels::Gray)
                .with_depth(SampleDepth::U16),
        );
        for (x, v) in deep.as_u16().unwrap().iter().enumerate() {
            assert_eq!(usize::from(*v), x);
        }
        let two = canvas::render(
            &ChartField::new(4, 1, ChartKind::Steps).with_steps(2),
            &RenderConfig::new().with_channels(Channels::Gray),
        );
        assert_eq!(two.as_u8().unwrap(), &[0, 0, 255, 255]);
    }

    #[test]
    fn zone_plate_is_symmetric_peaks_at_the_centre_and_reaches_nyquist() {
        let f = ChartField::new(65, 65, ChartKind::ZonePlate);
        assert_eq!(f.base(32, 32), grey(1.0));
        for y in 0..65 {
            for x in 0..65 {
                let v = f.base(x, y);
                assert_eq!(v, f.base(64 - x, y));
                assert_eq!(v, f.base(x, 64 - y));
                assert_eq!(v, f.base(y, x));
                assert!((0.0..=1.0).contains(&v[0]));
            }
        }
        // Along the centre row the phase is π·dx²/65: compare with the
        // library cosine.
        for x in 0..65u32 {
            let dx = f64::from(x) - 32.0;
            let want = 0.5 + 0.5 * (std::f64::consts::PI * dx * dx / 65.0).cos();
            assert!((f.base(x, 32)[0] - want).abs() < 1e-12);
        }
        // Phase step between neighbouring pixels at the edge is close
        // to π (Nyquist): (32² − 31²)·π/65 = 63π/65.
        let edge = (32.0f64 * 32.0 - 31.0 * 31.0) / 65.0;
        assert!((edge - 1.0).abs() < 0.04);
    }

    #[test]
    fn bursts_have_line_widths_one_two_four_eight() {
        let f = ChartField::new(64, 1, ChartKind::Bursts);
        let row: String = (0..64)
            .map(|x| if f.base(x, 0) == WHITE { '#' } else { '.' })
            .collect();
        assert_eq!(
            row,
            ".#.#.#.#.#.#.#.#..##..##..##..##....####....####........########"
        );
    }

    #[test]
    fn composite_regions_hold_their_charts() {
        let f = ChartField::new(300, 200, ChartKind::Composite);
        // Bars over the top 55 %.
        assert_eq!(f.base(0, 0), grey(0.75));
        assert_eq!(f.base(299, 109), BLACK);
        assert_eq!(f.base(40, 50), [0.75, 0.75, 0.0, 1.0]);
        // Staircase rows 110..134, ramp rows 134..158.
        assert_eq!(f.base(0, 110), BLACK);
        assert_eq!(f.base(299, 133), WHITE);
        assert_eq!(f.base(150, 120), grey(0.5));
        assert_eq!(f.base(0, 134), BLACK);
        assert_eq!(f.base(299, 157), WHITE);
        assert_eq!(f.base(100, 140), grey(100.0 / 299.0));
        // Bottom band: zone plate, patches, bursts.
        let zp = f.base(50, 178);
        assert!(zp[0] == zp[1] && zp[1] == zp[2]);
        assert_eq!(f.base(100, 190), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(f.base(199, 160), [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(f.base(200, 199), BLACK);
        assert_eq!(f.base(201, 199), WHITE);
    }

    #[test]
    fn charts_are_identical_at_any_thread_budget_and_round_consistently() {
        for kind in ["bars", "steps", "ramp", "zoneplate", "bursts", "composite"] {
            let mut q = map(&[("type", kind), ("w", "200"), ("h", "180")]);
            let low = render_image(&q).unwrap();
            q.insert("threads".into(), "4".into());
            assert_eq!(low, render_image(&q).unwrap(), "{kind}");
            q.insert("depth".into(), "16".into());
            let deep = render_image(&q).unwrap();
            q.insert("threads".into(), "1".into());
            assert_eq!(deep, render_image(&q).unwrap(), "{kind}");
            for (a, b) in low.as_u8().unwrap().iter().zip(deep.as_u16().unwrap()) {
                assert_eq!(u32::from(*a), (u32::from(*b) + 128) / 257, "{kind}");
            }
        }
    }

    #[cfg(feature = "label")]
    #[test]
    fn captions_mark_only_their_boxes() {
        let plain = render(&map(&[("w", "320"), ("h", "240"), ("labels", "0")])).unwrap();
        let labelled = render(&map(&[("w", "320"), ("h", "240")])).unwrap();
        assert_ne!(plain.pixels, labelled.pixels);
        let differing = plain
            .pixels
            .chunks(4)
            .zip(labelled.pixels.chunks(4))
            .filter(|(a, b)| a != b)
            .count();
        // Six small boxes: a few percent of the canvas at most.
        assert!(differing > 200 && differing < 320 * 240 / 8, "{differing}");
        // The bottom-right pixel of every region is caption-free.
        assert_eq!(plain.get(319, 239), labelled.get(319, 239));
        assert_eq!(plain.get(319, 100), labelled.get(319, 100));
        // Small canvases get no captions by default.
        let small = render(&map(&[("w", "64"), ("h", "48")])).unwrap();
        let small_plain = render(&map(&[("w", "64"), ("h", "48"), ("labels", "0")])).unwrap();
        assert_eq!(small.pixels, small_plain.pixels);
    }

    #[test]
    fn bad_parameters_are_refused() {
        assert!(render(&map(&[("type", "pal")])).is_err());
        assert!(render(&map(&[("level", "0")])).is_err());
        assert!(render(&map(&[("steps", "1")])).is_err());
        assert!(render(&map(&[("labels", "maybe")])).is_err());
    }
}

//! Pseudo-image specifications: build a still image from a short
//! string such as `gradient:red-blue`, `radial-gradient:white-black`,
//! `plasma:fractal`, `xc:#336699`, `hald:8` or `pattern:bricks`, the
//! way image tools name their built-in sources.
//!
//! A specification is `name:arguments`. The name selects the
//! generator, the arguments are its positional form (colours, a type
//! name, a level), and everything else — size, per-generator options,
//! seed, output depth — is set on the [`PseudoImage`] record:
//!
//! ```
//! use oxideav_generator::pseudo::PseudoImage;
//!
//! let img = PseudoImage::parse("gradient:red-blue")?
//!     .with_size(64, 32)
//!     .with_define("gradient:angle", "90")?
//!     .render()?;
//! assert_eq!((img.width, img.height), (64, 32));
//! assert_eq!(img.as_u8().unwrap()[..4], [255, 0, 0, 255]);
//! # Ok::<(), oxideav_core::Error>(())
//! ```
//!
//! # Names
//!
//! | specification                         | image                                           |
//! |---------------------------------------|-------------------------------------------------|
//! | `xc:COLOUR`, `canvas:COLOUR`          | solid colour                                    |
//! | `gradient:`, `gradient:C1-C2`, `gradient:C1-C2-C3…` | linear gradient (default white → black; three or more colours are evenly spaced stops) |
//! | `radial-gradient:…`                   | radial gradient, first colour at the centre     |
//! | `conic-gradient:…`                    | conic gradient, clockwise from North            |
//! | `plasma:`, `plasma:C1-C2`, `plasma:fractal` | plasma clouds over a gradient / random corner colours |
//! | `hald:N`                              | identity colour lookup image of level `N`       |
//! | `pattern:NAME`                        | two-colour pattern ([`crate::image::pattern`])  |
//! | `noise:perlin` \| `simplex` \| `value` \| `worley` | lattice / cellular noise           |
//! | `random:TYPE`, `noise:TYPE`           | per-pixel noise: `random`, `uniform`, `gaussian`, `impulse`, `laplacian`, `poisson`, `multiplicative` |
//! | `chart:TYPE`                          | `bars`, `steps`, `ramp`, `zoneplate`, `bursts`, `composite` |
//! | `mandelbrot:`, `julia:`, `ramp:`, `grating:`, `label:TEXT` | the remaining still generators |
//!
//! # Defines
//!
//! [`PseudoImage::with_define`] takes the option vocabulary used with
//! these names — `gradient:angle`, `gradient:vector`,
//! `gradient:direction`, `gradient:extent`, `gradient:bounding-box`,
//! `gradient:center`, `gradient:radii` — and, without a namespace, any
//! key of the generator's `generate://` query (`stops`, `spread`,
//! `colorspace`, `hue`, `shape`, `size`, `amount`, …). A namespaced define
//! addressed to a different generator (for example `gradient:angle` on
//! an `xc:` image) is accepted and ignored, as a define is a setting
//! of the session rather than of one image.
//!
//! # Reproducibility
//!
//! Seeded generators (`plasma`, `noise`, `random`) take their seed
//! from [`PseudoImage::with_seed`]; the same specification, size and
//! seed always produce the same samples.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use crate::image::canvas::{Channels, Dither, Image, RenderConfig, SampleDepth};
use crate::source::ParsedUri;

/// A parsed pseudo-image specification plus its settings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PseudoImage {
    kind: String,
    query: BTreeMap<String, String>,
}

/// Define namespaces and the generator kinds they address.
const NAMESPACES: &[(&str, &[&str])] = &[
    ("gradient", &["gradient", "plasma"]),
    ("radial-gradient", &["gradient"]),
    ("conic-gradient", &["gradient"]),
    ("plasma", &["plasma"]),
    ("pattern", &["pattern"]),
    ("noise", &["noise", "random"]),
    ("random", &["random"]),
    ("chart", &["chart"]),
    ("hald", &["hald"]),
    ("xc", &["xc"]),
    ("canvas", &["xc"]),
    ("label", &["label"]),
];

impl PseudoImage {
    /// Parse a `name:arguments` specification. The size defaults to
    /// 640 × 480 (`hald:` sizes itself); set it with
    /// [`PseudoImage::with_size`].
    pub fn parse(spec: &str) -> Result<Self> {
        let spec = spec.trim();
        let uri = crate::shorthand::translate(spec);
        // `translate` passes unknown input through unchanged.
        if !uri.starts_with("generate://") {
            return Err(Error::invalid(format!(
                "pseudo-image: {spec:?} is not a known specification (expected NAME:ARGS, e.g. gradient:red-blue)"
            )));
        }
        let parsed = ParsedUri::parse(&uri)?;
        let mut query = parsed.query;
        // Fold the gradient-family kinds onto `gradient` + `type`.
        let kind = match parsed.kind.as_str() {
            "radial-gradient" | "radial_gradient" | "radial" => {
                query.insert("type".into(), "radial".into());
                "gradient".to_string()
            }
            "conic-gradient" | "conic_gradient" | "conic" => {
                query.insert("type".into(), "conic".into());
                "gradient".to_string()
            }
            other => other.to_string(),
        };
        if !Self::kinds().contains(&kind.as_str()) {
            return Err(Error::invalid(format!(
                "pseudo-image: {spec:?} does not name a still image (known: {})",
                Self::kinds().join(", ")
            )));
        }
        Ok(Self { kind, query })
    }

    /// Parse and size in one call.
    pub fn parse_sized(spec: &str, width: u32, height: u32) -> Result<Self> {
        Ok(Self::parse(spec)?.with_size(width, height))
    }

    /// The still-image generator kinds a specification can resolve to.
    pub fn kinds() -> &'static [&'static str] {
        &[
            "xc", "gradient", "plasma", "hald", "pattern", "noise", "random", "chart", "fractal",
            "ramp", "grating", "label",
        ]
    }

    /// The generator kind this specification resolved to.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The generator's `generate://` query.
    pub fn query(&self) -> &BTreeMap<String, String> {
        &self.query
    }

    /// Canvas size in pixels.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.query.insert("w".into(), width.to_string());
        self.query.insert("h".into(), height.to_string());
        self
    }

    /// Canvas size from a `WxH` string.
    pub fn with_geometry(self, geometry: &str) -> Result<Self> {
        let bad = || Error::invalid(format!("pseudo-image: size {geometry:?} is not WxH"));
        let (w, h) = geometry.trim().split_once(['x', 'X']).ok_or_else(bad)?;
        Ok(self.with_size(
            w.trim().parse().map_err(|_| bad())?,
            h.trim().parse().map_err(|_| bad())?,
        ))
    }

    /// Seed of the random generators.
    pub fn with_seed(mut self, seed: u32) -> Self {
        self.query.insert("seed".into(), seed.to_string());
        self
    }

    /// Set one option. `key` is either `namespace:option`
    /// (`gradient:angle`) or a bare query key (`stops`); see the module
    /// docs.
    pub fn with_define(mut self, key: &str, value: &str) -> Result<Self> {
        let key = key.trim();
        let (namespace, option) = match key.split_once(':') {
            Some((ns, opt)) => (Some(ns.trim().to_ascii_lowercase()), opt.trim()),
            None => (None, key),
        };
        if option.is_empty() {
            return Err(Error::invalid(format!(
                "pseudo-image: define {key:?} names no option"
            )));
        }
        if let Some(ns) = namespace {
            let Some((_, kinds)) = NAMESPACES.iter().find(|(name, _)| *name == ns) else {
                return Err(Error::invalid(format!(
                    "pseudo-image: unknown define namespace {ns:?} in {key:?}"
                )));
            };
            if !kinds.contains(&self.kind.as_str()) {
                return Ok(self);
            }
        }
        let option = option.to_ascii_lowercase();
        let option = match option.as_str() {
            "bounding-box" | "bounding_box" | "boundingbox" => "bbox",
            "centre" => "center",
            "color-space" | "colourspace" | "interpolate" => "colorspace",
            other => other,
        };
        self.query
            .insert(option.to_string(), value.trim().to_string());
        Ok(self)
    }

    /// Sample depth of the rendered image.
    pub fn with_depth(mut self, depth: SampleDepth) -> Self {
        let name = match depth {
            SampleDepth::U16 => "16",
            SampleDepth::F32 => "float",
            _ => "8",
        };
        self.query.insert("depth".into(), name.into());
        self
    }

    /// Channel layout of the rendered image.
    pub fn with_channels(mut self, channels: Channels) -> Self {
        let name = match channels {
            Channels::Gray => "gray",
            Channels::GrayAlpha => "graya",
            Channels::Rgb => "rgb",
            _ => "rgba",
        };
        self.query.insert("channels".into(), name.into());
        self
    }

    /// Dither of the integer depths.
    pub fn with_dither(mut self, dither: Dither) -> Self {
        let name = match dither {
            Dither::Ordered => "ordered",
            Dither::Noise => "noise",
            _ => "none",
        };
        self.query.insert("dither".into(), name.into());
        self
    }

    /// Worker budget (serial by default; the output does not depend on
    /// it).
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.query
            .insert("threads".into(), threads.clamp(1, 256).to_string());
        self
    }

    /// Depth, layout, dither and thread budget from a
    /// [`RenderConfig`].
    pub fn with_config(self, cfg: &RenderConfig) -> Self {
        let mut me = self
            .with_depth(cfg.depth)
            .with_channels(cfg.channels)
            .with_dither(cfg.dither)
            .with_threads(cfg.threads);
        me.query
            .insert("dither_seed".into(), cfg.dither_seed.to_string());
        me
    }

    /// The canonical `generate://` URI of this image, usable with the
    /// source registry (float depth excepted).
    pub fn to_uri(&self) -> String {
        let mut uri = format!("generate://{}", self.kind);
        for (i, (k, v)) in self.query.iter().enumerate() {
            uri.push(if i == 0 { '?' } else { '&' });
            uri.push_str(k);
            uri.push('=');
            for b in v.bytes() {
                match b {
                    b'A'..=b'Z'
                    | b'a'..=b'z'
                    | b'0'..=b'9'
                    | b'-'
                    | b'_'
                    | b'.'
                    | b'~'
                    | b','
                    | b'/' => uri.push(b as char),
                    _ => uri.push_str(&format!("%{b:02x}")),
                }
            }
        }
        uri
    }

    /// Render the image.
    pub fn render(&self) -> Result<Image> {
        crate::image::render_still(&self.kind, &self.query).unwrap_or_else(|| {
            Err(Error::invalid(format!(
                "pseudo-image: {} is not a still image generator",
                self.kind
            )))
        })
    }
}

/// Parse `spec`, size it `width × height` and render it as 8-bit RGBA.
pub fn render(spec: &str, width: u32, height: u32) -> Result<Image> {
    PseudoImage::parse_sized(spec, width, height)?.render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::gradient::{
        GradientDirection, GradientExtent, GradientKind, GradientOptions, GradientStop,
    };
    use crate::image::{canvas, gradient};

    fn rgba(spec: &str, w: u32, h: u32) -> Vec<u8> {
        render(spec, w, h).unwrap().as_u8().unwrap().to_vec()
    }

    #[test]
    fn solid_colours() {
        assert!(
            rgba("xc:red", 3, 2)
                .chunks(4)
                .all(|p| p == [255, 0, 0, 255])
        );
        assert!(
            rgba("canvas:#33669980", 3, 2)
                .chunks(4)
                .all(|p| p == [0x33, 0x66, 0x99, 0x80])
        );
        assert!(
            rgba("xc:rgb(10,20,30)", 2, 2)
                .chunks(4)
                .all(|p| p == [10, 20, 30, 255])
        );
        assert!(rgba("xc:", 2, 2).chunks(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn gradient_specifications_match_the_option_records() {
        let red = [255, 0, 0, 255];
        let blue = [0, 0, 255, 255];
        let base = || GradientOptions::new(20, 12).with_colors(red, blue);
        assert_eq!(
            rgba("gradient:red-blue", 20, 12),
            gradient::render_linear(&base()).unwrap().pixels
        );
        assert_eq!(
            rgba("gradient:", 20, 12),
            gradient::render_linear(&GradientOptions::new(20, 12))
                .unwrap()
                .pixels
        );
        let img = PseudoImage::parse_sized("gradient:red-blue", 20, 12)
            .unwrap()
            .with_define("gradient:angle", "45")
            .unwrap()
            .render()
            .unwrap();
        assert_eq!(
            img.as_u8().unwrap(),
            &gradient::render_linear(&base().with_angle(45.0))
                .unwrap()
                .pixels[..]
        );
        let img = PseudoImage::parse_sized("gradient:red-blue", 20, 12)
            .unwrap()
            .with_define("gradient:vector", "2,3,15,9")
            .unwrap()
            .with_define("gradient:bounding-box", "10x8+2+2")
            .unwrap()
            .with_define("colorspace", "RGB")
            .unwrap()
            .render()
            .unwrap();
        let want = base()
            .with_vector(2.0, 3.0, 15.0, 9.0)
            .with_bounding_box(gradient::BoundingBox::new(10, 8, 2, 2))
            .with_colorspace(gradient::GradientColorspace::Linear);
        assert_eq!(
            img.as_u8().unwrap(),
            &gradient::render_linear(&want).unwrap().pixels[..]
        );
        let img = PseudoImage::parse_sized("gradient:red-blue", 20, 12)
            .unwrap()
            .with_define("gradient:direction", "NorthWest")
            .unwrap()
            .render()
            .unwrap();
        assert_eq!(
            img.as_u8().unwrap(),
            &gradient::render_linear(&base().with_direction(GradientDirection::NorthWest))
                .unwrap()
                .pixels[..]
        );
    }

    #[test]
    fn radial_and_conic_specifications() {
        let img = PseudoImage::parse_sized("radial-gradient:white-black", 21, 15)
            .unwrap()
            .with_define("gradient:extent", "Ellipse")
            .unwrap()
            .with_define("gradient:center", "8,6")
            .unwrap()
            .render()
            .unwrap();
        let want = GradientOptions::new(21, 15)
            .with_extent(GradientExtent::Ellipse)
            .with_center(8.0, 6.0);
        assert_eq!(
            img.as_u8().unwrap(),
            &gradient::render_radial(&want).unwrap().pixels[..]
        );
        for extent in [
            "Circle",
            "Diagonal",
            "Maximum",
            "Minimum",
            "closest-side",
            "farthest-corner",
        ] {
            let img = PseudoImage::parse_sized("radial-gradient:", 9, 7)
                .unwrap()
                .with_define("gradient:extent", extent)
                .unwrap()
                .with_define("gradient:radii", "3,2")
                .unwrap()
                .render()
                .unwrap();
            // Explicit radii win over the extent: centre white, and
            // black three pixels out along x.
            assert_eq!(img.pixel(4, 3), [1.0, 1.0, 1.0, 1.0], "{extent}");
            assert_eq!(img.pixel(7, 3), [0.0, 0.0, 0.0, 1.0], "{extent}");
        }
        let conic = rgba("conic-gradient:red-lime-blue-red", 9, 9);
        let stops = GradientStop::parse_list("red,lime,blue,red").unwrap();
        let want = GradientOptions::new(9, 9)
            .with_stops(stops)
            .field(GradientKind::Conic)
            .unwrap();
        assert_eq!(
            conic,
            canvas::render(&want, &RenderConfig::new()).as_u8().unwrap()
        );
    }

    #[test]
    fn multi_colour_gradient_is_an_even_stop_list() {
        let img = rgba("gradient:red-white-blue,direction=East", 5, 1);
        assert_eq!(img[..4], [255, 0, 0, 255]);
        assert_eq!(img[8..12], [255, 255, 255, 255]);
        assert_eq!(img[16..], [0, 0, 255, 255]);
        let img = rgba(
            "gradient:rgb(0,0,0)-rgb(255,255,255)-rgb(0,0,0),direction=East",
            3,
            1,
        );
        assert_eq!(img[4..8], [255, 255, 255, 255]);
    }

    #[test]
    fn plasma_is_reproducible_from_its_seed() {
        for spec in [
            "plasma:",
            "plasma:red-blue",
            "plasma:fractal",
            "plasma:tomato",
        ] {
            let draw = |seed: u32| {
                PseudoImage::parse_sized(spec, 33, 17)
                    .unwrap()
                    .with_seed(seed)
                    .render()
                    .unwrap()
            };
            assert_eq!(draw(11), draw(11), "{spec}");
            assert_ne!(draw(11), draw(12), "{spec}");
        }
        // Endpoint colours steer the cloud: red at the top, blue at
        // the bottom, on average.
        let img = PseudoImage::parse_sized("plasma:red-blue", 64, 64)
            .unwrap()
            .with_seed(3)
            .render()
            .unwrap();
        let mean = |rows: std::ops::Range<u32>, c: usize| {
            let mut sum = 0.0;
            for y in rows.clone() {
                for x in 0..64 {
                    sum += img.pixel(x, y)[c];
                }
            }
            sum / (f64::from(rows.end - rows.start) * 64.0)
        };
        assert!(mean(0..8, 0) > mean(56..64, 0) + 0.4);
        assert!(mean(56..64, 2) > mean(0..8, 2) + 0.4);
    }

    #[test]
    fn plasma_over_a_stop_list() {
        let spec = PseudoImage::parse_sized("plasma:red-lime-blue", 48, 48)
            .unwrap()
            .with_seed(2);
        assert_eq!(spec.kind(), "plasma");
        assert_eq!(spec.query()["stops"], "red,lime,blue");
        let img = spec.render().unwrap();
        let mean = |rows: std::ops::Range<u32>, c: usize| {
            let mut sum = 0.0;
            for y in rows.clone() {
                for x in 0..48 {
                    sum += img.pixel(x, y)[c];
                }
            }
            sum / (f64::from(rows.end - rows.start) * 48.0)
        };
        // Red at the top, green in the middle, blue at the bottom.
        assert!(mean(0..6, 0) > 0.6 && mean(0..6, 2) < 0.3);
        assert!(mean(21..27, 1) > 0.6);
        assert!(mean(42..48, 2) > 0.6 && mean(42..48, 0) < 0.3);
    }

    #[test]
    fn hald_patterns_noise_and_charts_resolve() {
        let hald = render("hald:3", 1, 1).unwrap();
        assert_eq!((hald.width, hald.height), (27, 27));
        assert_eq!(hald.as_u8().unwrap()[..4], [0, 0, 0, 255]);
        let p = PseudoImage::parse("pattern:CHECKERBOARD").unwrap();
        assert_eq!(p.kind(), "pattern");
        let checker = rgba("pattern:checkerboard,size=1", 2, 2);
        assert_eq!(checker[..8], [0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(PseudoImage::parse("noise:perlin").unwrap().kind(), "noise");
        assert_eq!(
            PseudoImage::parse("noise:gaussian").unwrap().kind(),
            "random"
        );
        assert_eq!(
            PseudoImage::parse("random:poisson").unwrap().kind(),
            "random"
        );
        assert_eq!(PseudoImage::parse("noise:").unwrap().kind(), "noise");
        let bars = rgba("chart:bars", 8, 1);
        assert_eq!(bars[..4], [191, 191, 191, 255]);
        assert_eq!(bars[28..], [0, 0, 0, 255]);
        for kind in [
            "random",
            "uniform",
            "gaussian",
            "impulse",
            "laplacian",
            "poisson",
            "multiplicative",
        ] {
            let a = PseudoImage::parse_sized(&format!("random:{kind}"), 16, 16)
                .unwrap()
                .with_seed(5);
            assert_eq!(a.render().unwrap(), a.render().unwrap(), "{kind}");
        }
    }

    #[test]
    fn output_settings_and_uri_round_trip() {
        let p = PseudoImage::parse_sized("gradient:black-white", 3, 1)
            .unwrap()
            .with_define("gradient:direction", "East")
            .unwrap()
            .with_depth(SampleDepth::U16)
            .with_channels(Channels::Gray);
        assert_eq!(p.render().unwrap().as_u16().unwrap(), &[0, 32768, 65535]);
        let float = p.clone().with_depth(SampleDepth::F32).render().unwrap();
        assert_eq!(float.as_f32().unwrap(), &[0.0, 0.5, 1.0]);
        // The URI names the same image.
        let again = PseudoImage::parse(&p.to_uri()).unwrap();
        assert_eq!(again, p);
        let cfg = RenderConfig::new()
            .with_depth(SampleDepth::U16)
            .with_dither(Dither::Ordered)
            .with_threads(3);
        let a = PseudoImage::parse_sized("plasma:red-blue", 24, 24)
            .unwrap()
            .with_config(&cfg)
            .render()
            .unwrap();
        let b = PseudoImage::parse_sized("plasma:red-blue", 24, 24)
            .unwrap()
            .with_config(&cfg.with_threads(1))
            .render()
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.depth(), SampleDepth::U16);
        assert_eq!(
            PseudoImage::parse("xc:red")
                .unwrap()
                .with_geometry("7x5")
                .unwrap()
                .render()
                .unwrap()
                .height,
            5
        );
    }

    #[test]
    fn defines_for_another_generator_are_ignored_and_bad_input_is_refused() {
        let a = PseudoImage::parse_sized("xc:red", 4, 4)
            .unwrap()
            .with_define("gradient:angle", "45")
            .unwrap();
        assert_eq!(a, PseudoImage::parse_sized("xc:red", 4, 4).unwrap());
        assert!(
            PseudoImage::parse("xc:red")
                .unwrap()
                .with_define("tiff:angle", "1")
                .is_err()
        );
        assert!(
            PseudoImage::parse("xc:red")
                .unwrap()
                .with_define("gradient:", "1")
                .is_err()
        );
        assert!(PseudoImage::parse("photo.png").is_err());
        assert!(PseudoImage::parse("rose:").is_err());
        assert!(PseudoImage::parse("testsrc:").is_err());
        assert!(PseudoImage::parse("synth:1,sine,440").is_err());
        assert!(render("gradient:red-nocolour", 4, 4).is_err());
        assert!(render("pattern:tartan", 4, 4).is_err());
        assert!(
            PseudoImage::parse("xc:red")
                .unwrap()
                .with_geometry("wide")
                .is_err()
        );
        let bad = PseudoImage::parse_sized("gradient:", 4, 4)
            .unwrap()
            .with_define("gradient:extent", "square")
            .unwrap();
        assert!(bad.render().is_err());
    }
}

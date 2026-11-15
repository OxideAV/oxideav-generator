//! The string front-ends must answer every input with an image or an
//! error — never a panic, never an allocation sized by hostile input.

use std::collections::BTreeMap;

use oxideav_generator::image::{MAX_PIXELS, render_still};
use oxideav_generator::pseudo::PseudoImage;

/// Small deterministic generator for the test inputs.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const NAMES: &[&str] = &[
    "xc",
    "canvas",
    "gradient",
    "radial-gradient",
    "conic-gradient",
    "plasma",
    "hald",
    "pattern",
    "noise",
    "random",
    "chart",
    "mandelbrot",
    "julia",
    "ramp",
    "grating",
    "radial",
    "conic",
];

const ARGS: &[&str] = &[
    "",
    "red",
    "red-blue",
    "red-white-blue",
    "#fff-#000",
    "rgba(1,2,3,0.5)-hsl(10,20%,30%)",
    "fractal",
    "3",
    "0",
    "99",
    "checkerboard",
    "bricks",
    "gray50",
    "gray500",
    "perlin",
    "worley",
    "gaussian",
    "poisson",
    "bars",
    "zoneplate",
    "composite",
    "-",
    "--",
    "red-",
    "-blue",
    "(",
    ")",
    "rgb(",
    "nocolour",
    "#12",
    "#0000ffff8000",
    "NaN",
    "1e99",
    "-1",
];

const KEYS: &[&str] = &[
    "angle",
    "vector",
    "direction",
    "extent",
    "bbox",
    "center",
    "radii",
    "colorspace",
    "stops",
    "spread",
    "hue",
    "type",
    "size",
    "line",
    "color",
    "color1",
    "color2",
    "from",
    "to",
    "seed",
    "amount",
    "mono",
    "scale",
    "octaves",
    "lacunarity",
    "gain",
    "k",
    "points",
    "dist",
    "level",
    "steps",
    "labels",
    "depth",
    "channels",
    "dither",
    "threads",
    "roughness",
    "amplitude",
    "bits",
    "iter",
    "zoom",
    "freq",
    "phase",
];

const VALUES: &[&str] = &[
    "",
    "0",
    "1",
    "2",
    "7",
    "45",
    "-30",
    "360.5",
    "1e9",
    "-1e9",
    "NaN",
    "inf",
    "-inf",
    "0.5",
    "1,2,3,4",
    "0,0,0,0",
    "5,5",
    "1e30,1e30",
    "3x3+1+1",
    "999x999+0+0",
    "0x0",
    "North",
    "SouthWest",
    "Circle",
    "Ellipse",
    "farthest-corner",
    "srgb",
    "linear",
    "oklab",
    "oklch",
    "lab",
    "lch",
    "pad",
    "repeat",
    "reflect",
    "longer",
    "red",
    "red,blue",
    "red 50%,blue 50%",
    "red@NaN,blue",
    "red 1e400,blue",
    ",",
    ";;",
    "8",
    "16",
    "float",
    "gray",
    "graya",
    "rgb",
    "rgba",
    "ordered",
    "noise",
    "radial",
    "conic",
    "fractal",
    "palette",
    "euclidean",
    "manhattan",
    "4294967295",
    "4294967296",
    "x",
    "(((",
    "100%",
    "-5%",
];

#[test]
fn random_specifications_never_panic() {
    let mut rng = Lcg(0x5EED_1234_ABCD_0001);
    let mut rendered = 0u32;
    let mut refused = 0u32;
    for _ in 0..6000 {
        let spec = format!("{}:{}", rng.pick(NAMES), rng.pick(ARGS));
        let Ok(mut image) = PseudoImage::parse(&spec) else {
            refused += 1;
            continue;
        };
        image = image.with_size((rng.next() % 12) as u32, (rng.next() % 12) as u32);
        for _ in 0..rng.next() % 4 {
            let key = rng.pick(KEYS);
            let key = if rng.next().is_multiple_of(3) {
                format!("gradient:{key}")
            } else {
                key.to_string()
            };
            image = match image.clone().with_define(&key, rng.pick(VALUES)) {
                Ok(next) => next,
                Err(_) => image,
            };
        }
        match image.render() {
            Ok(img) => {
                rendered += 1;
                let samples = match img.depth() {
                    oxideav_generator::image::canvas::SampleDepth::U16 => {
                        img.as_u16().unwrap().len()
                    }
                    oxideav_generator::image::canvas::SampleDepth::F32 => {
                        img.as_f32().unwrap().len()
                    }
                    _ => img.as_u8().unwrap().len(),
                };
                assert_eq!(
                    samples,
                    img.width as usize * img.height as usize * img.channels.count(),
                    "{spec}"
                );
            }
            Err(_) => refused += 1,
        }
    }
    // The vocabulary is mostly valid: both outcomes must be common.
    assert!(rendered > 1000, "only {rendered} rendered");
    assert!(refused > 300, "only {refused} refused");
}

#[test]
fn random_text_is_refused_or_rendered() {
    let mut rng = Lcg(42);
    let alphabet: Vec<char> = "abcxyzGRADIENTplasma:-,=&%#()@ 0123456789.+~\u{e9}\u{4e2d}"
        .chars()
        .collect();
    for _ in 0..4000 {
        let len = rng.next() % 24;
        let text: String = (0..len)
            .map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize])
            .collect();
        if let Ok(image) = PseudoImage::parse(&text) {
            let _ = image.with_size(5, 4).render();
        }
        let _ = oxideav_generator::shorthand::translate(&text);
    }
}

#[test]
fn oversized_canvases_are_refused_before_allocating() {
    for kind in [
        "xc",
        "gradient",
        "radial-gradient",
        "conic-gradient",
        "pattern",
        "grating",
        "ramp",
        "fractal",
        "plasma",
        "noise",
        "random",
        "chart",
    ] {
        for (w, h) in [
            ("4000000000", "4000000000"),
            ("16385", "16385"),
            ("1", "268435457"),
            ("99999999999999999999", "2"),
        ] {
            let q: BTreeMap<String, String> = [("w", w), ("h", h)]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let out = render_still(kind, &q).expect("a still-image kind");
            assert!(out.is_err(), "{kind} {w}x{h} was not refused");
        }
    }
    assert_eq!(MAX_PIXELS, 16384 * 16384);
    assert!(
        PseudoImage::parse("plasma:")
            .unwrap()
            .with_size(70000, 70000)
            .render()
            .is_err()
    );
}

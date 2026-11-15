//! Megapixels-per-second throughput of the still-image generators.
//!
//! A plain timing harness (no statistics framework): every case renders
//! a 1920×1080 canvas several times and reports the median rate. Run
//! with:
//!
//! ```text
//! cargo bench --bench mpps            # every case
//! cargo bench --bench mpps -- radial  # cases whose name contains "radial"
//! ```

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

const W: u32 = 1920;
const H: u32 = 1080;
const RUNS: usize = 7;

fn query(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut q: BTreeMap<String, String> = items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    q.insert("w".into(), W.to_string());
    q.insert("h".into(), H.to_string());
    q
}

/// Median megapixels per second of `f` over [`RUNS`] calls.
fn rate(mut f: impl FnMut()) -> f64 {
    let mut secs: Vec<f64> = (0..RUNS)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64()
        })
        .collect();
    secs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (W as f64 * H as f64) / 1e6 / secs[RUNS / 2]
}

type Case = (&'static str, Box<dyn FnMut()>);

fn main() {
    let filter: Option<String> = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let mut cases: Vec<Case> = Vec::new();
    use oxideav_generator::image::{gradient, noise, pattern, plasma, xc};

    macro_rules! legacy {
        ($name:literal, $module:ident, $items:expr) => {{
            let q = query($items);
            cases.push((
                $name,
                Box::new(move || {
                    black_box($module::render(black_box(&q)).unwrap());
                }),
            ));
        }};
    }
    legacy!("xc", xc, &[("color", "red")]);
    legacy!("checkerboard", pattern, &[("size", "32")]);
    legacy!("linear south", gradient, &[("from", "red"), ("to", "blue")]);
    legacy!(
        "linear east",
        gradient,
        &[("from", "red"), ("to", "blue"), ("direction", "East")]
    );
    legacy!(
        "linear angle=30",
        gradient,
        &[("from", "red"), ("to", "blue"), ("angle", "30")]
    );
    legacy!(
        "linear angle=30 linear-light",
        gradient,
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("colorspace", "linear")
        ]
    );
    legacy!(
        "radial",
        gradient,
        &[("from", "red"), ("to", "blue"), ("type", "radial")]
    );
    legacy!(
        "radial linear-light",
        gradient,
        &[
            ("from", "red"),
            ("to", "blue"),
            ("type", "radial"),
            ("colorspace", "linear")
        ]
    );
    legacy!("plasma palette", plasma, &[("seed", "42")]);
    legacy!(
        "plasma gradient",
        plasma,
        &[("from", "red"), ("to", "blue"), ("seed", "42")]
    );
    legacy!("plasma fractal", plasma, &[("type", "fractal")]);
    legacy!("noise perlin", noise, &[("type", "perlin")]);
    legacy!("noise simplex", noise, &[("type", "simplex")]);
    legacy!("noise value", noise, &[("type", "value")]);
    legacy!("noise worley", noise, &[("type", "worley")]);

    // Generators and output formats without an 8-bit RGBA `render`.
    macro_rules! still {
        ($name:literal, $kind:literal, $items:expr) => {{
            let q = query($items);
            cases.push((
                $name,
                Box::new(move || {
                    black_box(
                        oxideav_generator::image::render_still($kind, black_box(&q))
                            .unwrap()
                            .unwrap(),
                    );
                }),
            ));
        }};
    }
    still!(
        "linear angle=30 16-bit",
        "gradient",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("depth", "16")
        ]
    );
    still!(
        "linear angle=30 float",
        "gradient",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("depth", "float")
        ]
    );
    still!(
        "linear angle=30 dither=noise",
        "gradient",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("dither", "noise")
        ]
    );
    still!(
        "linear angle=30 threads=4",
        "gradient",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("threads", "4")
        ]
    );
    still!(
        "linear 5 stops angle=30",
        "gradient",
        &[("stops", "red,gold,white,teal,navy"), ("angle", "30")]
    );
    still!(
        "linear angle=30 oklab",
        "gradient",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("colorspace", "oklab")
        ]
    );
    still!(
        "linear angle=30 oklch",
        "gradient",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("angle", "30"),
            ("colorspace", "oklch")
        ]
    );
    still!(
        "radial reflect",
        "gradient",
        &[
            ("type", "radial"),
            ("radii", "200,120"),
            ("spread", "reflect")
        ]
    );
    still!(
        "conic",
        "gradient",
        &[("type", "conic"), ("stops", "red,lime,blue,red")]
    );
    still!(
        "conic threads=4",
        "gradient",
        &[
            ("type", "conic"),
            ("stops", "red,lime,blue,red"),
            ("threads", "4")
        ]
    );
    still!("pattern bricks", "pattern", &[("type", "bricks")]);
    still!("pattern dots", "pattern", &[("type", "dots")]);
    still!("chart composite", "chart", &[("labels", "0")]);
    still!("chart zoneplate", "chart", &[("type", "zoneplate")]);
    still!("random uniform-range", "random", &[("type", "random")]);
    still!("random gaussian", "random", &[("type", "gaussian")]);
    still!(
        "random gaussian threads=4",
        "random",
        &[("type", "gaussian"), ("threads", "4")]
    );
    still!("random poisson", "random", &[("type", "poisson")]);
    still!(
        "noise perlin ramp 16-bit",
        "noise",
        &[
            ("type", "perlin"),
            ("from", "black"),
            ("to", "white"),
            ("depth", "16")
        ]
    );
    still!(
        "noise perlin threads=4",
        "noise",
        &[("type", "perlin"), ("threads", "4")]
    );
    still!(
        "plasma gradient 16-bit",
        "plasma",
        &[
            ("from", "red"),
            ("to", "blue"),
            ("seed", "42"),
            ("depth", "16")
        ]
    );

    // `-- stages`: where the time of a render goes — the geometry
    // (gradient parameter / noise scalar), the colour, and the packing
    // into samples.
    if filter.as_deref() == Some("stages") {
        stages();
        return;
    }

    println!("{W}x{H}, median of {RUNS} runs");
    for (name, mut f) in cases {
        if filter.as_deref().is_some_and(|p| !name.contains(p)) {
            continue;
        }
        println!("{name:<34} {:>9.1} MP/s", rate(&mut f));
    }
}

/// Stage breakdown of representative renders, in ns per pixel.
fn stages() {
    use oxideav_generator::image::canvas::{self, Field, RenderConfig, SampleDepth};
    use oxideav_generator::image::gradient::{GradientKind, GradientOptions};
    use oxideav_generator::image::noise::NoiseField;
    use oxideav_generator::image::random::RandomOptions;

    let ns = |mut f: Box<dyn FnMut()>| 1e3 / rate(&mut *f);
    let report = |name: &str, parts: &[(&str, f64)]| {
        let line: Vec<String> = parts.iter().map(|(k, v)| format!("{k} {v:.1}")).collect();
        println!("{name:<28} ns/pixel: {}", line.join(" | "));
    };

    for (name, items, kind) in [
        (
            "linear angle=30",
            vec![("from", "red"), ("to", "blue"), ("angle", "30")],
            GradientKind::Linear,
        ),
        (
            "linear angle=30 linear-light",
            vec![
                ("from", "red"),
                ("to", "blue"),
                ("angle", "30"),
                ("colorspace", "linear"),
            ],
            GradientKind::Linear,
        ),
        (
            "linear angle=30 oklab",
            vec![
                ("from", "red"),
                ("to", "blue"),
                ("angle", "30"),
                ("colorspace", "oklab"),
            ],
            GradientKind::Linear,
        ),
        (
            "radial",
            vec![("from", "red"), ("to", "blue")],
            GradientKind::Radial,
        ),
        (
            "conic",
            vec![("stops", "red,lime,blue,red")],
            GradientKind::Conic,
        ),
    ] {
        let opts = GradientOptions::from_query(&query(&items)).unwrap();
        let (f1, f2, f3, f4) = (
            opts.field(kind).unwrap(),
            opts.field(kind).unwrap(),
            opts.field(kind).unwrap(),
            opts.field(kind).unwrap(),
        );
        let geometry = ns(Box::new(move || {
            let mut acc = 0.0;
            for y in 0..H {
                for x in 0..W {
                    acc += f1.parameter(x, y);
                }
            }
            black_box(acc);
        }));
        let colour = ns(Box::new(move || {
            let mut acc = 0.0;
            for y in 0..H {
                for x in 0..W {
                    acc += f2.color_at(f2.parameter(x, y))[0];
                }
            }
            black_box(acc);
        }));
        let float = ns(Box::new(move || {
            black_box(canvas::render(
                &f3,
                &RenderConfig::new().with_depth(SampleDepth::F32),
            ));
        }));
        let eight = ns(Box::new(move || {
            black_box(canvas::render(&f4, &RenderConfig::new()));
        }));
        report(
            name,
            &[
                ("geometry", geometry),
                ("+colour", colour),
                ("render f32", float),
                ("render u8", eight),
            ],
        );
    }

    let nq = query(&[("type", "perlin")]);
    let (n1, n2) = (
        NoiseField::from_query(&nq).unwrap(),
        NoiseField::from_query(&nq).unwrap(),
    );
    let scalar = ns(Box::new(move || {
        let mut acc = 0.0;
        for y in 0..H {
            for x in 0..W {
                acc += n1.scalar(x, y);
            }
        }
        black_box(acc);
    }));
    let full = ns(Box::new(move || {
        black_box(canvas::render(&n2, &RenderConfig::new()));
    }));
    report("noise perlin", &[("scalar", scalar), ("render u8", full)]);

    let r = RandomOptions::new(W, H).field();
    let r2 = r.clone();
    let fill = ns(Box::new(move || {
        let mut row = vec![[0f64; 4]; W as usize];
        for y in 0..H {
            r2.fill_row(y, 0, &mut row);
            black_box(&row);
        }
    }));
    let full = ns(Box::new(move || {
        black_box(canvas::render(&r, &RenderConfig::new()));
    }));
    report("random gaussian", &[("samples", fill), ("render u8", full)]);
}

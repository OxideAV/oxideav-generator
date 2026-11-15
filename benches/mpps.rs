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

    println!("{W}x{H}, median of {RUNS} runs");
    for (name, mut f) in cases {
        if filter.as_deref().is_some_and(|p| !name.contains(p)) {
            continue;
        }
        println!("{name:<34} {:>9.1} MP/s", rate(&mut f));
    }
}

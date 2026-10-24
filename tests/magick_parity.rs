//! Black-box parity against the `magick` binary (ImageMagick 7) for the
//! `gradient:` / `radial-gradient:` / `plasma:` pseudo-images.
//!
//! The harness runs only when `magick` is on `PATH`; otherwise every
//! test prints a note and passes (the in-tree unit tests carry the
//! transcribed probe tables, so CI without ImageMagick still pins the
//! numbers).
//!
//! Gradients: every pixel within one 8-bit level per channel, with one
//! documented exception — ImageMagick 7.1.2 writes the pixel at the
//! gradient vector's start point with the value of its left-hand
//! neighbour. A case may therefore show at most one out-of-tolerance
//! pixel, and that pixel's `magick` value must equal `magick`'s own
//! value one pixel to the left (which is how the artefact is told
//! apart from a real mismatch).
//!
//! Plasma: seeded random on both sides with different generators, so
//! the comparison is statistical — base-gradient band means, channel
//! standard deviations, and neighbour differences.

use std::collections::BTreeMap;
use std::process::Command;

use oxideav_generator::image::{Rgba8Image, gradient, plasma};

fn magick_available() -> bool {
    match Command::new("magick").arg("-version").output() {
        Ok(o) if o.status.success() => true,
        _ => {
            eprintln!("magick_parity: `magick` not on PATH — skipping black-box comparison");
            false
        }
    }
}

/// `magick -size WxH <args…> -depth 8 rgba:-` → RGBA8 image.
fn magick_rgba(w: u32, h: u32, args: &[&str]) -> Rgba8Image {
    let out = Command::new("magick")
        .arg("-size")
        .arg(format!("{w}x{h}"))
        .args(args)
        .args(["-depth", "8", "rgba:-"])
        .output()
        .expect("spawn magick");
    assert!(
        out.status.success(),
        "magick {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout.len(),
        (w * h * 4) as usize,
        "magick {args:?}: unexpected byte count"
    );
    Rgba8Image {
        width: w,
        height: h,
        pixels: out.stdout,
    }
}

fn query(w: u32, h: u32, items: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut q: BTreeMap<String, String> = items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    q.insert("w".into(), w.to_string());
    q.insert("h".into(), h.to_string());
    q
}

/// Compare with the ±1 rule and the single start-pixel exception.
fn assert_gradient_parity(ours: &Rgba8Image, theirs: &Rgba8Image, label: &str) {
    assert_eq!((ours.width, ours.height), (theirs.width, theirs.height));
    let mut outliers = Vec::new();
    for y in 0..ours.height {
        for x in 0..ours.width {
            let a = ours.get(x, y);
            let b = theirs.get(x, y);
            let within = (0..4).all(|i| (a[i] as i16 - b[i] as i16).abs() <= 1);
            if !within {
                outliers.push((x, y, a, b));
            }
        }
    }
    if outliers.is_empty() {
        return;
    }
    assert!(
        outliers.len() == 1,
        "{label}: {} pixels differ by more than one level: {:?}",
        outliers.len(),
        &outliers[..outliers.len().min(8)]
    );
    let (x, y, a, b) = outliers[0];
    assert!(
        x > 0 && theirs.get(x - 1, y) == b,
        "{label}: pixel ({x},{y}) ours {a:?} vs magick {b:?} is not the start-pixel artefact"
    );
    eprintln!("{label}: magick start-pixel artefact at ({x},{y}) — ours {a:?}, magick {b:?}");
}

/// One gradient case: `defines` are `(key, value)` pairs that become
/// both `-define gradient:key=value` and our query keys.
fn check_gradient(radial: bool, w: u32, h: u32, colors: &str, defines: &[(&str, &str)]) {
    let mut args: Vec<String> = Vec::new();
    for (k, v) in defines {
        args.push("-define".into());
        args.push(format!("gradient:{k}={v}"));
    }
    args.push(format!(
        "{}:{colors}",
        if radial {
            "radial-gradient"
        } else {
            "gradient"
        }
    ));
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let theirs = magick_rgba(w, h, &arg_refs);

    let (from, to) = match colors.rsplit_once('-') {
        // Colour functions never contain '-', so the last one splits
        // the two stops.
        Some((a, b)) if !colors.is_empty() => (a, b),
        _ => ("white", "black"),
    };
    let mut items: Vec<(&str, &str)> = defines
        .iter()
        .map(|(k, v)| {
            (
                match *k {
                    "bounding-box" => "bbox",
                    other => other,
                },
                *v,
            )
        })
        .collect();
    if !colors.is_empty() {
        items.push(("from", from));
        items.push(("to", to));
    }
    if radial {
        items.push(("type", "radial"));
    }
    let ours = gradient::render(&query(w, h, &items)).expect("render");
    assert_gradient_parity(
        &ours,
        &theirs,
        &format!(
            "{} {w}x{h} {colors:?} {defines:?}",
            if radial { "radial" } else { "linear" }
        ),
    );
}

const DIRECTIONS: [&str; 8] = [
    "North",
    "South",
    "East",
    "West",
    "NorthEast",
    "NorthWest",
    "SouthEast",
    "SouthWest",
];
const EXTENTS: [&str; 5] = ["Circle", "Diagonal", "Ellipse", "Maximum", "Minimum"];
const SIZES: [(u32, u32); 7] = [(5, 3), (7, 5), (4, 4), (1, 5), (5, 1), (16, 9), (33, 17)];

#[test]
fn linear_defaults_and_colours() {
    if !magick_available() {
        return;
    }
    for &(w, h) in &SIZES {
        for colors in [
            "",
            "black-white",
            "red-blue",
            "#102030-#f0e0d0",
            "gray(100)-gray(201)",
            "rgba(255,0,0,0.5)-blue",
            "srgb(255,0,0)-#0000ff80",
            "hsl(120,100%,50%)-navy",
        ] {
            check_gradient(false, w, h, colors, &[]);
        }
    }
}

#[test]
fn linear_directions() {
    if !magick_available() {
        return;
    }
    for &(w, h) in &SIZES {
        for d in DIRECTIONS {
            check_gradient(false, w, h, "black-white", &[("direction", d)]);
        }
    }
    check_gradient(false, 16, 9, "red-blue", &[("direction", "NorthWest")]);
}

#[test]
fn linear_angles() {
    if !magick_available() {
        return;
    }
    for &(w, h) in &SIZES {
        for a in [
            "0", "30", "45", "90", "135", "180", "225", "270", "315", "10.5", "200", "-45", "400",
        ] {
            check_gradient(false, w, h, "black-white", &[("angle", a)]);
        }
    }
    check_gradient(false, 33, 17, "red-blue", &[("angle", "60")]);
}

#[test]
fn linear_vectors() {
    if !magick_available() {
        return;
    }
    for v in [
        "0,0,4,0", "1,0,3,0", "3,0,1,0", "4,0,0,0", "0,0,4,4", "0,0,4,2", "1,1,3,3", "4,2,0,0",
        "0,2,4,0", "2,0,2,0", "2,1,2,1",
    ] {
        check_gradient(false, 5, 5, "black-white", &[("vector", v)]);
        check_gradient(false, 5, 3, "red-blue", &[("vector", v)]);
    }
    check_gradient(false, 33, 17, "black-white", &[("vector", "3,2,30,14")]);
    check_gradient(false, 33, 17, "black-white", &[("vector", "30,14,3,2")]);
    check_gradient(
        false,
        16,
        9,
        "black-white",
        &[("vector", "2.5,1.5,12.25,7")],
    );
}

#[test]
fn linear_extent_defaults_to_south_east_and_is_otherwise_ignored() {
    if !magick_available() {
        return;
    }
    for &(w, h) in &SIZES {
        for e in EXTENTS {
            check_gradient(false, w, h, "black-white", &[("extent", e)]);
            check_gradient(
                false,
                w,
                h,
                "black-white",
                &[("extent", e), ("direction", "East")],
            );
            check_gradient(
                false,
                w,
                h,
                "black-white",
                &[("extent", e), ("angle", "45")],
            );
        }
    }
}

#[test]
fn linear_bounding_box() {
    if !magick_available() {
        return;
    }
    check_gradient(false, 5, 5, "red-blue", &[("bounding-box", "3x3+1+1")]);
    check_gradient(false, 5, 5, "red-blue", &[("bounding-box", "3x3")]);
    check_gradient(
        false,
        5,
        5,
        "red-blue",
        &[("bounding-box", "3x3+1+1"), ("direction", "East")],
    );
    check_gradient(
        false,
        5,
        5,
        "black-white",
        &[("bounding-box", "3x3+1+1"), ("angle", "45")],
    );
    check_gradient(
        false,
        16,
        9,
        "black-white",
        &[("bounding-box", "10x4+3+2"), ("vector", "0,0,15,8")],
    );
    check_gradient(
        false,
        1,
        5,
        "rgba(0,0,255,0.5)-red",
        &[("bounding-box", "1x3+0+1")],
    );
}

#[test]
fn radial_defaults_and_extents() {
    if !magick_available() {
        return;
    }
    for &(w, h) in &SIZES {
        check_gradient(true, w, h, "", &[]);
        check_gradient(true, w, h, "black-white", &[]);
        check_gradient(true, w, h, "red-blue", &[]);
        check_gradient(true, w, h, "rgba(255,0,0,0.5)-blue", &[]);
        for e in EXTENTS {
            check_gradient(true, w, h, "black-white", &[("extent", e)]);
        }
    }
}

#[test]
fn radial_center_radii_and_vectors() {
    if !magick_available() {
        return;
    }
    for c in ["1,1", "1.5,1.5", "3,2", "0,0", "6,4", "10,10", "2.25,0.5"] {
        check_gradient(true, 7, 5, "black-white", &[("center", c)]);
        check_gradient(true, 16, 9, "red-blue", &[("center", c)]);
    }
    for r in ["1", "2", "3", "4", "2,2", "3,1", "1,1", "0,0", "2.5,1.5"] {
        check_gradient(true, 7, 5, "black-white", &[("radii", r)]);
        for e in EXTENTS {
            check_gradient(true, 7, 5, "black-white", &[("radii", r), ("extent", e)]);
        }
    }
    check_gradient(
        true,
        7,
        5,
        "black-white",
        &[("center", "1,1"), ("radii", "2,2")],
    );
    check_gradient(
        true,
        7,
        5,
        "black-white",
        &[("center", "1,1"), ("extent", "Minimum")],
    );
    for d in DIRECTIONS {
        check_gradient(true, 7, 5, "black-white", &[("direction", d)]);
        check_gradient(true, 16, 9, "black-white", &[("direction", d)]);
    }
    for v in [
        "0,0,6,4", "3,2,6,2", "3,2,6,3", "3,1,6,1", "2,2,5,2", "3,2,3,4", "3,2,0,2", "1,1,5,3",
        "1,1,1,1",
    ] {
        check_gradient(true, 7, 5, "black-white", &[("vector", v)]);
        check_gradient(
            true,
            7,
            5,
            "black-white",
            &[("vector", v), ("extent", "Minimum")],
        );
        check_gradient(
            true,
            7,
            5,
            "black-white",
            &[("vector", v), ("radii", "1,1")],
        );
    }
    check_gradient(
        true,
        7,
        5,
        "black-white",
        &[("center", "1,1"), ("vector", "3,2,6,2")],
    );
    check_gradient(true, 7, 5, "black-white", &[("angle", "45")]);
    check_gradient(true, 5, 5, "red-blue", &[("bounding-box", "3x3+1+1")]);
}

// ───────────────────────── plasma (statistical) ─────────────────────────

fn mean_sd(img: &Rgba8Image, ch: usize) -> (f64, f64) {
    let n = (img.width * img.height) as f64;
    let vals = img.pixels.as_chunks::<4>().0.iter().map(|p| p[ch] as f64);
    let mean = vals.clone().sum::<f64>() / n;
    let var = vals.map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
    (mean, var.sqrt())
}

fn band_mean(img: &Rgba8Image, ch: usize, x0: u32, y0: u32, w: u32, h: u32) -> f64 {
    let mut sum = 0.0;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            sum += img.get(x, y)[ch] as f64;
        }
    }
    sum / (w * h) as f64
}

/// Mean absolute difference between horizontally adjacent pixels.
fn neighbour_diff(img: &Rgba8Image, ch: usize) -> f64 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in 0..img.height {
        for x in 1..img.width {
            sum += (img.get(x, y)[ch] as f64 - img.get(x - 1, y)[ch] as f64).abs();
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn plasma_colour_form_statistics() {
    if !magick_available() {
        return;
    }
    for size in [64u32, 128] {
        for seed in ["1", "2", "3"] {
            // `gray50` is 127 on both sides (ImageMagick's bare `grey`
            // is its own X11 value, not CSS's 128, so avoid it here).
            let theirs = magick_rgba(size, size, &["-seed", seed, "plasma:gray50-gray50"]);
            let ours = plasma::render(&query(
                size,
                size,
                &[("from", "gray50"), ("to", "gray50"), ("seed", seed)],
            ))
            .unwrap();
            for ch in 0..3 {
                let (tm, tsd) = mean_sd(&theirs, ch);
                let (om, osd) = mean_sd(&ours, ch);
                // Flat base (128) plus zero-mean noise on both sides.
                // magick's per-channel mean drifts up to ~27 levels off
                // the base for some seeds (seed 2: R ≈ 154).
                assert!((tm - 127.0).abs() < 35.0, "magick mean {tm}");
                assert!((om - 127.0).abs() < 35.0, "ours mean {om}");
                assert!((15.0..=40.0).contains(&tsd), "magick sd {tsd}");
                assert!((15.0..=40.0).contains(&osd), "ours sd {osd}");
                assert!(
                    (osd - tsd).abs() < 15.0,
                    "{size}px seed {seed} ch{ch}: sd ours {osd:.1} vs magick {tsd:.1}"
                );
                let tnd = neighbour_diff(&theirs, ch);
                let ond = neighbour_diff(&ours, ch);
                assert!((2.0..=20.0).contains(&tnd), "magick neighbour diff {tnd}");
                assert!((2.0..=20.0).contains(&ond), "ours neighbour diff {ond}");
            }
        }
    }
}

#[test]
fn plasma_colour_form_keeps_the_gradient_and_its_defines() {
    if !magick_available() {
        return;
    }
    let band = 8;
    // South (default): red on top, blue at the bottom.
    let theirs = magick_rgba(64, 64, &["-seed", "1", "plasma:red-blue"]);
    let ours = plasma::render(&query(
        64,
        64,
        &[("from", "red"), ("to", "blue"), ("seed", "1")],
    ))
    .unwrap();
    for (img, who) in [(&theirs, "magick"), (&ours, "ours")] {
        let top_r = band_mean(img, 0, 0, 0, 64, band);
        let bot_r = band_mean(img, 0, 0, 64 - band, 64, band);
        let top_b = band_mean(img, 2, 0, 0, 64, band);
        let bot_b = band_mean(img, 2, 0, 64 - band, 64, band);
        assert!(
            top_r > 190.0 && bot_r < 70.0,
            "{who}: red {top_r} → {bot_r}"
        );
        assert!(
            top_b < 70.0 && bot_b > 190.0,
            "{who}: blue {top_b} → {bot_b}"
        );
        assert!(mean_sd(img, 1).0 < 40.0, "{who}: green leaks");
    }
    // `gradient:direction=East` is honoured by both.
    let theirs = magick_rgba(
        64,
        64,
        &[
            "-seed",
            "1",
            "-define",
            "gradient:direction=East",
            "plasma:red-blue",
        ],
    );
    let ours = plasma::render(&query(
        64,
        64,
        &[
            ("from", "red"),
            ("to", "blue"),
            ("direction", "East"),
            ("seed", "1"),
        ],
    ))
    .unwrap();
    for (img, who) in [(&theirs, "magick"), (&ours, "ours")] {
        assert!(band_mean(img, 0, 0, 0, band, 64) > 190.0, "{who}: left red");
        assert!(
            band_mean(img, 0, 64 - band, 0, band, 64) < 70.0,
            "{who}: right red"
        );
        assert!(
            band_mean(img, 2, 64 - band, 0, band, 64) > 190.0,
            "{who}: right blue"
        );
    }
    // Bare `plasma:` is white → black; `plasma:red` is red → black.
    let theirs = magick_rgba(64, 64, &["-seed", "1", "plasma:"]);
    let ours = plasma::render(&query(
        64,
        64,
        &[("from", "white"), ("to", "black"), ("seed", "1")],
    ))
    .unwrap();
    for (img, who) in [(&theirs, "magick"), (&ours, "ours")] {
        assert!(
            band_mean(img, 1, 0, 0, 64, band) > 190.0,
            "{who}: top white"
        );
        assert!(
            band_mean(img, 1, 0, 64 - band, 64, band) < 70.0,
            "{who}: bottom black"
        );
    }
    let theirs = magick_rgba(64, 64, &["-seed", "1", "plasma:red"]);
    let ours = plasma::render(&query(64, 64, &[("from", "red"), ("seed", "1")])).unwrap();
    for (img, who) in [(&theirs, "magick"), (&ours, "ours")] {
        assert!(band_mean(img, 0, 0, 0, 64, band) > 190.0, "{who}: top red");
        assert!(
            band_mean(img, 0, 0, 64 - band, 64, band) < 70.0,
            "{who}: bottom red→black"
        );
        assert!(mean_sd(img, 2).0 < 40.0, "{who}: blue stays low");
    }
}

#[test]
fn plasma_fractal_form_statistics() {
    if !magick_available() {
        return;
    }
    for seed in ["1", "2", "3"] {
        let theirs = magick_rgba(64, 64, &["-seed", seed, "plasma:fractal"]);
        let ours = plasma::render(&query(64, 64, &[("type", "fractal"), ("seed", seed)])).unwrap();
        for (img, who) in [(&theirs, "magick"), (&ours, "ours")] {
            // Coloured (channel means differ) and spread out.
            let means: Vec<f64> = (0..3).map(|c| mean_sd(img, c).0).collect();
            assert!(
                means.iter().any(|m| (m - means[0]).abs() > 5.0),
                "{who} seed {seed}: channel means {means:?} too alike"
            );
            let sd: f64 = (0..3).map(|c| mean_sd(img, c).1).sum::<f64>() / 3.0;
            assert!((20.0..=90.0).contains(&sd), "{who} seed {seed}: sd {sd}");
            let nd = neighbour_diff(img, 0);
            assert!(
                (2.0..=30.0).contains(&nd),
                "{who} seed {seed}: neighbour diff {nd}"
            );
        }
    }
}

#[test]
fn plasma_is_seed_reproducible_on_both_sides() {
    if !magick_available() {
        return;
    }
    let a = magick_rgba(16, 16, &["-seed", "7", "plasma:red-blue"]);
    let b = magick_rgba(16, 16, &["-seed", "7", "plasma:red-blue"]);
    assert_eq!(a.pixels, b.pixels, "magick -seed is reproducible");
    let q = query(16, 16, &[("from", "red"), ("to", "blue"), ("seed", "7")]);
    assert_eq!(
        plasma::render(&q).unwrap().pixels,
        plasma::render(&q).unwrap().pixels
    );
}

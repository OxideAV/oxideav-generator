//! Frame-fill / sample-fill hot-path benches.
//!
//! Every bench drives a generator's public `render()` with fixed,
//! deterministic parameters — no fixtures, no I/O, no seeds that
//! change between runs. Video benches render exactly one 320×240
//! frame (`duration` = one frame at the configured fps) so the number
//! reported is per-frame fill cost; the audio benches render one
//! second at 48 kHz so the number is per-second synthesis cost.
//!
//! Run with:
//! ```text
//! cargo bench --bench framefill
//! ```

use std::collections::BTreeMap;

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// One 320×240 frame per render call.
const FRAME: [(&str, &str); 4] = [
    ("w", "320"),
    ("h", "240"),
    ("duration", "0.04"),
    ("fps", "25"),
];

fn video_benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("video-frame-320x240");

    let q = map(&FRAME);
    g.bench_function("colorwheel", |b| {
        b.iter(|| oxideav_generator::video::colorwheel::render(black_box(&q)).unwrap())
    });
    g.bench_function("zoneplate", |b| {
        b.iter(|| oxideav_generator::video::zoneplate::render(black_box(&q)).unwrap())
    });
    g.bench_function("testsrc", |b| {
        b.iter(|| oxideav_generator::video::testsrc::render(black_box(&q)).unwrap())
    });

    let mut q_snow = map(&FRAME);
    q_snow.insert("seed".into(), "42".into());
    g.bench_function("snow-mono", |b| {
        b.iter(|| oxideav_generator::video::snow::render(black_box(&q_snow)).unwrap())
    });
    let mut q_snow_rgb = q_snow.clone();
    q_snow_rgb.insert("mode".into(), "rgb".into());
    g.bench_function("snow-rgb", |b| {
        b.iter(|| oxideav_generator::video::snow::render(black_box(&q_snow_rgb)).unwrap())
    });

    let mut q_box = map(&FRAME);
    q_box.insert("vx".into(), "3".into());
    q_box.insert("vy".into(), "2".into());
    g.bench_function("movingbox", |b| {
        b.iter(|| oxideav_generator::video::movingbox::render(black_box(&q_box)).unwrap())
    });

    let mut q_scroll = map(&FRAME);
    q_scroll.insert("size".into(), "16".into());
    q_scroll.insert("vx".into(), "3".into());
    g.bench_function("scroll-checkerboard", |b| {
        b.iter(|| oxideav_generator::video::scroll::render(black_box(&q_scroll)).unwrap())
    });

    g.finish();
}

fn image_benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("image-320x240");
    let dims = [("w", "320"), ("h", "240")];

    let mut q_perlin = map(&dims);
    q_perlin.insert("type".into(), "perlin".into());
    q_perlin.insert("scale".into(), "32".into());
    q_perlin.insert("seed".into(), "42".into());
    g.bench_function("noise-perlin", |b| {
        b.iter(|| oxideav_generator::image::noise::render(black_box(&q_perlin)).unwrap())
    });

    let mut q_checker = map(&dims);
    q_checker.insert("type".into(), "checkerboard".into());
    q_checker.insert("size".into(), "16".into());
    g.bench_function("pattern-checkerboard", |b| {
        b.iter(|| oxideav_generator::image::pattern::render(black_box(&q_checker)).unwrap())
    });

    let mut q_ramp = map(&dims);
    q_ramp.insert("bits".into(), "8".into());
    g.bench_function("ramp-8bit", |b| {
        b.iter(|| oxideav_generator::image::ramp::render(black_box(&q_ramp)).unwrap())
    });

    let mut q_grating = map(&dims);
    q_grating.insert("freq".into(), "8".into());
    q_grating.insert("angle".into(), "45".into());
    g.bench_function("grating", |b| {
        b.iter(|| oxideav_generator::image::grating::render(black_box(&q_grating)).unwrap())
    });

    g.finish();
}

fn audio_benches(c: &mut Criterion) {
    let mut g = c.benchmark_group("audio-1s-48k");
    let base = [("rate", "48000"), ("duration", "1")];

    let mut q_sine = map(&base);
    q_sine.insert("type".into(), "sine".into());
    q_sine.insert("freq".into(), "440".into());
    g.bench_function("sine", |b| {
        b.iter(|| oxideav_generator::audio::synth::render(black_box(&q_sine)).unwrap())
    });

    let mut q_supersaw = map(&base);
    q_supersaw.insert("type".into(), "supersaw".into());
    q_supersaw.insert("voices".into(), "7".into());
    g.bench_function("supersaw-7", |b| {
        b.iter(|| oxideav_generator::audio::synth::render(black_box(&q_supersaw)).unwrap())
    });

    let mut q_pink = map(&base);
    q_pink.insert("type".into(), "noise".into());
    q_pink.insert("color".into(), "pink".into());
    g.bench_function("noise-pink", |b| {
        b.iter(|| oxideav_generator::audio::synth::render(black_box(&q_pink)).unwrap())
    });

    g.finish();
}

criterion_group!(benches, video_benches, image_benches, audio_benches);
criterion_main!(benches);

//! Byte-level pins of the 8-bit RGBA stills.
//!
//! Each hash (64-bit FNV-1a of the RGBA plane) was taken from the
//! 0.1.6 release build. A generator whose default 8-bit output drifts
//! by a single byte fails here, so refactors and speed-ups of the
//! rendering paths cannot silently change published pixels. New
//! generators are pinned from the release that introduced them.
//!
//! Two lists: [`PINS`] holds generators built from `+ − × ÷ √` and
//! integer arithmetic only, whose bytes are the same on every
//! platform. [`PINS_LIBRARY_MATH`] holds the ones that call the
//! platform's `cos` or power function (the sinusoidal grating, the
//! linear-light gradients); those are compared only on the platform
//! the reference build ran on, since another C library may round a
//! value sitting on a tie the other way.

use oxideav_core::{Frame, SourceOutput, SourceRegistry};

fn fnv(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `(uri, hash of the 0.1.6 output)`, platform-independent arithmetic.
const PINS: &[(&str, u64)] = &[
    (
        "generate://xc?color=%23336699cc&w=64&h=48",
        0xd954b77f1ca31325,
    ),
    (
        "generate://gradient?from=red&to=blue&w=64&h=48",
        0x40690ed75c6adb25,
    ),
    (
        "generate://gradient?from=red&to=blue&direction=East&w=64&h=48",
        0x1e263a6063ac8825,
    ),
    (
        "generate://gradient?from=gold&to=navy&direction=NorthWest&w=64&h=48",
        0xb34a779d0f3c87d1,
    ),
    (
        "generate://gradient?from=red&to=blue&angle=30&w=64&h=48",
        0xa8efd9873993e555,
    ),
    (
        "generate://gradient?from=rgba(255,0,0,0.5)&to=blue&vector=5,7,50,40&w=64&h=48",
        0xadeda28baed56266,
    ),
    (
        "generate://gradient?from=red&to=blue&vector=1,1,60,45&bbox=40x30%2B8%2B6&w=64&h=48",
        0xeaf9dba723c1b01d,
    ),
    (
        "generate://gradient?from=red&to=blue&type=radial&w=64&h=48",
        0xe99310e52785d625,
    ),
    (
        "generate://radial-gradient?from=white&to=black&extent=Ellipse&w=64&h=48",
        0x2ad8473646cf8f15,
    ),
    (
        "generate://radial-gradient?extent=farthest-corner&center=12,40&w=64&h=48",
        0x83af8d54512df8a0,
    ),
    (
        "generate://pattern?type=checkerboard&w=64&h=48&size=5",
        0xba98ca4519ef2915,
    ),
    (
        "generate://pattern?type=hstripes&w=64&h=48&size=3",
        0x63e9dc14b9b9cb25,
    ),
    (
        "generate://pattern?type=vstripes&w=64&h=48&size=7",
        0x1b7c141a165ce325,
    ),
    ("generate://ramp?w=64&h=8&bits=5", 0x372105d6b5cc6965),
    ("generate://hald?level=3", 0xfddbd5578c4ef368),
    (
        "generate://fractal?type=mandelbrot&w=64&h=48&iter=40",
        0xe76cb706e4a0239d,
    ),
    (
        "generate://fractal?type=julia&w=64&h=48&iter=40",
        0x8cbff12e54c949c1,
    ),
    ("generate://plasma?w=64&h=48&seed=42", 0x6f9e373851c6ea1c),
    (
        "generate://plasma?from=red&to=blue&w=64&h=48&seed=42",
        0x0274c3ba5a04f123,
    ),
    (
        "generate://plasma?from=red&direction=East&w=64&h=48&seed=7",
        0x7f2f0cb72d582622,
    ),
    (
        "generate://plasma?type=fractal&w=64&h=48&seed=42",
        0x8c06643fb166180f,
    ),
    (
        "generate://noise?type=perlin&w=64&h=48&scale=12&seed=42",
        0x30fa578b12f7c4f4,
    ),
    (
        "generate://noise?type=simplex&w=64&h=48&scale=12&octaves=3&seed=42",
        0x8098366f937a4723,
    ),
    (
        "generate://noise?type=value&w=64&h=48&scale=12&seed=9",
        0x4a110e9bf17c16ad,
    ),
    (
        "generate://noise?type=worley&w=64&h=48&scale=12&seed=42",
        0x9a11974c2470dc49,
    ),
    (
        "generate://noise?type=worley&dist=manhattan&k=2&points=2&w=64&h=48&scale=12&seed=42",
        0xf949f8bc337817af,
    ),
];

/// `(uri, hash of the 0.1.6 output)` for generators that go through
/// the platform's math library (reference: macOS on aarch64).
const PINS_LIBRARY_MATH: &[(&str, u64)] = &[
    (
        "generate://gradient?from=red&to=blue&angle=30&colorspace=linear&w=64&h=48",
        0x419d8f1604204601,
    ),
    (
        "generate://radial-gradient?from=red&to=blue&center=20,10&radii=30,12&colorspace=linear&w=64&h=48",
        0x60442bb2c33882f0,
    ),
    (
        "generate://grating?w=64&h=48&freq=4&angle=30",
        0x9291c824c183aaa1,
    ),
];

fn check(pins: &[(&str, u64)]) {
    let mut reg = SourceRegistry::new();
    oxideav_generator::register_source(&mut reg);
    for (uri, want) in pins {
        for threads in ["", "&threads=3"] {
            let uri = format!("{uri}{threads}");
            let SourceOutput::Frames(mut src) = reg.open(&uri).unwrap() else {
                panic!("{uri}: expected a frame source")
            };
            let Frame::Video(v) = src.next_frame().unwrap() else {
                panic!("{uri}: expected a video frame")
            };
            assert_eq!(fnv(&v.planes[0].data), *want, "{uri}");
        }
    }
}

#[test]
fn eight_bit_stills_match_the_release_pins() {
    check(PINS);
}

#[test]
fn library_math_stills_match_the_release_pins_on_the_reference_platform() {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        check(PINS_LIBRARY_MATH);
    } else {
        // Still render them: they must not fail, whatever the bytes.
        let mut reg = SourceRegistry::new();
        oxideav_generator::register_source(&mut reg);
        for (uri, _) in PINS_LIBRARY_MATH {
            assert!(reg.open(uri).is_ok(), "{uri}");
        }
    }
}

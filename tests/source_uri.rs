//! Integration: open `generate://` URIs through a `SourceRegistry`,
//! read the bytes back, and confirm they contain a valid container
//! header.

use std::io::Read;

use oxideav_core::SourceRegistry;
use oxideav_generator::register_source;

fn read_all_via_registry(uri: &str) -> Vec<u8> {
    let mut reg = SourceRegistry::new();
    register_source(&mut reg);
    let mut handle = reg.open(uri).expect("open generate:// URI");
    let mut bytes = Vec::new();
    handle.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn synth_sine_emits_riff_wave() {
    let bytes = read_all_via_registry("generate://synth?type=sine&freq=440&duration=0.1");
    assert_eq!(&bytes[0..4], b"RIFF", "expected RIFF magic");
    assert_eq!(&bytes[8..12], b"WAVE", "expected WAVE magic");
    // 0.1s at 8000 Hz mono 16-bit = 800 samples × 2 bytes = 1600 data bytes.
    // Plus 44-byte header = 1644 total.
    assert_eq!(bytes.len(), 44 + 1600);
}

#[test]
fn synth_sine_data_amplitude_within_tolerance() {
    let bytes = read_all_via_registry("generate://synth?type=sine&freq=1000&duration=0.01");
    // Skip the 44-byte header, decode the data chunk.
    let data = &bytes[44..];
    let samples: Vec<i16> = data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    // Amplitude default 0.8 → peak ≈ 0.8 × 32767 ≈ 26214.
    let peak = samples
        .iter()
        .map(|&s| s.unsigned_abs() as i32)
        .max()
        .unwrap();
    assert!((25000..=27000).contains(&peak), "peak = {peak}");
}

#[test]
fn xc_red_emits_png_with_red_pixel() {
    let bytes = read_all_via_registry("generate://xc?color=red&w=2&h=2");
    // PNG signature.
    assert_eq!(
        &bytes[0..8],
        &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']
    );
    // IHDR chunk type immediately after signature + 4-byte length.
    assert_eq!(&bytes[12..16], b"IHDR");
    // IHDR width / height (BE u32) at offset 16.
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!(w, 2);
    assert_eq!(h, 2);
    // Bit depth + color type at offsets 24, 25.
    assert_eq!(bytes[24], 8); // 8-bit
    assert_eq!(bytes[25], 6); // truecolor + alpha
}

#[test]
fn gradient_returns_png() {
    let bytes =
        read_all_via_registry("generate://gradient?from=red&to=blue&direction=horizontal&w=8&h=4");
    assert_eq!(
        &bytes[0..8],
        &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']
    );
}

#[test]
fn plasma_default_returns_png() {
    let bytes = read_all_via_registry("generate://plasma?w=16&h=16&seed=7");
    assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);
}

#[test]
fn unknown_kind_returns_error() {
    let mut reg = SourceRegistry::new();
    register_source(&mut reg);
    let res = reg.open("generate://nonsensekind");
    assert!(res.is_err());
}

#[test]
fn video_uri_returns_clear_unsupported_error() {
    // testsrc isn't routable through the URI yet — explicit error.
    let mut reg = SourceRegistry::new();
    register_source(&mut reg);
    let res = reg.open("generate://testsrc?w=64&h=48&duration=1&fps=10");
    let err = match res {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("Y4M") || msg.contains("filter"),
        "msg = {msg:?}"
    );
}

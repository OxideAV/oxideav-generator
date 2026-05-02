//! Integration: open `generate://` URIs through a `SourceRegistry`,
//! pull frames out via the [`FrameSource`] trait, and confirm both the
//! `CodecParameters` shape and the produced sample / pixel data are
//! what the generator promised.

use oxideav_core::{
    Error, Frame, MediaType, PixelFormat, Rational, Result, SampleFormat, SourceOutput,
    SourceRegistry,
};
use oxideav_generator::register_source;

fn registry() -> SourceRegistry {
    let mut reg = SourceRegistry::new();
    register_source(&mut reg);
    reg
}

fn open_frames(reg: &SourceRegistry, uri: &str) -> Box<dyn oxideav_core::FrameSource> {
    match reg.open(uri).expect("open generate:// URI") {
        SourceOutput::Frames(f) => f,
        SourceOutput::Bytes(_) => panic!("expected Frames variant for {uri}, got Bytes"),
        SourceOutput::Packets(_) => panic!("expected Frames variant for {uri}, got Packets"),
    }
}

fn drain(src: &mut dyn oxideav_core::FrameSource) -> Result<Vec<Frame>> {
    let mut out = Vec::new();
    loop {
        match src.next_frame() {
            Ok(f) => out.push(f),
            Err(Error::Eof) => return Ok(out),
            Err(e) => return Err(e),
        }
    }
}

// ---------------------------- Audio ----------------------------

#[test]
fn synth_sine_returns_frames_variant_with_correct_params() {
    let reg = registry();
    let src = open_frames(&reg, "generate://synth?type=sine&freq=440&duration=0.1");
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Audio);
    assert_eq!(p.codec_id.as_str(), "pcm_s16le");
    assert_eq!(p.sample_rate, Some(8000));
    assert_eq!(p.channels, Some(1));
    assert_eq!(p.sample_format, Some(SampleFormat::S16));
    // 0.1 s × 8000 Hz mono = 800 samples → 800 µs * 1000 = 100_000 µs.
    assert_eq!(src.duration_micros(), Some(100_000));
}

#[test]
fn synth_sine_emits_one_audio_frame_with_correct_pcm_length() {
    let reg = registry();
    let mut src = open_frames(&reg, "generate://synth?type=sine&freq=440&duration=0.1");
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Audio(a) = &frames[0] else {
        panic!("expected audio frame");
    };
    // 0.1 s × 8000 Hz = 800 samples / channel; mono S16 → 1 plane × 1600 bytes.
    assert_eq!(a.samples, 800);
    assert_eq!(a.data.len(), 1);
    assert_eq!(a.data[0].len(), 1600);
}

#[test]
fn synth_sine_pcm_amplitude_within_tolerance() {
    let reg = registry();
    let mut src = open_frames(&reg, "generate://synth?type=sine&freq=1000&duration=0.01");
    let frames = drain(&mut *src).unwrap();
    let Frame::Audio(a) = &frames[0] else {
        panic!();
    };
    let samples: Vec<i16> = a.data[0]
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

// ---------------------------- Image ----------------------------

#[test]
fn xc_red_emits_one_video_frame_with_red_pixel() {
    let reg = registry();
    let mut src = open_frames(&reg, "generate://xc?color=red&w=2&h=2");
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Video);
    assert_eq!(p.width, Some(2));
    assert_eq!(p.height, Some(2));
    assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
    assert_eq!(p.frame_rate, Some(Rational::new(1, 1)));

    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Video(v) = &frames[0] else {
        panic!();
    };
    assert_eq!(v.planes.len(), 1);
    assert_eq!(v.planes[0].stride, 8); // 2 px × RGBA8
    assert_eq!(v.planes[0].data.len(), 16); // 2×2 RGBA8
    assert_eq!(&v.planes[0].data[0..4], &[255, 0, 0, 255]);
}

#[test]
fn gradient_returns_one_video_frame() {
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://gradient?from=red&to=blue&direction=horizontal&w=8&h=4",
    );
    let p = src.params();
    assert_eq!(p.width, Some(8));
    assert_eq!(p.height, Some(4));

    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Video(v) = &frames[0] else {
        panic!();
    };
    // 8 × 4 RGBA = 128 bytes; first pixel is "red".
    assert_eq!(v.planes[0].data.len(), 128);
    assert_eq!(&v.planes[0].data[0..4], &[255, 0, 0, 255]);
}

#[test]
fn plasma_default_returns_one_video_frame() {
    let reg = registry();
    let mut src = open_frames(&reg, "generate://plasma?w=16&h=16&seed=7");
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Video(v) = &frames[0] else {
        panic!();
    };
    assert_eq!(v.planes[0].data.len(), 16 * 16 * 4);
}

// ---------------------------- Video ----------------------------

#[test]
fn video_testsrc_returns_frames_variant_no_more_unsupported() {
    // Round-1 used to bail with `Unsupported` here. Now the URI resolves
    // to a real FrameSource that emits frames.
    let reg = registry();
    let mut src = open_frames(&reg, "generate://testsrc?w=32&h=16&duration=0.2&fps=10");
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Video);
    assert_eq!(p.width, Some(32));
    assert_eq!(p.height, Some(16));
    assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
    assert_eq!(p.frame_rate, Some(Rational::new(10, 1)));

    let frames = drain(&mut *src).unwrap();
    // 0.2 s × 10 fps = 2 frames.
    assert_eq!(frames.len(), 2);
    for (i, f) in frames.iter().enumerate() {
        let Frame::Video(v) = f else { panic!() };
        assert_eq!(v.pts, Some(i as i64));
        assert_eq!(v.planes[0].stride, 32 * 4);
        assert_eq!(v.planes[0].data.len(), 32 * 16 * 4);
    }
}

#[test]
fn video_smptebars_returns_frames_variant() {
    let reg = registry();
    let mut src = open_frames(&reg, "generate://smptebars?w=8&h=8&duration=0.1&fps=10");
    let frames = drain(&mut *src).unwrap();
    // 0.1 s × 10 fps = 1 frame.
    assert_eq!(frames.len(), 1);
}

#[test]
fn video_gradient_animate_returns_frames_variant() {
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://gradient_animate?w=8&h=4&duration=0.2&fps=10&hue_rate=60",
    );
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 2);
}

// ---------------------------- Errors ----------------------------

#[test]
fn unknown_kind_returns_error() {
    let reg = registry();
    let res = reg.open("generate://nonsensekind");
    assert!(res.is_err());
}

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
        _ => panic!("expected Frames variant for {uri}, got an unknown SourceOutput variant"),
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

#[test]
fn grating_default_returns_one_video_frame_with_unit_peak() {
    // freq=0 + phase=0 → cos(0) = 1 across the whole image → every
    // pixel is RGBA (255, 255, 255, 255). 4×4 = 64 bytes.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://grating?w=4&h=4&freq=0&phase=0&amplitude=1",
    );
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Video(v) = &frames[0] else {
        panic!();
    };
    assert_eq!(v.planes[0].data.len(), 64);
    for chunk in v.planes[0].data.chunks_exact(4) {
        assert_eq!(chunk, &[255, 255, 255, 255]);
    }
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

#[test]
fn video_zoneplate_returns_frames_variant() {
    // Zone plate at 9×9 (odd-sized so the centre is an integer pixel)
    // with amplitude=1 produces a peak-luma centre. 0.2 s × 10 fps = 2
    // frames, both identical with motion=none.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://zoneplate?w=9&h=9&duration=0.2&fps=10&k=0.05",
    );
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Video);
    assert_eq!(p.width, Some(9));
    assert_eq!(p.height, Some(9));
    assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
    assert_eq!(p.frame_rate, Some(Rational::new(10, 1)));
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 2);
    // Centre pixel of the first frame is white (cos(0) = 1).
    let Frame::Video(v) = &frames[0] else {
        panic!();
    };
    let stride = v.planes[0].stride;
    let centre_offset = 4 * stride + 4 * 4; // (x=4, y=4)
    assert_eq!(
        &v.planes[0].data[centre_offset..centre_offset + 4],
        &[255, 255, 255, 255]
    );
}

#[test]
fn video_scroll_returns_translated_frames() {
    // Constant-velocity scroll of an 8×8 checkerboard (cell=4) at
    // vx=4/frame: after one frame the board has shifted by exactly one
    // cell, so pixel (0, 0) flips colour between frame 0 and frame 1.
    // 0.2 s × 10 fps = 2 frames.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://scroll?w=8&h=8&size=4&vx=4&vy=0&duration=0.2&fps=10",
    );
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Video);
    assert_eq!(p.width, Some(8));
    assert_eq!(p.height, Some(8));
    assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
    assert_eq!(p.frame_rate, Some(Rational::new(10, 1)));
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 2);
    let (Frame::Video(v0), Frame::Video(v1)) = (&frames[0], &frames[1]) else {
        panic!("expected two video frames");
    };
    // Frame 0: top-left cell is color1 (black). Frame 1: the white
    // cell has scrolled in.
    assert_eq!(&v0.planes[0].data[0..4], &[0, 0, 0, 255]);
    assert_eq!(&v1.planes[0].data[0..4], &[255, 255, 255, 255]);
}

#[test]
fn video_colorwheel_returns_frames_variant() {
    // Rotating colour wheel at 9×9 (odd-sized so the centre is an
    // integer pixel). With the default mid lightness and zero
    // saturation at r=0, the centre pixel is a pure grey (127 on every
    // channel). 0.3 s × 10 fps = 3 frames; with spin>0 frame 0 and the
    // last frame differ.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://colorwheel?w=9&h=9&duration=0.3&fps=10&spin=120",
    );
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Video);
    assert_eq!(p.width, Some(9));
    assert_eq!(p.height, Some(9));
    assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
    assert_eq!(p.frame_rate, Some(Rational::new(10, 1)));
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 3);
    let (Frame::Video(v0), Frame::Video(vlast)) = (&frames[0], frames.last().unwrap()) else {
        panic!("expected video frames");
    };
    // Centre pixel (x=4, y=4) is achromatic grey.
    let stride = v0.planes[0].stride;
    let centre = 4 * stride + 4 * 4;
    let c = &v0.planes[0].data[centre..centre + 4];
    assert_eq!(c[0], c[1]);
    assert_eq!(c[1], c[2]);
    assert_eq!(c[3], 255);
    // The wheel rotates: the full frame buffer changes over the run.
    assert_ne!(v0.planes[0].data, vlast.planes[0].data);
}

#[test]
fn video_movingbox_returns_exact_box_positions() {
    // 2×2 white box on black, starting at the origin, moving 2 px/frame
    // rightward on an 8×4 frame. 0.2 s × 10 fps = 2 frames. Frame 0 has
    // the box at x ∈ {0, 1}; frame 1 at x ∈ {2, 3} — pixel (0, 0) flips
    // from fg to bg and pixel (2, 0) the other way.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://movingbox?w=8&h=4&bw=2&bh=2&vx=2&vy=0&duration=0.2&fps=10",
    );
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Video);
    assert_eq!(p.width, Some(8));
    assert_eq!(p.height, Some(4));
    assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
    assert_eq!(p.frame_rate, Some(Rational::new(10, 1)));
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 2);
    let (Frame::Video(v0), Frame::Video(v1)) = (&frames[0], &frames[1]) else {
        panic!("expected two video frames");
    };
    assert_eq!(&v0.planes[0].data[0..4], &[255, 255, 255, 255]);
    assert_eq!(&v1.planes[0].data[0..4], &[0, 0, 0, 255]);
    assert_eq!(&v1.planes[0].data[8..12], &[255, 255, 255, 255]); // (2, 0)
}

#[test]
fn video_box_alias_matches_movingbox() {
    let reg = registry();
    let mut a = open_frames(
        &reg,
        "generate://movingbox?w=8&h=4&bw=2&bh=2&vx=1&duration=0.2&fps=10",
    );
    let mut b = open_frames(
        &reg,
        "generate://box?w=8&h=4&bw=2&bh=2&vx=1&duration=0.2&fps=10",
    );
    let fa = drain(&mut *a).unwrap();
    let fb = drain(&mut *b).unwrap();
    assert_eq!(fa.len(), fb.len());
    for (x, y) in fa.iter().zip(fb.iter()) {
        let (Frame::Video(x), Frame::Video(y)) = (x, y) else {
            panic!("expected video frames");
        };
        assert_eq!(x.planes[0].data, y.planes[0].data);
    }
}

#[test]
fn synth_chirp_returns_audio_frames() {
    // 0.05 s sweep from 200 → 800 Hz, linear. 8000 × 0.05 = 400 mono
    // samples × 2 bytes = 800 bytes of S16 LE PCM.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://synth?type=chirp&f0=200&f1=800&shape=linear&duration=0.05",
    );
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Audio);
    assert_eq!(p.codec_id.as_str(), "pcm_s16le");
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Audio(a) = &frames[0] else {
        panic!();
    };
    assert_eq!(a.samples, 400);
    assert_eq!(a.data[0].len(), 800);
}

#[test]
fn synth_fm_returns_audio_frames() {
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://synth?type=fm&carrier=440&modulator=110&index=4&duration=0.02",
    );
    let frames = drain(&mut *src).unwrap();
    let Frame::Audio(a) = &frames[0] else {
        panic!();
    };
    // 8000 × 0.02 = 160 samples × S16 LE = 320 bytes.
    assert_eq!(a.samples, 160);
    assert_eq!(a.data[0].len(), 320);
}

#[test]
fn synth_vibrato_returns_audio_frames_with_correct_sample_count() {
    // 0.05 s × 8000 Hz = 400 mono samples × S16 LE = 800 bytes. The
    // single emitted AudioFrame must report 400 samples on its `samples`
    // count + carry exactly the expected byte length on its one data
    // plane. Single-frame URI → drain → frame-shape roundtrip — the same
    // shape probe used for every other in-tree synth type. The carrier
    // is a sine at 440 Hz with the default ±0.5 % vibrato depth at 5 Hz.
    let reg = registry();
    let mut src = open_frames(&reg, "generate://synth?type=vibrato&duration=0.05");
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Audio);
    assert_eq!(p.codec_id.as_str(), "pcm_s16le");
    assert_eq!(p.sample_rate, Some(8000));
    assert_eq!(p.channels, Some(1));
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Audio(a) = &frames[0] else {
        panic!("expected audio frame for vibrato");
    };
    assert_eq!(a.samples, 400);
    assert_eq!(a.data.len(), 1);
    assert_eq!(a.data[0].len(), 800);
    // The synthesised samples must stay inside the S16 amplitude bound
    // implied by amplitude=0.8 (the global default) — vibrato passes
    // the carrier's `amplitude` through unchanged because the phase
    // reshuffling cannot push the oscillator outside its own image.
    let samples: Vec<i16> = a.data[0]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let peak = samples
        .iter()
        .map(|&s| s.unsigned_abs() as i32)
        .max()
        .unwrap();
    // amplitude 0.8 × 32767 ≈ 26214; allow ≤ 27000 the same generous
    // headroom the shepard probe uses (the actual peak is a sine
    // amplitude bound, well clear of the assertion).
    assert!(peak <= 27000, "vibrato peak {peak} exceeds 0.8 bound");
}

#[test]
fn synth_shepard_returns_audio_frames_with_correct_sample_count() {
    // 0.05 s × 8000 Hz = 400 mono samples × S16 LE = 800 bytes. The
    // single emitted AudioFrame must report 400 samples on its `samples`
    // count + carry exactly the expected byte length on its one data
    // plane. Single-frame URI → drain → frame-shape roundtrip.
    let reg = registry();
    let mut src = open_frames(&reg, "generate://synth?type=shepard&voices=6&duration=0.05");
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Audio);
    assert_eq!(p.codec_id.as_str(), "pcm_s16le");
    assert_eq!(p.sample_rate, Some(8000));
    assert_eq!(p.channels, Some(1));
    let frames = drain(&mut *src).unwrap();
    assert_eq!(frames.len(), 1);
    let Frame::Audio(a) = &frames[0] else {
        panic!("expected audio frame for shepard");
    };
    assert_eq!(a.samples, 400);
    assert_eq!(a.data.len(), 1);
    assert_eq!(a.data[0].len(), 800);
    // The synthesised samples must stay inside the S16 amplitude bound
    // implied by amplitude=0.8 (the global default) — the Shepard stack
    // is normalised by Σ weights so a single voice's peak is the
    // worst-case alignment.
    let samples: Vec<i16> = a.data[0]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    let peak = samples
        .iter()
        .map(|&s| s.unsigned_abs() as i32)
        .max()
        .unwrap();
    // amplitude 0.8 × 32767 ≈ 26214; allow generous headroom because
    // the actual aligned peak across voices is well below that.
    assert!(peak <= 27000, "shepard peak {peak} exceeds 0.8 bound");
}

#[test]
fn synth_multitone_returns_audio_frames() {
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://synth?type=multitone&freqs=440,1000,2200&duration=0.01",
    );
    let frames = drain(&mut *src).unwrap();
    let Frame::Audio(a) = &frames[0] else {
        panic!();
    };
    assert_eq!(a.samples, 80);
}

#[test]
fn synth_dtmf_returns_audio_frames_with_sequence_length() {
    // Two keys × (0.1 s tone + 0.05 s gap) at 8000 Hz =
    // 2 × (800 + 400) = 2400 mono samples × 2 bytes = 4800 bytes.
    // `duration=` is intentionally absent — dtmf derives its length
    // from the dialled string + tone/gap timing.
    let reg = registry();
    let mut src = open_frames(
        &reg,
        "generate://synth?type=dtmf&digits=12&tone=0.1&gap=0.05",
    );
    let p = src.params();
    assert_eq!(p.media_type, MediaType::Audio);
    assert_eq!(p.codec_id.as_str(), "pcm_s16le");
    let frames = drain(&mut *src).unwrap();
    let Frame::Audio(a) = &frames[0] else {
        panic!();
    };
    assert_eq!(a.samples, 2400);
    assert_eq!(a.data[0].len(), 4800);
}

// ---------------------------- Errors ----------------------------

#[test]
fn unknown_kind_returns_error() {
    let reg = registry();
    let res = reg.open("generate://nonsensekind");
    assert!(res.is_err());
}

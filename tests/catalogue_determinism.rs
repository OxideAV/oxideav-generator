//! Cross-cutting determinism contract: every generator kind in the
//! URI catalogue, opened twice with identical parameters, must
//! produce byte-identical output — audio PCM bytes and video plane
//! bytes alike. This is the crate-wide guarantee that makes generated
//! streams usable as codec fixtures: same params (+ seed where one
//! exists) ⇒ the same bytes, on any machine, in any run.
//!
//! The URI lists double as catalogue-rot detection: every kind the
//! dispatcher knows must open successfully and yield at least one
//! frame with tiny parameters.

use oxideav_core::{Error, Frame, Result, SourceOutput, SourceRegistry};
use oxideav_generator::register_source;

fn registry() -> SourceRegistry {
    let mut reg = SourceRegistry::new();
    register_source(&mut reg);
    reg
}

/// Open a URI and drain every frame's raw bytes (audio planes +
/// video planes, concatenated in frame order).
fn drain_bytes(reg: &SourceRegistry, uri: &str) -> Result<Vec<Vec<u8>>> {
    let mut src = match reg.open(uri)? {
        SourceOutput::Frames(f) => f,
        _ => panic!("expected Frames variant for {uri}"),
    };
    let mut out = Vec::new();
    loop {
        match src.next_frame() {
            Ok(Frame::Audio(a)) => out.extend(a.data),
            Ok(Frame::Video(v)) => out.extend(v.planes.into_iter().map(|p| p.data)),
            Ok(_) => panic!("unexpected frame type from {uri}"),
            Err(Error::Eof) => return Ok(out),
            Err(e) => return Err(e),
        }
    }
}

fn assert_deterministic(uris: &[&str]) {
    let reg = registry();
    for uri in uris {
        let a = drain_bytes(&reg, uri).unwrap_or_else(|e| panic!("{uri}: {e}"));
        assert!(!a.is_empty(), "{uri}: produced no frames");
        assert!(
            a.iter().any(|b| !b.is_empty()),
            "{uri}: produced only empty buffers"
        );
        let b = drain_bytes(&reg, uri).unwrap();
        assert_eq!(a, b, "{uri}: two renders with identical params differ");
    }
}

#[test]
fn every_audio_synth_type_is_byte_deterministic() {
    assert_deterministic(&[
        "generate://synth?type=sine&freq=440&duration=0.02",
        "generate://synth?type=sine&freq=440&phase=90&channels=2&chphase=90&duration=0.02",
        "generate://synth?type=square&freq=220&duration=0.02",
        "generate://synth?type=triangle&freq=220&duration=0.02",
        "generate://synth?type=sawtooth&freq=220&duration=0.02",
        "generate://synth?type=supersaw&freq=220&voices=3&detune=8&duration=0.02",
        "generate://synth?type=pwm&freq=220&duty=0.25&duration=0.02",
        "generate://synth?type=pluck&freq=220&duration=0.02",
        "generate://synth?type=chirp&shape=linear&f0=100&f1=1000&duration=0.02",
        "generate://synth?type=chirp&shape=exp&f0=100&f1=1000&duration=0.02",
        "generate://synth?type=fm&carrier=440&modulator=110&index=2&duration=0.02",
        "generate://synth?type=am&carrier=440&modulator=60&index=0.5&duration=0.02",
        "generate://synth?type=tremolo&wave=sine&freq=440&lfo=5&depth=0.7&duration=0.02",
        "generate://synth?type=vibrato&wave=sine&freq=440&lfo=5&depth=0.005&duration=0.02",
        "generate://synth?type=ringmod&f1=440&f2=60&duration=0.02",
        "generate://synth?type=dtmf&digits=42&tone=0.01&gap=0.005",
        "generate://synth?type=adsr&wave=sine&freq=440&attack=0.005&decay=0.005&sustain=0.7&release=0.005&duration=0.02",
        "generate://synth?type=formant&vowel=A&f0=220&duration=0.02",
        "generate://synth?type=shepard&voices=4&duration=0.02",
        "generate://synth?type=multitone&freqs=440,1000&duration=0.02",
        "generate://synth?type=noise&color=white&seed=42&duration=0.02",
        "generate://synth?type=noise&color=pink&seed=42&duration=0.02",
        "generate://synth?type=noise&color=brown&seed=42&duration=0.02",
        "generate://synth?type=noise&color=blue&seed=42&duration=0.02",
        "generate://synth?type=noise&color=violet&seed=42&duration=0.02",
        "generate://synth?type=silence&duration=0.02",
        "generate://synth?type=dc&level=0.25&duration=0.02",
        "generate://synth?type=impulse&period=10&duration=0.02",
    ]);
}

#[test]
fn every_image_kind_is_byte_deterministic() {
    assert_deterministic(&[
        "generate://xc?color=red&w=8&h=8",
        "generate://gradient?from=red&to=blue&direction=horizontal&w=8&h=8",
        "generate://gradient?from=red&to=blue&type=radial&w=8&h=8",
        "generate://pattern?type=checkerboard&w=8&h=8&size=2",
        "generate://pattern?type=hstripes&w=8&h=8&size=2",
        "generate://pattern?type=vstripes&w=8&h=8&size=2",
        "generate://grating?w=8&h=8&freq=2&angle=45",
        "generate://ramp?w=8&h=2&bits=3",
        "generate://fractal?type=mandelbrot&w=8&h=8&iter=16",
        "generate://fractal?type=julia&w=8&h=8&iter=16",
        "generate://plasma?w=8&h=8&seed=42",
        "generate://noise?type=perlin&w=8&h=8&scale=4&seed=42",
        "generate://noise?type=simplex&w=8&h=8&scale=4&seed=42",
        "generate://noise?type=value&w=8&h=8&scale=4&seed=42",
        "generate://noise?type=worley&w=8&h=8&scale=4&seed=42",
    ]);
}

#[cfg(feature = "label")]
#[test]
fn label_generator_is_byte_deterministic() {
    assert_deterministic(&["generate://label?text=Hi&size=12"]);
}

#[test]
fn every_video_kind_is_byte_deterministic() {
    assert_deterministic(&[
        "generate://testsrc?w=16&h=8&duration=0.2&fps=10",
        "generate://smptebars?w=16&h=8&duration=0.2&fps=10",
        "generate://fractal_zoom?w=8&h=8&duration=0.2&fps=10&iter=16",
        "generate://gradient_animate?w=8&h=8&duration=0.2&fps=10",
        "generate://zoneplate?w=8&h=8&duration=0.2&fps=10",
        "generate://scroll?pattern=checkerboard&size=2&vx=1&w=8&h=8&duration=0.2&fps=10",
        "generate://colorwheel?w=8&h=8&duration=0.2&fps=10&spin=90",
        "generate://movingbox?w=8&h=8&bw=2&bh=2&vx=1&vy=1&duration=0.2&fps=10",
        "generate://box?w=8&h=8&bw=2&bh=2&vx=1&duration=0.2&fps=10",
        "generate://snow?w=8&h=8&seed=42&duration=0.2&fps=10",
        "generate://snow?w=8&h=8&seed=42&mode=rgb&duration=0.2&fps=10",
    ]);
}

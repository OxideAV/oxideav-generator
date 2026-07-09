//! Integration: invoke each generator filter as a zero-input
//! [`StreamFilter`] and confirm it emits the expected number of frames
//! with the right shape.

use oxideav_core::filter::FilterContext;
use oxideav_core::{Frame, RuntimeContext};

use oxideav_generator::register_filters;

struct CollectCtx {
    frames: Vec<Frame>,
}
impl FilterContext for CollectCtx {
    fn emit(&mut self, _output_port: usize, frame: Frame) -> oxideav_core::Result<()> {
        self.frames.push(frame);
        Ok(())
    }
}

fn make_ctx() -> RuntimeContext {
    let mut ctx = RuntimeContext::new();
    register_filters(&mut ctx);
    ctx
}

#[test]
fn audio_synth_emits_one_audio_frame() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "type": "sine",
        "freq": 440,
        "duration": 0.05,
        "rate": 8000
    });
    let mut filter = ctx.filters.make("audio.synth", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 1);
    let Frame::Audio(a) = &sink.frames[0] else {
        panic!("expected audio frame");
    };
    // 0.05s × 8000 Hz = 400 samples per channel; 2 bytes per sample × 1 ch = 800 bytes.
    assert_eq!(a.samples, 400);
    assert_eq!(a.data[0].len(), 800);
}

#[test]
fn image_xc_emits_one_video_frame() {
    let ctx = make_ctx();
    let params = serde_json::json!({"color": "blue", "w": 4, "h": 4});
    let mut filter = ctx.filters.make("image.xc", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 1);
}

#[test]
fn video_testsrc_emits_n_frames() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "w": 32, "h": 16,
        "duration": 0.3, "fps": 10
    });
    let mut filter = ctx.filters.make("video.testsrc", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 3);
}

#[test]
fn video_gradient_animate_emits_frames() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "w": 8, "h": 4,
        "duration": 0.2, "fps": 5,
        "hue_rate": 60
    });
    let mut filter = ctx
        .filters
        .make("video.gradient_animate", &params, &[])
        .unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 1);
}

#[test]
fn video_scroll_emits_n_frames_with_zero_input_ports() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "w": 16, "h": 8,
        "size": 4,
        "vx": 2, "vy": 1,
        "duration": 0.3, "fps": 10
    });
    let filter = ctx.filters.make("video.scroll", &params, &[]).unwrap();
    assert!(filter.input_ports().is_empty());
    assert_eq!(filter.output_ports().len(), 1);

    let mut filter = ctx.filters.make("video.scroll", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 3);
    // Non-zero velocity ⇒ consecutive frames differ.
    let (Frame::Video(a), Frame::Video(b)) = (&sink.frames[0], &sink.frames[1]) else {
        panic!("expected video frames");
    };
    assert_ne!(a.planes[0].data, b.planes[0].data);
}

#[test]
fn video_colorwheel_emits_n_frames_with_zero_input_ports() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "w": 16, "h": 8,
        "spin": 120,
        "duration": 0.3, "fps": 10
    });
    let filter = ctx.filters.make("video.colorwheel", &params, &[]).unwrap();
    assert!(filter.input_ports().is_empty());
    assert_eq!(filter.output_ports().len(), 1);

    let mut filter = ctx.filters.make("video.colorwheel", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 3);
    // Non-zero spin ⇒ consecutive frames differ.
    let (Frame::Video(a), Frame::Video(b)) = (&sink.frames[0], &sink.frames[1]) else {
        panic!("expected video frames");
    };
    assert_ne!(a.planes[0].data, b.planes[0].data);
}

#[test]
fn video_movingbox_emits_n_frames_with_zero_input_ports() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "w": 16, "h": 8,
        "bw": 4, "bh": 4,
        "vx": 2, "vy": 1,
        "duration": 0.3, "fps": 10
    });
    let filter = ctx.filters.make("video.movingbox", &params, &[]).unwrap();
    assert!(filter.input_ports().is_empty());
    assert_eq!(filter.output_ports().len(), 1);

    let mut filter = ctx.filters.make("video.movingbox", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 3);
    // Non-zero velocity ⇒ consecutive frames differ.
    let (Frame::Video(a), Frame::Video(b)) = (&sink.frames[0], &sink.frames[1]) else {
        panic!("expected video frames");
    };
    assert_ne!(a.planes[0].data, b.planes[0].data);
}

#[test]
fn video_snow_emits_n_frames_with_zero_input_ports() {
    let ctx = make_ctx();
    let params = serde_json::json!({
        "w": 16, "h": 8,
        "seed": 42,
        "duration": 0.3, "fps": 10
    });
    let filter = ctx.filters.make("video.snow", &params, &[]).unwrap();
    assert!(filter.input_ports().is_empty());
    assert_eq!(filter.output_ports().len(), 1);

    let mut filter = ctx.filters.make("video.snow", &params, &[]).unwrap();
    let mut sink = CollectCtx { frames: vec![] };
    filter.flush(&mut sink).unwrap();
    assert_eq!(sink.frames.len(), 3);
    // Frame index feeds the hash ⇒ consecutive frames differ.
    let (Frame::Video(a), Frame::Video(b)) = (&sink.frames[0], &sink.frames[1]) else {
        panic!("expected video frames");
    };
    assert_ne!(a.planes[0].data, b.planes[0].data);
}

#[test]
fn image_filters_have_zero_input_ports() {
    let ctx = make_ctx();
    for name in [
        "image.xc",
        "image.gradient",
        "image.pattern",
        "image.grating",
        "image.fractal",
        "image.plasma",
        "image.noise",
        "image.ramp",
    ] {
        let params = serde_json::json!({"w": 8, "h": 8});
        let filter = ctx.filters.make(name, &params, &[]).unwrap();
        assert!(
            filter.input_ports().is_empty(),
            "{name} should have zero input ports"
        );
        assert_eq!(filter.output_ports().len(), 1);
    }
}

#[test]
fn audio_synth_filter_has_zero_input_ports() {
    let ctx = make_ctx();
    let params = serde_json::json!({"type": "silence", "duration": 0.01});
    let filter = ctx.filters.make("audio.synth", &params, &[]).unwrap();
    assert!(filter.input_ports().is_empty());
    assert_eq!(filter.output_ports().len(), 1);
}

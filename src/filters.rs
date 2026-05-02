//! Filter-registry adapters for every generator.
//!
//! Each generator is wrapped in a zero-input [`StreamFilter`] —
//! `input_ports()` returns an empty slice; the entire output is
//! produced in `flush()` (which the executor calls when EOS reaches
//! this stage). For audio the output is a single AudioFrame; for
//! image / video, one or more VideoFrames.

use std::collections::BTreeMap;

use oxideav_core::filter::FilterContext;
use oxideav_core::{
    AudioFrame, Frame, PixelFormat, PortParams, PortSpec, Result, RuntimeContext, SampleFormat,
    StreamFilter, TimeBase, VideoFrame, VideoPlane,
};
use serde_json::Value;

use crate::audio::f32_sample_to_i16;
use crate::audio::synth as audio_synth;
use crate::image::{fractal, gradient, noise, pattern, plasma, xc, Rgba8Image};
use crate::video::{fractal_zoom, gradient_animate, smptebars, testsrc};

/// Install every generator filter into `ctx.filters`.
///
/// Names mirror the URI catalogue:
/// - `audio.synth`
/// - `image.xc`, `image.gradient`, `image.pattern`,
///   `image.fractal`, `image.plasma`, `image.noise`
/// - `video.testsrc`, `video.smptebars`, `video.fractal_zoom`,
///   `video.gradient_animate`
pub fn register_filters(ctx: &mut RuntimeContext) {
    ctx.filters
        .register("audio.synth", Box::new(make_audio_synth));
    ctx.filters.register("image.xc", Box::new(make_image_xc));
    ctx.filters
        .register("image.gradient", Box::new(make_image_gradient));
    ctx.filters
        .register("image.pattern", Box::new(make_image_pattern));
    ctx.filters
        .register("image.fractal", Box::new(make_image_fractal));
    ctx.filters
        .register("image.plasma", Box::new(make_image_plasma));
    ctx.filters
        .register("image.noise", Box::new(make_image_noise));
    ctx.filters
        .register("video.testsrc", Box::new(make_video_testsrc));
    ctx.filters
        .register("video.smptebars", Box::new(make_video_smptebars));
    ctx.filters
        .register("video.fractal_zoom", Box::new(make_video_fractal_zoom));
    ctx.filters.register(
        "video.gradient_animate",
        Box::new(make_video_gradient_animate),
    );
}

/// Convert a JSON `params` object into the `BTreeMap<String, String>`
/// shape the URI parsers consume. Numeric / bool values are stringified
/// so the same query-string code path serves both transports.
fn params_to_query(params: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(obj) = params.as_object() {
        for (k, v) in obj {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => String::new(),
                _ => v.to_string(),
            };
            out.insert(k.clone(), s);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

fn make_audio_synth(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    let q = params_to_query(params);
    let buf = audio_synth::render(&q)?;
    let port = PortSpec::audio("audio", buf.sample_rate, buf.channels, SampleFormat::S16);
    Ok(Box::new(AudioSynthFilter {
        out_port: [port],
        samples: buf.samples,
        channels: buf.channels,
        emitted: false,
    }))
}

struct AudioSynthFilter {
    out_port: [PortSpec; 1],
    samples: Vec<f32>,
    channels: u16,
    emitted: bool,
}

impl StreamFilter for AudioSynthFilter {
    fn input_ports(&self) -> &[PortSpec] {
        &[]
    }
    fn output_ports(&self) -> &[PortSpec] {
        &self.out_port
    }
    fn push(&mut self, _ctx: &mut dyn FilterContext, _port: usize, _frame: &Frame) -> Result<()> {
        // No input ports — push should never reach us.
        Ok(())
    }
    fn flush(&mut self, ctx: &mut dyn FilterContext) -> Result<()> {
        if self.emitted {
            return Ok(());
        }
        self.emitted = true;
        // Convert to interleaved S16 bytes.
        let pcm: Vec<i16> = self.samples.iter().map(|&x| f32_sample_to_i16(x)).collect();
        let mut bytes = Vec::with_capacity(pcm.len() * 2);
        for s in pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let samples_per_channel = (self.samples.len() / self.channels.max(1) as usize) as u32;
        ctx.emit(
            0,
            Frame::Audio(AudioFrame {
                samples: samples_per_channel,
                pts: Some(0),
                data: vec![bytes],
            }),
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Images (single still frame each)
// ---------------------------------------------------------------------------

fn make_image_xc(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    image_filter(xc::render(&params_to_query(params))?)
}
fn make_image_gradient(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    image_filter(gradient::render(&params_to_query(params))?)
}
fn make_image_pattern(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    image_filter(pattern::render(&params_to_query(params))?)
}
fn make_image_fractal(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    image_filter(fractal::render(&params_to_query(params))?)
}
fn make_image_plasma(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    image_filter(plasma::render(&params_to_query(params))?)
}
fn make_image_noise(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    image_filter(noise::render(&params_to_query(params))?)
}

fn image_filter(img: Rgba8Image) -> Result<Box<dyn StreamFilter>> {
    let port = PortSpec::video(
        "video",
        img.width,
        img.height,
        PixelFormat::Rgba,
        TimeBase::new(1, 1),
    );
    Ok(Box::new(SingleFrameFilter {
        out_port: [port],
        frame: Some(rgba_image_to_video_frame(img, 0)),
    }))
}

struct SingleFrameFilter {
    out_port: [PortSpec; 1],
    frame: Option<VideoFrame>,
}

impl StreamFilter for SingleFrameFilter {
    fn input_ports(&self) -> &[PortSpec] {
        &[]
    }
    fn output_ports(&self) -> &[PortSpec] {
        &self.out_port
    }
    fn push(&mut self, _ctx: &mut dyn FilterContext, _port: usize, _frame: &Frame) -> Result<()> {
        Ok(())
    }
    fn flush(&mut self, ctx: &mut dyn FilterContext) -> Result<()> {
        if let Some(f) = self.frame.take() {
            ctx.emit(0, Frame::Video(f))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Video (frame sequences)
// ---------------------------------------------------------------------------

fn make_video_testsrc(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    let seq = testsrc::render(&params_to_query(params))?;
    Ok(video_filter_from_seq(seq))
}
fn make_video_smptebars(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    let seq = smptebars::render(&params_to_query(params))?;
    Ok(video_filter_from_seq(seq))
}
fn make_video_fractal_zoom(params: &Value, _inputs: &[PortSpec]) -> Result<Box<dyn StreamFilter>> {
    let seq = fractal_zoom::render(&params_to_query(params))?;
    Ok(video_filter_from_seq(seq))
}
fn make_video_gradient_animate(
    params: &Value,
    _inputs: &[PortSpec],
) -> Result<Box<dyn StreamFilter>> {
    let seq = gradient_animate::render(&params_to_query(params))?;
    Ok(video_filter_from_seq(seq))
}

fn video_filter_from_seq(seq: crate::video::FrameSeq) -> Box<dyn StreamFilter> {
    let (w, h) = seq
        .frames
        .first()
        .map(|f| (f.width, f.height))
        .unwrap_or((0, 0));
    let port = PortSpec::video(
        "video",
        w,
        h,
        PixelFormat::Rgba,
        TimeBase::new(1, seq.fps.max(1) as i64),
    );
    let frames: Vec<VideoFrame> = seq
        .frames
        .into_iter()
        .enumerate()
        .map(|(i, img)| rgba_image_to_video_frame(img, i as i64))
        .collect();
    Box::new(MultiFrameFilter {
        out_port: [port],
        frames,
        emitted: false,
    })
}

struct MultiFrameFilter {
    out_port: [PortSpec; 1],
    frames: Vec<VideoFrame>,
    emitted: bool,
}

impl StreamFilter for MultiFrameFilter {
    fn input_ports(&self) -> &[PortSpec] {
        &[]
    }
    fn output_ports(&self) -> &[PortSpec] {
        &self.out_port
    }
    fn push(&mut self, _ctx: &mut dyn FilterContext, _port: usize, _frame: &Frame) -> Result<()> {
        Ok(())
    }
    fn flush(&mut self, ctx: &mut dyn FilterContext) -> Result<()> {
        if self.emitted {
            return Ok(());
        }
        self.emitted = true;
        for frame in std::mem::take(&mut self.frames) {
            ctx.emit(0, Frame::Video(frame))?;
        }
        Ok(())
    }
}

fn rgba_image_to_video_frame(img: Rgba8Image, pts: i64) -> VideoFrame {
    let stride = (img.width as usize) * 4;
    VideoFrame {
        pts: Some(pts),
        planes: vec![VideoPlane {
            stride,
            data: img.pixels,
        }],
    }
}

// Re-export so the lib's _ = ... pattern compiles even if the helper
// is unused by external code.
#[allow(dead_code)]
fn _ensure_pp_used() {
    let _ = PortParams::Subtitle; // suppress "unused import" if the layout shifts
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CollectCtx {
        frames: Vec<Frame>,
    }
    impl FilterContext for CollectCtx {
        fn emit(&mut self, _output_port: usize, frame: Frame) -> Result<()> {
            self.frames.push(frame);
            Ok(())
        }
    }

    #[test]
    fn audio_synth_filter_emits_one_audio_frame() {
        let params: Value = serde_json::json!({
            "type": "sine",
            "freq": 440,
            "duration": 0.01,
            "rate": 8000
        });
        let mut filter = make_audio_synth(&params, &[]).unwrap();
        let mut ctx = CollectCtx { frames: vec![] };
        filter.flush(&mut ctx).unwrap();
        assert_eq!(ctx.frames.len(), 1);
        match &ctx.frames[0] {
            Frame::Audio(a) => {
                assert_eq!(a.samples, 80);
                assert_eq!(a.data.len(), 1);
                assert_eq!(a.data[0].len(), 80 * 2);
            }
            other => panic!("expected audio, got {other:?}"),
        }
    }

    #[test]
    fn image_xc_filter_emits_one_video_frame() {
        let params = serde_json::json!({"color": "red", "w": 4, "h": 4});
        let mut filter = make_image_xc(&params, &[]).unwrap();
        let mut ctx = CollectCtx { frames: vec![] };
        filter.flush(&mut ctx).unwrap();
        assert_eq!(ctx.frames.len(), 1);
        match &ctx.frames[0] {
            Frame::Video(v) => {
                assert_eq!(v.planes.len(), 1);
                assert_eq!(v.planes[0].stride, 16);
                assert_eq!(v.planes[0].data.len(), 64);
                // First pixel red.
                assert_eq!(&v.planes[0].data[0..4], &[255, 0, 0, 255]);
            }
            other => panic!("expected video, got {other:?}"),
        }
    }

    #[test]
    fn video_testsrc_filter_emits_n_frames() {
        let params = serde_json::json!({
            "w": 32, "h": 16,
            "duration": 0.2, "fps": 10
        });
        let mut filter = make_video_testsrc(&params, &[]).unwrap();
        let mut ctx = CollectCtx { frames: vec![] };
        filter.flush(&mut ctx).unwrap();
        assert_eq!(ctx.frames.len(), 2); // 0.2s × 10fps = 2 frames
    }
}

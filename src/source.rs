//! `generate://` URI scheme opener.
//!
//! Parses a query-string-shaped URI like
//! `generate://synth?type=sine&freq=440&duration=5`, dispatches to the
//! matching generator, and returns it as a
//! [`FrameSource`](oxideav_core::FrameSource) — frames are produced
//! natively (audio: one [`AudioFrame`](oxideav_core::AudioFrame) per
//! call until the configured duration is exhausted; image: a single
//! still [`VideoFrame`](oxideav_core::VideoFrame) followed by `Eof`;
//! video: one [`VideoFrame`](oxideav_core::VideoFrame) per call until
//! the configured frame count is exhausted).
//!
//! No container layer is involved — the executor consumes
//! [`SourceOutput::Frames`](oxideav_core::SourceOutput::Frames)
//! directly, skipping both demux + decode for synthetic sources.

use std::collections::BTreeMap;

use oxideav_core::{
    AudioFrame, ChannelLayout, CodecId, CodecParameters, Error, Frame, FrameSource, PixelFormat,
    Rational, Result, SampleFormat, SourceRegistry, VideoFrame, VideoPlane,
};

use crate::audio::synth as audio_synth;
#[cfg(feature = "label")]
use crate::image::label;
use crate::image::{fractal, gradient, grating, noise, pattern, plasma, xc, Rgba8Image};
use crate::video::{
    colorwheel, fractal_zoom, gradient_animate, scroll, smptebars, testsrc, zoneplate, FrameSeq,
};

/// Register the `generate` URI scheme as a [`FrameSource`] driver.
pub fn register_source(registry: &mut SourceRegistry) {
    registry.register_frames("generate", open_generate_frames);
}

/// Opener for `generate://...` URIs. Parses the URI, dispatches to the
/// matching generator, and returns a boxed [`FrameSource`].
pub fn open_generate_frames(uri: &str) -> Result<Box<dyn FrameSource>> {
    let parsed = ParsedUri::parse(uri)?;
    match parsed.kind.as_str() {
        // Audio
        "synth" => {
            let buf = audio_synth::render(&parsed.query)?;
            Ok(Box::new(AudioFrameSource::new(buf)))
        }

        // Image basics — one static frame.
        "xc" => Ok(Box::new(SingleImageFrameSource::new(xc::render(
            &parsed.query,
        )?))),
        "gradient" => Ok(Box::new(SingleImageFrameSource::new(gradient::render(
            &parsed.query,
        )?))),
        "pattern" => Ok(Box::new(SingleImageFrameSource::new(pattern::render(
            &parsed.query,
        )?))),
        "grating" => Ok(Box::new(SingleImageFrameSource::new(grating::render(
            &parsed.query,
        )?))),
        #[cfg(feature = "label")]
        "label" => Ok(Box::new(SingleImageFrameSource::new(label::render(
            &parsed.query,
        )?))),
        #[cfg(not(feature = "label"))]
        "label" => Err(Error::Unsupported(
            "generate://label: oxideav-generator was built without the `label` feature".into(),
        )),

        // Procedural images — one static frame.
        "fractal" => Ok(Box::new(SingleImageFrameSource::new(fractal::render(
            &parsed.query,
        )?))),
        "plasma" => Ok(Box::new(SingleImageFrameSource::new(plasma::render(
            &parsed.query,
        )?))),
        "noise" => Ok(Box::new(SingleImageFrameSource::new(noise::render(
            &parsed.query,
        )?))),

        // Video — full frame sequences. Y4M is no longer in the loop;
        // frames flow straight into the pipeline as Frames-shape source
        // output.
        "testsrc" => Ok(Box::new(VideoFrameSourceImpl::new(testsrc::render(
            &parsed.query,
        )?))),
        "smptebars" => Ok(Box::new(VideoFrameSourceImpl::new(smptebars::render(
            &parsed.query,
        )?))),
        "fractal_zoom" => Ok(Box::new(VideoFrameSourceImpl::new(fractal_zoom::render(
            &parsed.query,
        )?))),
        "gradient_animate" => Ok(Box::new(VideoFrameSourceImpl::new(
            gradient_animate::render(&parsed.query)?,
        ))),
        "zoneplate" => Ok(Box::new(VideoFrameSourceImpl::new(zoneplate::render(
            &parsed.query,
        )?))),
        "scroll" => Ok(Box::new(VideoFrameSourceImpl::new(scroll::render(
            &parsed.query,
        )?))),
        "colorwheel" => Ok(Box::new(VideoFrameSourceImpl::new(colorwheel::render(
            &parsed.query,
        )?))),

        other => Err(Error::Unsupported(format!(
            "generate://{other}: unknown generator kind"
        ))),
    }
}

// ───────────────────────── Audio FrameSource ─────────────────────────

/// [`FrameSource`] for `generate://synth?...`. Emits one
/// [`AudioFrame`] containing the entire rendered buffer interleaved as
/// signed 16-bit little-endian PCM, then `Eof`.
struct AudioFrameSource {
    params: CodecParameters,
    /// `None` once the single frame has been emitted.
    pending: Option<AudioFrame>,
    duration_us: i64,
}

impl AudioFrameSource {
    fn new(buf: audio_synth::AudioBuffer) -> Self {
        let channels = buf.channels.max(1);
        let sample_rate = buf.sample_rate.max(1);
        let samples_per_channel = (buf.samples.len() / channels as usize) as u32;

        // Convert f32 → interleaved S16 LE bytes.
        let bytes = audio_buffer_to_s16le_bytes(&buf);

        // CodecParameters: pcm_s16le, populated channels / sample_rate /
        // sample_format / channel_layout.
        let mut params = CodecParameters::audio(CodecId::new("pcm_s16le"))
            .channels(channels)
            .channel_layout(ChannelLayout::from_count(channels));
        params.sample_rate = Some(sample_rate);
        params.sample_format = Some(SampleFormat::S16);

        // Duration in microseconds = samples / sample_rate * 1e6.
        let duration_us = ((samples_per_channel as i64) * 1_000_000) / (sample_rate as i64).max(1);

        let frame = AudioFrame {
            samples: samples_per_channel,
            pts: Some(0),
            data: vec![bytes],
        };

        Self {
            params,
            pending: Some(frame),
            duration_us,
        }
    }
}

impl FrameSource for AudioFrameSource {
    fn params(&self) -> &CodecParameters {
        &self.params
    }
    fn next_frame(&mut self) -> Result<Frame> {
        match self.pending.take() {
            Some(f) => Ok(Frame::Audio(f)),
            None => Err(Error::Eof),
        }
    }
    fn duration_micros(&self) -> Option<i64> {
        Some(self.duration_us)
    }
}

fn audio_buffer_to_s16le_bytes(buf: &audio_synth::AudioBuffer) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.samples.len() * 2);
    for &x in &buf.samples {
        let s = crate::audio::f32_sample_to_i16(x);
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

// ───────────────────────── Single-image FrameSource ─────────────────────────

/// [`FrameSource`] for the static image generators (`xc`, `gradient`,
/// `pattern`, `fractal`, `plasma`, `noise`). Emits exactly one
/// [`VideoFrame`] then `Eof`. Frame rate is set to 1/1 — these are
/// still images.
struct SingleImageFrameSource {
    params: CodecParameters,
    pending: Option<VideoFrame>,
}

impl SingleImageFrameSource {
    fn new(img: Rgba8Image) -> Self {
        let mut params = CodecParameters::video(CodecId::new("rawvideo"));
        params.width = Some(img.width);
        params.height = Some(img.height);
        params.pixel_format = Some(PixelFormat::Rgba);
        params.frame_rate = Some(Rational::new(1, 1));
        let frame = rgba_image_to_video_frame(img, 0);
        Self {
            params,
            pending: Some(frame),
        }
    }
}

impl FrameSource for SingleImageFrameSource {
    fn params(&self) -> &CodecParameters {
        &self.params
    }
    fn next_frame(&mut self) -> Result<Frame> {
        match self.pending.take() {
            Some(f) => Ok(Frame::Video(f)),
            None => Err(Error::Eof),
        }
    }
    fn duration_micros(&self) -> Option<i64> {
        // One frame at 1 fps → 1 s.
        Some(1_000_000)
    }
}

// ───────────────────────── Multi-frame video FrameSource ─────────────────────────

/// [`FrameSource`] for `testsrc` / `smptebars` / `fractal_zoom` /
/// `gradient_animate`. Drains a precomputed [`FrameSeq`] one frame per
/// `next_frame()` call.
struct VideoFrameSourceImpl {
    params: CodecParameters,
    /// Iterator state: pop from the front of a pre-built deque-style
    /// vector. We use `into_iter` on construction and store the iterator
    /// behind a `std::vec::IntoIter` so we can take ownership without
    /// re-allocating per frame.
    frames: std::vec::IntoIter<Rgba8Image>,
    pts_index: i64,
    duration_us: i64,
}

impl VideoFrameSourceImpl {
    fn new(seq: FrameSeq) -> Self {
        let fps = seq.fps.max(1);
        let (w, h) = seq
            .frames
            .first()
            .map(|f| (f.width, f.height))
            .unwrap_or((0, 0));
        let n = seq.frames.len() as i64;

        let mut params = CodecParameters::video(CodecId::new("rawvideo"));
        params.width = Some(w);
        params.height = Some(h);
        params.pixel_format = Some(PixelFormat::Rgba);
        params.frame_rate = Some(Rational::new(fps as i64, 1));

        let duration_us = if fps > 0 {
            (n * 1_000_000) / fps as i64
        } else {
            0
        };

        Self {
            params,
            frames: seq.frames.into_iter(),
            pts_index: 0,
            duration_us,
        }
    }
}

impl FrameSource for VideoFrameSourceImpl {
    fn params(&self) -> &CodecParameters {
        &self.params
    }
    fn next_frame(&mut self) -> Result<Frame> {
        match self.frames.next() {
            Some(img) => {
                let pts = self.pts_index;
                self.pts_index += 1;
                Ok(Frame::Video(rgba_image_to_video_frame(img, pts)))
            }
            None => Err(Error::Eof),
        }
    }
    fn duration_micros(&self) -> Option<i64> {
        Some(self.duration_us)
    }
}

// Shared helper — build a VideoFrame from an Rgba8Image at the given pts.
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

// ───────────────────────── URI parser ─────────────────────────

/// Parsed `generate://` URI.
///
/// `kind` is the path component (e.g. `synth`, `xc`, `gradient`); `query`
/// is the percent-decoded `key=value` map from the query string.
#[derive(Debug, Clone)]
pub struct ParsedUri {
    pub kind: String,
    pub query: BTreeMap<String, String>,
}

impl ParsedUri {
    pub fn parse(uri: &str) -> Result<Self> {
        // Strip the `generate://` scheme. Accept both `generate://synth?...`
        // (canonical) and the bare `synth?...` shape that `SourceRegistry`
        // hands us after stripping the scheme prefix.
        let body = uri
            .strip_prefix("generate://")
            .or_else(|| uri.strip_prefix("generate:"))
            .unwrap_or(uri);

        let (kind, query_str) = match body.split_once('?') {
            Some((k, q)) => (k, q),
            None => (body, ""),
        };
        if kind.is_empty() {
            return Err(Error::invalid(
                "generate://: missing generator kind (e.g. generate://synth?…)",
            ));
        }
        let query = parse_query(query_str)?;
        Ok(Self {
            kind: kind.to_string(),
            query,
        })
    }
}

/// Parse `k1=v1&k2=v2&…` into a map. Values are percent-decoded.
fn parse_query(s: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    if s.is_empty() {
        return Ok(out);
    }
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some(kv) => kv,
            None => (pair, ""),
        };
        out.insert(percent_decode(k)?, percent_decode(v)?);
    }
    Ok(out)
}

/// Minimal RFC 3986 percent-decoder. Accepts `+` as space (form-encoding
/// convention; harmless for our query keys/values).
fn percent_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_nibble(bytes[i + 1])?;
                let lo = hex_nibble(bytes[i + 2])?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out)
        .map_err(|e| Error::invalid(format!("percent-decoded value is not UTF-8: {e}")))
}

fn hex_nibble(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(Error::invalid(format!(
            "invalid percent-escape hex byte 0x{c:02x}"
        ))),
    }
}

/// Convenience: `query.get("k")` parsed as a `f64`, or `default`.
pub fn q_f64(q: &BTreeMap<String, String>, key: &str, default: f64) -> Result<f64> {
    match q.get(key) {
        None => Ok(default),
        Some(s) => s.parse::<f64>().map_err(|_| {
            Error::invalid(format!(
                "query parameter `{key}` must be a number, got {s:?}"
            ))
        }),
    }
}

/// Convenience: `query.get("k")` parsed as a `u32`, or `default`.
pub fn q_u32(q: &BTreeMap<String, String>, key: &str, default: u32) -> Result<u32> {
    match q.get(key) {
        None => Ok(default),
        Some(s) => s.parse::<u32>().map_err(|_| {
            Error::invalid(format!(
                "query parameter `{key}` must be a non-negative integer, got {s:?}"
            ))
        }),
    }
}

/// Convenience: `query.get("k")` parsed as an `i32` (sign allowed),
/// or `default`.
pub fn q_i32(q: &BTreeMap<String, String>, key: &str, default: i32) -> Result<i32> {
    match q.get(key) {
        None => Ok(default),
        Some(s) => s.parse::<i32>().map_err(|_| {
            Error::invalid(format!(
                "query parameter `{key}` must be an integer, got {s:?}"
            ))
        }),
    }
}

/// Convenience: `query.get("k")` as a `&str`, or `default`.
pub fn q_str<'a>(q: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> &'a str {
    q.get(key).map(|s| s.as_str()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::MediaType;

    #[test]
    fn parse_simple() {
        let p = ParsedUri::parse("generate://synth?type=sine&freq=440&duration=5").unwrap();
        assert_eq!(p.kind, "synth");
        assert_eq!(p.query.get("type").unwrap(), "sine");
        assert_eq!(p.query.get("freq").unwrap(), "440");
        assert_eq!(p.query.get("duration").unwrap(), "5");
    }

    #[test]
    fn parse_no_query() {
        let p = ParsedUri::parse("generate://plasma").unwrap();
        assert_eq!(p.kind, "plasma");
        assert!(p.query.is_empty());
    }

    #[test]
    fn parse_percent_decoded_color() {
        let p = ParsedUri::parse("generate://xc?color=%23ff0000").unwrap();
        assert_eq!(p.query.get("color").unwrap(), "#ff0000");
    }

    #[test]
    fn parse_missing_kind_errors() {
        assert!(ParsedUri::parse("generate://").is_err());
    }

    #[test]
    fn unknown_kind_errors() {
        let err = match open_generate_frames("generate://nonsense") {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        let msg = format!("{err}");
        assert!(msg.contains("nonsense"), "msg = {msg:?}");
    }

    #[test]
    fn audio_synth_produces_one_audio_frame_then_eof() {
        // 8000 Hz × 0.01 s = 80 samples mono.
        let mut src =
            open_generate_frames("generate://synth?type=sine&freq=440&duration=0.01&rate=8000")
                .unwrap();
        // Params: pcm_s16le, 1 channel, 8000 Hz, S16.
        let p = src.params();
        assert_eq!(p.media_type, MediaType::Audio);
        assert_eq!(p.codec_id.as_str(), "pcm_s16le");
        assert_eq!(p.sample_rate, Some(8000));
        assert_eq!(p.channels, Some(1));
        assert_eq!(p.sample_format, Some(SampleFormat::S16));

        let frame = src.next_frame().unwrap();
        match frame {
            Frame::Audio(a) => {
                assert_eq!(a.samples, 80);
                assert_eq!(a.data.len(), 1);
                assert_eq!(a.data[0].len(), 80 * 2);
            }
            other => panic!("expected audio, got {other:?}"),
        }
        assert!(matches!(src.next_frame(), Err(Error::Eof)));
    }

    #[test]
    fn image_xc_produces_one_video_frame_then_eof() {
        let mut src = open_generate_frames("generate://xc?color=red&w=4&h=4").unwrap();
        let p = src.params();
        assert_eq!(p.media_type, MediaType::Video);
        assert_eq!(p.width, Some(4));
        assert_eq!(p.height, Some(4));
        assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));

        let frame = src.next_frame().unwrap();
        match frame {
            Frame::Video(v) => {
                assert_eq!(v.planes.len(), 1);
                assert_eq!(v.planes[0].stride, 16);
                assert_eq!(v.planes[0].data.len(), 64);
                // First pixel red.
                assert_eq!(&v.planes[0].data[0..4], &[255, 0, 0, 255]);
            }
            other => panic!("expected video, got {other:?}"),
        }
        assert!(matches!(src.next_frame(), Err(Error::Eof)));
    }

    #[test]
    fn video_testsrc_produces_n_frames_then_eof() {
        // 0.2 s × 10 fps = 2 frames, 32×16.
        let mut src =
            open_generate_frames("generate://testsrc?w=32&h=16&duration=0.2&fps=10").unwrap();
        let p = src.params();
        assert_eq!(p.width, Some(32));
        assert_eq!(p.height, Some(16));
        assert_eq!(p.pixel_format, Some(PixelFormat::Rgba));
        assert_eq!(p.frame_rate, Some(Rational::new(10, 1)));

        let mut count = 0;
        let mut last_pts = -1;
        loop {
            match src.next_frame() {
                Ok(Frame::Video(v)) => {
                    let pts = v.pts.unwrap();
                    assert!(pts > last_pts, "monotonic pts");
                    last_pts = pts;
                    assert_eq!(v.planes[0].stride, 32 * 4);
                    assert_eq!(v.planes[0].data.len(), 32 * 16 * 4);
                    count += 1;
                }
                Err(Error::Eof) => break,
                other => panic!("unexpected: {other:?}"),
            }
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn video_smptebars_default_is_supported_via_uri() {
        // Smoke test: smptebars used to bail with Unsupported; should now
        // return a usable FrameSource even with default params.
        let mut src =
            open_generate_frames("generate://smptebars?w=8&h=8&duration=0.1&fps=10").unwrap();
        let p = src.params();
        assert_eq!(p.width, Some(8));
        assert_eq!(p.height, Some(8));
        // 0.1 s × 10 fps → 1 frame, then Eof.
        assert!(matches!(src.next_frame(), Ok(Frame::Video(_))));
        assert!(matches!(src.next_frame(), Err(Error::Eof)));
    }
}

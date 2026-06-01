//! Audio generators (synth + noise + silence).
//!
//! Generators emit a normalised f32 [`AudioBuffer`](synth::AudioBuffer)
//! that the URI / filter wrappers convert to interleaved 16-bit
//! little-endian PCM frames at the boundary. No container layer is
//! involved on the source path — frames flow straight to the pipeline.

pub mod synth;

/// Single-sample f32 → i16 with hard clipping at the i16 endpoints.
///
/// Maps `-1.0 → -32768`, `+1.0 → +32767` — the standard asymmetric
/// signed-PCM mapping used everywhere from WAV / AIFF on down.
/// Used by the URI [`FrameSource`](oxideav_core::FrameSource) wrapper
/// and by the zero-input filter wrapper to materialise PCM bytes from
/// the f32 mixing buffer.
#[inline]
pub fn f32_sample_to_i16(x: f32) -> i16 {
    let clipped = x.clamp(-1.0, 1.0);
    if clipped >= 0.0 {
        (clipped * 32767.0) as i16
    } else {
        (clipped * 32768.0) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::f32_sample_to_i16;

    #[test]
    fn clipping_extremes() {
        assert_eq!(f32_sample_to_i16(2.0), 32767);
        assert_eq!(f32_sample_to_i16(-2.0), -32768);
        assert_eq!(f32_sample_to_i16(0.0), 0);
    }
}

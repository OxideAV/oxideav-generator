//! Minimal canonical PCM WAV writer.
//!
//! Produces 16-bit little-endian integer PCM in a single `data` chunk
//! after the standard 44-byte RIFF/WAVE header. Mono or stereo,
//! arbitrary sample rate.
//!
//! Hand-rolled to avoid pulling in a `wav` crate — the format is well
//! known and tiny.

use oxideav_core::{Error, Result};

/// Encode `samples` (interleaved if `channels > 1`) as a canonical
/// 16-bit little-endian PCM WAV byte stream.
///
/// `samples` are normalised f32 in `[-1.0, 1.0]`; values outside that
/// range are clipped.
pub fn encode_pcm16(samples: &[i16], channels: u16, sample_rate: u32) -> Result<Vec<u8>> {
    if !(1..=2).contains(&channels) {
        return Err(Error::Unsupported(format!(
            "WAV writer: only mono/stereo supported, got {channels} channels"
        )));
    }
    let bytes_per_sample: u16 = 2;
    let byte_rate = sample_rate * (channels as u32) * (bytes_per_sample as u32);
    let block_align = channels * bytes_per_sample;
    let data_size = samples.len() * bytes_per_sample as usize;
    let riff_size = 36 + data_size; // 44-byte header minus the leading 8 RIFF bytes

    let mut out = Vec::with_capacity(44 + data_size);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff_size as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // fmt chunk — PCM (no extension), 16 bytes payload.
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // WAVE_FORMAT_PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&(bytes_per_sample * 8).to_le_bytes()); // bits-per-sample

    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_size as u32).to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    Ok(out)
}

/// Convert a normalised f32 sample buffer to clipped i16.
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|&x| f32_sample_to_i16(x)).collect()
}

/// Single-sample f32 → i16, with hard clipping.
#[inline]
pub fn f32_sample_to_i16(x: f32) -> i16 {
    let clipped = x.clamp(-1.0, 1.0);
    // Map -1.0 → -32768, +1.0 → +32767. Matches sox / aplay convention.
    if clipped >= 0.0 {
        (clipped * 32767.0) as i16
    } else {
        (clipped * 32768.0) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_shape_mono_8000() {
        let samples = vec![0i16; 100];
        let bytes = encode_pcm16(&samples, 1, 8000).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1); // channels
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 8000);
        assert_eq!(&bytes[36..40], b"data");
    }

    #[test]
    fn clipping_extremes() {
        assert_eq!(f32_sample_to_i16(2.0), 32767);
        assert_eq!(f32_sample_to_i16(-2.0), -32768);
        assert_eq!(f32_sample_to_i16(0.0), 0);
    }

    #[test]
    fn rejects_too_many_channels() {
        assert!(encode_pcm16(&[0i16; 4], 6, 48000).is_err());
    }
}

//! `generate://` URI scheme opener.
//!
//! Parses a query-string-shaped URI like
//! `generate://synth?type=sine&freq=440&duration=5`, dispatches to the
//! matching generator, and returns the resulting bytes wrapped in an
//! `std::io::Cursor` (which already implements
//! [`ReadSeek`](oxideav_core::ReadSeek)).

use std::collections::BTreeMap;
use std::io::Cursor;

use oxideav_core::{Error, ReadSeek, Result, SourceRegistry};

use crate::audio::synth as audio_synth;
use crate::image::{fractal, gradient, noise, pattern, plasma, xc};

/// Register the `generate` URI scheme.
pub fn register_source(registry: &mut SourceRegistry) {
    registry.register("generate", open_generate);
}

/// Opener for `generate://...` URIs.
pub fn open_generate(uri: &str) -> Result<Box<dyn ReadSeek>> {
    let parsed = ParsedUri::parse(uri)?;
    let bytes = match parsed.kind.as_str() {
        // Audio
        "synth" => audio_synth::generate(&parsed.query)?,

        // Image basics
        "xc" => xc::generate(&parsed.query)?,
        "gradient" => gradient::generate(&parsed.query)?,
        "pattern" => pattern::generate(&parsed.query)?,

        // Procedural images
        "fractal" => fractal::generate(&parsed.query)?,
        "plasma" => plasma::generate(&parsed.query)?,
        "noise" => noise::generate(&parsed.query)?,

        // Video — Y4M emission is implemented in `crate::video::*` but the
        // workspace doesn't yet ship a Y4M demuxer, so we fail loudly with
        // a helpful pointer to the filter API.
        "testsrc" | "smptebars" | "fractal_zoom" | "gradient_animate" => {
            return Err(Error::Unsupported(format!(
                "generate://{}: video sources via URI are not yet wired up \
                 (no Y4M demuxer in tree). Use the zero-input filter API \
                 (e.g. video.{}) inside a JSON pipeline instead.",
                parsed.kind, parsed.kind
            )));
        }

        other => {
            return Err(Error::Unsupported(format!(
                "generate://{other}: unknown generator kind"
            )));
        }
    };
    Ok(Box::new(Cursor::new(bytes)))
}

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

/// Convenience: `query.get("k")` as a `&str`, or `default`.
pub fn q_str<'a>(q: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> &'a str {
    q.get(key).map(|s| s.as_str()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let err = match open_generate("generate://nonsense") {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        let msg = format!("{err}");
        assert!(msg.contains("nonsense"), "msg = {msg:?}");
    }
}

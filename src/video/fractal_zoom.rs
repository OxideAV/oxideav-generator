//! Animated Mandelbrot / Julia zoom.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::FrameSeq;
use crate::image::fractal;
use crate::source::{q_f64, q_str, q_u32};

pub fn render(query: &BTreeMap<String, String>) -> Result<FrameSeq> {
    let w = q_u32(query, "w", 320)?.max(1);
    let h = q_u32(query, "h", 240)?.max(1);
    let duration_s = q_f64(query, "duration", 4.0)?.max(0.0);
    let fps = q_u32(query, "fps", 24)?.max(1);
    let kind = q_str(query, "type", "mandelbrot");
    let cx = q_f64(query, "cx", -0.7269)?;
    let cy = q_f64(query, "cy", 0.1889)?;
    let zoom_start = q_f64(query, "zoom_start", 1.0)?.max(1e-12);
    let zoom_rate = q_f64(query, "zoom_rate", 0.95)?;
    let iter = q_u32(query, "iter", 128)?.max(1);

    if !matches!(kind, "mandelbrot" | "julia") {
        return Err(Error::invalid(format!(
            "fractal_zoom: unknown type {kind:?} (expected mandelbrot|julia)"
        )));
    }

    let frame_count = ((duration_s * fps as f64).round() as usize).max(1);
    let mut frames = Vec::with_capacity(frame_count);
    let mut zoom = zoom_start;
    for _ in 0..frame_count {
        let mut q: BTreeMap<String, String> = BTreeMap::new();
        q.insert("type".into(), kind.into());
        q.insert("w".into(), w.to_string());
        q.insert("h".into(), h.to_string());
        q.insert("cx".into(), cx.to_string());
        q.insert("cy".into(), cy.to_string());
        q.insert("zoom".into(), zoom.to_string());
        q.insert("iter".into(), iter.to_string());
        let img = fractal::render(&q)?;
        frames.push(img);
        // Zoom rate < 1 zooms in; > 1 zooms out.
        zoom /= zoom_rate;
    }
    Ok(FrameSeq { frames, fps })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(items: &[(&str, &str)]) -> BTreeMap<String, String> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn fractal_zoom_advances_per_frame() {
        let seq = render(&map(&[
            ("w", "16"),
            ("h", "12"),
            ("duration", "0.2"),
            ("fps", "10"),
            ("iter", "16"),
            ("zoom_rate", "0.5"),
        ]))
        .unwrap();
        assert_eq!(seq.frames.len(), 2);
        assert_ne!(seq.frames[0].pixels, seq.frames[1].pixels);
    }

    #[test]
    fn fractal_zoom_unknown_type_errors() {
        assert!(render(&map(&[("type", "lorenz")])).is_err());
    }
}

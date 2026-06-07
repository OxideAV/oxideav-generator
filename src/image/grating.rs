//! Sinusoidal grating — a single-frequency cosine luma pattern.
//!
//! The grating is the canonical single-tone spatial-frequency probe
//! from Fourier image analysis: every pixel is set to
//! `0.5 + 0.5 · amplitude · cos(2π · (f_x · x + f_y · y) + phase)`,
//! where the spatial frequency vector `(f_x, f_y)` is derived from the
//! user-supplied total frequency `freq=` (cycles across the image
//! width) and direction `angle=` (degrees clockwise from the +x axis):
//! `f_x = freq · cos(θ) / w`, `f_y = freq · sin(θ) / w` (using the
//! image width as the unit so a horizontal grating with `freq=8`
//! contains exactly 8 full cycles end-to-end regardless of the image
//! height).
//!
//! Distinct from [`zoneplate`](crate::video::zoneplate) which sweeps
//! spatial frequency radially (`cos(k·r²)`) — the zone plate exercises
//! every frequency simultaneously, the grating isolates one
//! `(magnitude, direction)` pair. Both are pure cos-of-linear-phase
//! patterns; only the phase expression differs.
//!
//! The pattern lives on the same `Rgba8Image` row-major canvas as the
//! other image generators and emits one still frame through the URI
//! path or the `image.grating` filter.

use std::collections::BTreeMap;

use oxideav_core::Result;

use super::Rgba8Image;
use crate::source::{q_f64, q_u32};

/// Render a sinusoidal grating.
///
/// Recognised query parameters:
///
/// | Key         | Default | Meaning                                                     |
/// |-------------|---------|-------------------------------------------------------------|
/// | `w` / `h`   | 640/480 | Output resolution in pixels                                 |
/// | `freq`      | 8       | Cycles across the image width                               |
/// | `angle`     | 0       | Grating orientation in degrees clockwise from +x            |
/// | `phase`     | 0       | Phase offset in degrees (0 → bright stripe centred at x=0)  |
/// | `amplitude` | 1.0     | Modulation depth, clamped to `[0, 1]`                       |
///
/// `amplitude=0` collapses to a flat mid-grey (`v = 0.5`); `amplitude=1`
/// reaches both peak white and peak black. The luma byte is computed as
/// `round(255 · clamp(0.5 + 0.5 · amplitude · cos(φ), 0, 1))` with
/// `φ = 2π · (f_x · x + f_y · y) + phase_radians`. Output is RGBA8 with
/// `R = G = B = byte`, `A = 255` (matching the in-tree zone-plate
/// rendering convention).
pub fn render(query: &BTreeMap<String, String>) -> Result<Rgba8Image> {
    let w = q_u32(query, "w", 640)?.max(1);
    let h = q_u32(query, "h", 480)?.max(1);
    let freq = q_f64(query, "freq", 8.0)? as f32;
    let angle_deg = q_f64(query, "angle", 0.0)? as f32;
    let phase_deg = q_f64(query, "phase", 0.0)? as f32;
    let amplitude = q_f64(query, "amplitude", 1.0)?.clamp(0.0, 1.0) as f32;

    let theta = angle_deg.to_radians();
    let phase = phase_deg.to_radians();
    let inv_w = 1.0 / w as f32;
    // Snap near-zero cos/sin to 0 so the canonical orientations
    // (`angle=0` purely horizontal, `angle=90` purely vertical,
    // `angle=180` / `angle=270`) produce bit-exact axis-aligned
    // gratings without f32 round-off leakage on the orthogonal axis.
    // Anything more than 1e-6 in magnitude is left alone.
    let snap = |x: f32| if x.abs() < 1.0e-6 { 0.0 } else { x };
    let fx = freq * snap(theta.cos()) * inv_w;
    let fy = freq * snap(theta.sin()) * inv_w;
    let two_pi = std::f32::consts::TAU;

    let mut img = Rgba8Image::new(w, h);
    for y in 0..h {
        let yf = y as f32;
        for x in 0..w {
            let xf = x as f32;
            let phi = two_pi * (fx * xf + fy * yf) + phase;
            let v = 0.5 + 0.5 * amplitude * phi.cos();
            let byte = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            img.put(x, y, [byte, byte, byte, 255]);
        }
    }
    Ok(img)
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
    fn grating_default_dimensions_match_query() {
        let img = render(&map(&[("w", "64"), ("h", "32")])).unwrap();
        assert_eq!(img.width, 64);
        assert_eq!(img.height, 32);
        assert_eq!(img.pixels.len(), 64 * 32 * 4);
    }

    #[test]
    fn grating_amplitude_zero_is_flat_mid_grey() {
        // amplitude=0 collapses the cos term → v = 0.5 for every pixel
        // → byte = 128 (rounded). Same convention as the zone plate.
        let img = render(&map(&[("w", "8"), ("h", "8"), ("amplitude", "0")])).unwrap();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(img.get(x, y), [128, 128, 128, 255]);
            }
        }
    }

    #[test]
    fn grating_zero_freq_is_flat_white_at_phase_zero() {
        // freq=0 → φ = phase = 0 → cos(0) = 1 → v = 1.0 → byte = 255.
        let img = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("freq", "0"),
            ("phase", "0"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(img.get(x, y), [255, 255, 255, 255]);
            }
        }
    }

    #[test]
    fn grating_phase_180_inverts_zero_freq_to_black() {
        // freq=0, phase=180° → cos(π) = -1 → v = 0.0 → byte = 0.
        let img = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("freq", "0"),
            ("phase", "180"),
        ]))
        .unwrap();
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(img.get(x, y), [0, 0, 0, 255]);
            }
        }
    }

    #[test]
    fn grating_horizontal_constant_per_column() {
        // angle=0 → f_y = 0 → grating only varies in x; every column
        // must hold a single constant value across all rows.
        let img = render(&map(&[
            ("w", "32"),
            ("h", "16"),
            ("freq", "4"),
            ("angle", "0"),
        ]))
        .unwrap();
        for x in 0..img.width {
            let top = img.get(x, 0);
            for y in 1..img.height {
                assert_eq!(img.get(x, y), top, "row {y} differs at column {x}");
            }
        }
    }

    #[test]
    fn grating_vertical_constant_per_row() {
        // angle=90° → f_x = 0 → grating only varies in y; every row
        // must hold a single constant value across all columns.
        let img = render(&map(&[
            ("w", "32"),
            ("h", "16"),
            ("freq", "4"),
            ("angle", "90"),
        ]))
        .unwrap();
        for y in 0..img.height {
            let left = img.get(0, y);
            for x in 1..img.width {
                assert_eq!(img.get(x, y), left, "column {x} differs at row {y}");
            }
        }
    }

    #[test]
    fn grating_freq_one_sweeps_full_cycle_across_width() {
        // freq=1, angle=0, phase=0, amplitude=1 → cos(2π·x/w) goes from
        // cos(0)=1 at x=0 through cos(π)=-1 at x=w/2 back toward 1 near
        // x=w. Check the three landmarks on a 16-wide image:
        //   x=0  → byte = 255
        //   x=8  → byte = 0 (cos(π) = -1 exactly)
        //   x=16 wraps; we instead probe x=4 (cos(π/2)=0 → byte=128).
        let img = render(&map(&[
            ("w", "16"),
            ("h", "4"),
            ("freq", "1"),
            ("angle", "0"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0), [255, 255, 255, 255]);
        assert_eq!(img.get(8, 0), [0, 0, 0, 255]);
        // cos(π/2) ≈ 6.12e-17 in f32 → v ≈ 0.5; rounding may land on
        // 127 or 128 depending on the exact f32 cos approximation.
        let mid = img.get(4, 0)[0];
        assert!(mid == 127 || mid == 128, "mid byte at x=4 is {mid}");
    }

    #[test]
    fn grating_freq_two_has_two_white_peaks() {
        // freq=2, amplitude=1, w=16 → peaks at x=0 and x=8 (cos(0)=1
        // and cos(4π)=1). Each peak should hit byte=255 in row 0.
        let img = render(&map(&[
            ("w", "16"),
            ("h", "4"),
            ("freq", "2"),
            ("angle", "0"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        assert_eq!(img.get(0, 0)[0], 255);
        assert_eq!(img.get(8, 0)[0], 255);
        // Mid-trough between them at x=4 hits byte=0 (cos(2π·2·4/16) =
        // cos(π) = -1).
        assert_eq!(img.get(4, 0)[0], 0);
    }

    #[test]
    fn grating_diagonal_angle_breaks_row_column_symmetry() {
        // At angle=45°, both f_x and f_y are non-zero, so the constant-
        // per-row / constant-per-column properties no longer hold.
        let img = render(&map(&[
            ("w", "32"),
            ("h", "32"),
            ("freq", "4"),
            ("angle", "45"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        let p00 = img.get(0, 0);
        let p10 = img.get(10, 0);
        let p01 = img.get(0, 10);
        // Some column varies down a row → at least one of these differs.
        assert!(p10 != p00 || p01 != p00, "expected diagonal variation");
    }

    #[test]
    fn grating_amplitude_clamped_to_unit_interval() {
        // amplitude=2 should clamp to 1; the resulting image must match
        // amplitude=1 byte-for-byte.
        let a = render(&map(&[
            ("w", "16"),
            ("h", "8"),
            ("freq", "3"),
            ("amplitude", "2"),
        ]))
        .unwrap();
        let b = render(&map(&[
            ("w", "16"),
            ("h", "8"),
            ("freq", "3"),
            ("amplitude", "1"),
        ]))
        .unwrap();
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn grating_alpha_is_opaque() {
        let img = render(&map(&[("w", "4"), ("h", "4"), ("freq", "1")])).unwrap();
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(img.get(x, y)[3], 255, "alpha at ({x},{y}) not 255");
            }
        }
    }

    #[test]
    fn grating_phase_90_at_freq_zero_is_mid_grey() {
        // freq=0, phase=90° → cos(π/2) = 0 → v = 0.5 → byte = 128.
        let img = render(&map(&[
            ("w", "8"),
            ("h", "8"),
            ("freq", "0"),
            ("phase", "90"),
        ]))
        .unwrap();
        // Allow ±1 because cos(π/2) ≈ 0 but rounds to 6.12e-17 in
        // f32 → may land on 127 or 128 depending on rounding tie.
        for y in 0..8 {
            for x in 0..8 {
                let v = img.get(x, y)[0];
                assert!(
                    v == 127 || v == 128,
                    "byte at ({x},{y}) = {v}, expected 127 or 128"
                );
            }
        }
    }
}

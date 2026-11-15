//! Elementary functions built from `+ − × ÷` and exact bit operations
//! only, so their results are identical on every platform (the C
//! library's `ln` / `sin` / `cos` may differ in the last bit between
//! targets, which is enough to flip a rounding tie in a seeded noise
//! field).
//!
//! Accuracy is a few units in the last place — far below what any
//! sample depth resolves.

/// `1 / (2i + 1)` for `i = 0..=10`: the coefficients of the odd
/// series of `atanh`.
const ODD_RECIPROCALS: [f64; 11] = {
    let mut t = [0.0; 11];
    let mut i = 0;
    while i < 11 {
        t[i] = 1.0 / (2 * i + 1) as f64;
        i += 1;
    }
    t
};

/// Maclaurin coefficients of `cos` (`(−1)ⁱ / (2i)!`) and of
/// `sin x / x` (`(−1)ⁱ / (2i + 1)!`), `i = 0..=9`.
const COS_SIN_COEFFICIENTS: ([f64; 10], [f64; 10]) = {
    let mut c = [0.0; 10];
    let mut s = [0.0; 10];
    let mut factorial = 1.0; // (2i)!
    let mut sign = 1.0;
    let mut i = 0;
    while i < 10 {
        if i > 0 {
            factorial *= ((2 * i - 1) * (2 * i)) as f64;
        }
        c[i] = sign / factorial;
        s[i] = sign / (factorial * (2 * i + 1) as f64);
        sign = -sign;
        i += 1;
    }
    (c, s)
};

/// Natural logarithm of a positive, finite, normal `x`.
///
/// `x = m · 2ᵉ` with `m ∈ [√½, √2)`; then
/// `ln m = 2·atanh(z)`, `z = (m − 1) / (m + 1)`, `|z| < 0.1716`, by the
/// odd series `z + z³/3 + … + z²¹/21` (truncation below `10⁻¹⁷`).
#[inline]
pub(crate) fn ln(x: f64) -> f64 {
    debug_assert!(x > 0.0 && x.is_finite());
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7FF) as i64 - 1023;
    // Mantissa in [1, 2).
    let mut m = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | 0x3FF0_0000_0000_0000);
    if m >= std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut s = ODD_RECIPROCALS[10];
    for r in ODD_RECIPROCALS[..10].iter().rev() {
        s = r + z2 * s;
    }
    2.0 * z * s + e as f64 * std::f64::consts::LN_2
}

/// `(cos, sin)` of the angle `turns` full turns (`2π · turns` radians).
///
/// The angle is folded into the first octant (so the eight principal
/// directions are exact) and evaluated by the Maclaurin series of
/// `sin` and `cos` on `[0, π/4]`, to `x¹⁹` and `x¹⁸`.
#[inline]
pub(crate) fn cos_sin_turns(turns: f64) -> (f64, f64) {
    let t = turns - turns.floor(); // [0, 1)
    let eighths = t * 8.0;
    let octant = eighths.floor();
    let frac = eighths - octant; // [0, 1) of an octant
    let odd = (octant as u32) & 1 == 1;
    // Angle from the nearest axis below (even octant) or above (odd).
    let x = (if odd { 1.0 - frac } else { frac }) * std::f64::consts::FRAC_PI_4;
    let x2 = x * x;
    // Horner forms of the two series.
    let (cc, sc) = &COS_SIN_COEFFICIENTS;
    let mut c = cc[9];
    let mut s = sc[9];
    for i in (0..9).rev() {
        c = cc[i] + x2 * c;
        s = sc[i] + x2 * s;
    }
    let (near, far) = (c, x * s); // cos x, sin x with x ∈ [0, π/4]
    let (a, b) = if odd { (far, near) } else { (near, far) };
    // (a, b) = (cos, sin) of the angle within its quadrant.
    match (octant as u32) / 2 {
        0 => (a, b),
        1 => (-b, a),
        2 => (-a, -b),
        _ => (b, -a),
    }
}

/// Cosine of the angle `turns` full turns: the first component of
/// [`cos_sin_turns`], bit for bit, evaluating only the one series the
/// angle's octant needs.
#[inline]
pub(crate) fn cos_turns(turns: f64) -> f64 {
    let t = turns - turns.floor();
    let eighths = t * 8.0;
    let octant = eighths.floor();
    let frac = eighths - octant;
    let index = octant as u32;
    let odd = index & 1 == 1;
    let x = (if odd { 1.0 - frac } else { frac }) * std::f64::consts::FRAC_PI_4;
    let x2 = x * x;
    let quadrant = index / 2;
    // The cosine is ±cos x in the octants next to the x axis and
    // ±sin x in those next to the y axis.
    let wants_cos_series = (quadrant & 1 == 1) == odd;
    let (cc, sc) = &COS_SIN_COEFFICIENTS;
    let value = if wants_cos_series {
        let mut c = cc[9];
        for i in (0..9).rev() {
            c = cc[i] + x2 * c;
        }
        c
    } else {
        let mut s = sc[9];
        for i in (0..9).rev() {
            s = sc[i] + x2 * s;
        }
        x * s
    };
    if quadrant == 1 || quadrant == 2 {
        -value
    } else {
        value
    }
}

/// Square root by the hardware operation (IEEE 754 requires it to be
/// correctly rounded, so it is as portable as the four arithmetic
/// operations).
#[inline]
pub(crate) fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ln_tracks_the_library_function() {
        assert_eq!(ln(1.0), 0.0);
        let mut x: f64 = 1e-12;
        while x < 1e12 {
            let want = x.ln();
            let got = ln(x);
            assert!(
                (got - want).abs() <= 2e-15 * want.abs().max(1.0),
                "ln({x}): {got} vs {want}"
            );
            x *= 1.37;
        }
    }

    #[test]
    fn cos_sin_are_exact_on_the_axes_and_track_the_library_functions() {
        assert_eq!(cos_sin_turns(0.0), (1.0, 0.0));
        assert_eq!(cos_sin_turns(0.25), (0.0, 1.0));
        assert_eq!(cos_sin_turns(0.5), (-1.0, 0.0));
        assert_eq!(cos_sin_turns(0.75), (0.0, -1.0));
        let (c, s) = cos_sin_turns(0.125);
        assert!((c - s).abs() < 1e-15);
        for i in 0..4000 {
            let t = f64::from(i) / 4000.0 * 3.0 - 1.0;
            let (c, s) = cos_sin_turns(t);
            assert_eq!(cos_turns(t).to_bits(), c.to_bits(), "cos_turns({t})");
            let a = t * std::f64::consts::TAU;
            assert!((c - a.cos()).abs() < 3e-15, "cos {t}");
            assert!((s - a.sin()).abs() < 3e-15, "sin {t}");
        }
    }
}

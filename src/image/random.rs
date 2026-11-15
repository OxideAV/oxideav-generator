//! Seeded random fields — independent noise at every pixel, drawn from
//! a chosen distribution around a base colour.
//!
//! Every sample is a pure function of `(seed, x, y, channel)`: the
//! uniform variates come from the stateless counter hash
//! [`crate::video::snow::mix`], and the logarithm / cosine the shaped
//! distributions need are evaluated with basic arithmetic only, so a
//! given seed produces the same field on every platform, in any row
//! order and under any thread budget.
//!
//! With `u, u₁, u₂` uniform in `(0, 1)`, `n` standard normal
//! (`n = √(−2 ln u₁) · cos 2πu₂`), `c` the base colour component and
//! `k` the `amount`:
//!
//! | distribution     | sample                                              | default `k` |
//! |------------------|-----------------------------------------------------|-------------|
//! | `random`         | `u` (the base colour is ignored)                    | —           |
//! | `uniform`        | `c + k·(u − ½)` — width `k`, variance `k²/12`       | 0.25        |
//! | `gaussian`       | `c + k·n` — standard deviation `k`                  | 0.1         |
//! | `impulse`        | with probability `k`: 0 or 1 (equal odds); else `c` | 0.1         |
//! | `laplacian`      | `c − k·sgn(u − ½)·ln(1 − 2·abs(u − ½))` — scale `k`, variance `2k²` | 0.07 |
//! | `poisson`        | `N / k`, `N` Poisson with mean `k·c` — `k` counts at white | 64   |
//! | `multiplicative` | `c·(1 + k·n)` — relative standard deviation `k`     | 0.1         |
//!
//! Colour channels are independent unless `mono` is set (one variate
//! shared by R, G and B); alpha is the base colour's. Float output is
//! not clamped — a Gaussian field keeps its tails; integer depths clamp
//! to the representable range.
//!
//! The normal variates come in pairs — `√(−2 ln u₁)·cos 2πu₂` and the
//! same radius times `sin 2πu₂` are independent — so red and green
//! share one pair and blue takes the first half of a second.
//!
//! A Poisson count with mean up to 48 is drawn exactly by inverting
//! the cumulative distribution (tabulated once per channel, since the
//! mean is the same at every pixel) on a 53-bit uniform; above that the
//! count is `max(0, round(mean + √mean · n))`, whose error is far below
//! one count in the first two moments.

use std::collections::BTreeMap;

use oxideav_core::{Error, Result};

use super::canvas::{self, Field, Image, RenderConfig};
use super::detmath;
use super::palette::parse_color;
use crate::source::{q_f64, q_str, q_u32};
use crate::video::snow::mix;

/// Distribution of a [`RandomOptions`] field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum NoiseDistribution {
    /// Uniform over the whole range, base colour ignored.
    Random,
    /// Uniform, centred on the base colour.
    Uniform,
    /// Normal, centred on the base colour.
    #[default]
    Gaussian,
    /// Salt and pepper.
    Impulse,
    /// Double exponential, centred on the base colour.
    Laplacian,
    /// Photon-count noise with the base colour as mean.
    Poisson,
    /// Normal noise proportional to the base colour (speckle).
    Multiplicative,
}

impl NoiseDistribution {
    /// Case-insensitive name: `random`, `uniform`, `gaussian`,
    /// `impulse`, `laplacian`, `poisson`, `multiplicative` (plus
    /// `normal`, `salt-pepper`, `laplace`, `shot`, `speckle`).
    pub fn parse(s: &str) -> Option<Self> {
        let k: String = s
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Some(match k.as_str() {
            "random" | "white" => Self::Random,
            "uniform" => Self::Uniform,
            "gaussian" | "gauss" | "normal" => Self::Gaussian,
            "impulse" | "saltpepper" | "saltandpepper" => Self::Impulse,
            "laplacian" | "laplace" => Self::Laplacian,
            "poisson" | "shot" => Self::Poisson,
            "multiplicative" | "multiplicativegaussian" | "speckle" => Self::Multiplicative,
            _ => return None,
        })
    }

    /// Canonical lower-case name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::Uniform => "uniform",
            Self::Gaussian => "gaussian",
            Self::Impulse => "impulse",
            Self::Laplacian => "laplacian",
            Self::Poisson => "poisson",
            Self::Multiplicative => "multiplicative",
        }
    }

    /// Default `amount` (see the module table).
    pub fn default_amount(self) -> f64 {
        match self {
            Self::Random => 1.0,
            Self::Uniform => 0.25,
            Self::Gaussian | Self::Impulse | Self::Multiplicative => 0.1,
            Self::Laplacian => 0.07,
            Self::Poisson => 64.0,
        }
    }
}

/// A seeded random field. Build with [`RandomOptions::new`] and the
/// `with_*` setters, or from a `generate://random?…` query.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct RandomOptions {
    pub width: u32,
    pub height: u32,
    pub seed: u32,
    pub distribution: NoiseDistribution,
    /// Base colour, straight-alpha sRGB-encoded, unit range.
    pub base: [f64; 4],
    /// Strength parameter `k`; `None` uses the distribution's default.
    pub amount: Option<f64>,
    /// One variate for R, G and B (grey noise on a grey base).
    pub mono: bool,
}

impl RandomOptions {
    /// Gaussian noise (σ = 0.1) on opaque mid-grey, seed 0.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            seed: 0,
            distribution: NoiseDistribution::Gaussian,
            base: [0.5, 0.5, 0.5, 1.0],
            amount: None,
            mono: false,
        }
    }

    pub fn with_seed(mut self, seed: u32) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_distribution(mut self, distribution: NoiseDistribution) -> Self {
        self.distribution = distribution;
        self
    }

    pub fn with_base(mut self, base: [f64; 4]) -> Self {
        self.base = base;
        self
    }

    pub fn with_amount(mut self, amount: f64) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn with_mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    /// `w`, `h`, `seed`, `type` (distribution), `color` (base),
    /// `amount`, `mono=1`.
    pub fn from_query(query: &BTreeMap<String, String>) -> Result<Self> {
        let mut o = Self::new(q_u32(query, "w", 640)?, q_u32(query, "h", 480)?);
        o.seed = q_u32(query, "seed", 0)?;
        let t = q_str(query, "type", "gaussian");
        o.distribution = NoiseDistribution::parse(t).ok_or_else(|| {
            Error::invalid(format!(
                "random: unknown type {t:?} (expected random|uniform|gaussian|impulse|laplacian|poisson|multiplicative)"
            ))
        })?;
        if let Some(c) = query.get("color") {
            o.base = parse_color(c)?.map(|v| f64::from(v) / 255.0);
        }
        if query.contains_key("amount") {
            let a = q_f64(query, "amount", 0.0)?;
            if !a.is_finite() || a < 0.0 {
                return Err(Error::invalid(format!(
                    "random: amount must be a non-negative number, got {a}"
                )));
            }
            o.amount = Some(a);
        }
        o.mono = matches!(q_str(query, "mono", "0"), "1" | "true" | "yes" | "on");
        Ok(o)
    }

    /// The field, ready for [`canvas::render`].
    pub fn field(&self) -> RandomField {
        RandomField::new(*self)
    }
}

/// [`RandomOptions`] as a [`Field`].
#[derive(Clone, Debug)]
pub struct RandomField {
    opts: RandomOptions,
    amount: f64,
    /// Per-channel Poisson cumulative distribution (`cdf[c][n]` =
    /// `P(N ≤ n)`), empty when the channel uses the normal branch or
    /// the distribution is not Poisson.
    poisson_cdf: [Vec<f64>; 3],
}

/// Largest Poisson mean drawn by exact inversion.
const POISSON_EXACT_MEAN: f64 = 48.0;

/// Uniform in `(0, 1)`: draw `index` of stream `stream` at pixel
/// `(x, y)`.
#[inline]
fn uniform(seed: u32, stream: u32, index: u32, x: u32, y: u32) -> f64 {
    let word = mix(seed, 0xA24B_AED5 ^ (stream + 4 * index), x, y);
    (f64::from(word) + 0.5) / 4_294_967_296.0
}

/// Uniform in `(0, 1)` with 53 random bits, from draws 0 and 1 of the
/// stream.
#[inline]
fn uniform53(seed: u32, stream: u32, x: u32, y: u32) -> f64 {
    let hi = u64::from(mix(seed, 0xA24B_AED5 ^ stream, x, y));
    let lo = u64::from(mix(seed, 0xA24B_AED5 ^ (stream + 4), x, y));
    (((hi << 21) | (lo >> 11)) as f64 + 0.5) / 9_007_199_254_740_992.0
}

/// Two independent standard normals from draws 0 and 1 of the stream.
#[inline]
fn normal_pair(seed: u32, stream: u32, x: u32, y: u32) -> (f64, f64) {
    let u1 = uniform(seed, stream, 0, x, y);
    let u2 = uniform(seed, stream, 1, x, y);
    let radius = detmath::sqrt(-2.0 * detmath::ln(u1));
    let (cos, sin) = detmath::cos_sin_turns(u2);
    (radius * cos, radius * sin)
}

/// `e^(−m)` for `m ∈ [0, 48]`: `(e^(−m/64))⁶⁴` with the inner factor
/// from the Maclaurin series (argument at most ¾) and six squarings.
fn exp_neg(m: f64) -> f64 {
    let x = -m / 64.0;
    let mut s = 1.0;
    let mut k = 18.0;
    while k >= 1.0 {
        s = 1.0 + x / k * s;
        k -= 1.0;
    }
    let mut p = s;
    for _ in 0..6 {
        p *= p;
    }
    p
}

/// `P(N ≤ n)` for a Poisson variable of the given mean, `n = 0, 1, …`
/// until the remaining tail is below the resolution of an `f64`.
fn poisson_cdf(mean: f64) -> Vec<f64> {
    let mut p = exp_neg(mean);
    let mut cdf = p;
    let mut table = vec![cdf];
    let mut n = 0.0;
    while table.len() < 512 {
        n += 1.0;
        p *= mean / n;
        let next = cdf + p;
        if next == cdf && n > mean {
            break;
        }
        cdf = next;
        table.push(cdf);
    }
    table
}

impl RandomField {
    fn new(opts: RandomOptions) -> Self {
        let amount = opts
            .amount
            .unwrap_or_else(|| opts.distribution.default_amount());
        let mut poisson = [Vec::new(), Vec::new(), Vec::new()];
        if opts.distribution == NoiseDistribution::Poisson && amount > 0.0 {
            for (c, table) in poisson.iter_mut().enumerate() {
                let mean = amount * opts.base[c].max(0.0);
                if mean <= POISSON_EXACT_MEAN {
                    *table = poisson_cdf(mean);
                }
            }
        }
        Self {
            opts,
            amount,
            poisson_cdf: poisson,
        }
    }

    /// The three colour samples of pixel `(x, y)`.
    #[inline]
    fn pixel(&self, x: u32, y: u32) -> [f64; 3] {
        let seed = self.opts.seed;
        let k = self.amount;
        let b = self.opts.base;
        // Stream of channel `c`: one shared stream when mono.
        let stream = |c: usize| if self.opts.mono { 0 } else { c as u32 };
        let per_channel =
            |f: &dyn Fn(usize, u32) -> f64| [f(0, stream(0)), f(1, stream(1)), f(2, stream(2))];
        match self.opts.distribution {
            NoiseDistribution::Random => per_channel(&|_, s| uniform(seed, s, 0, x, y)),
            NoiseDistribution::Uniform => {
                per_channel(&|c, s| b[c] + k * (uniform(seed, s, 0, x, y) - 0.5))
            }
            NoiseDistribution::Gaussian | NoiseDistribution::Multiplicative => {
                let (n0, n1) = normal_pair(seed, 0, x, y);
                let n = if self.opts.mono {
                    [n0, n0, n0]
                } else {
                    [n0, n1, normal_pair(seed, 1, x, y).0]
                };
                if self.opts.distribution == NoiseDistribution::Gaussian {
                    [b[0] + k * n[0], b[1] + k * n[1], b[2] + k * n[2]]
                } else {
                    [
                        b[0] * (1.0 + k * n[0]),
                        b[1] * (1.0 + k * n[1]),
                        b[2] * (1.0 + k * n[2]),
                    ]
                }
            }
            NoiseDistribution::Impulse => per_channel(&|c, s| {
                let u = uniform(seed, s, 0, x, y);
                if u < k / 2.0 {
                    0.0
                } else if u < k {
                    1.0
                } else {
                    b[c]
                }
            }),
            NoiseDistribution::Laplacian => per_channel(&|c, s| {
                let d = uniform(seed, s, 0, x, y) - 0.5;
                let magnitude = -detmath::ln(1.0 - 2.0 * d.abs());
                if d < 0.0 {
                    b[c] - k * magnitude
                } else {
                    b[c] + k * magnitude
                }
            }),
            NoiseDistribution::Poisson => per_channel(&|c, s| {
                if k <= 0.0 {
                    return b[c];
                }
                let table = &self.poisson_cdf[c];
                let count = if table.is_empty() {
                    let mean = k * b[c].max(0.0);
                    (mean + detmath::sqrt(mean) * normal_pair(seed, s, x, y).0)
                        .round()
                        .max(0.0)
                } else {
                    // The count is the number of table entries below u.
                    let u = uniform53(seed, s, x, y);
                    table.partition_point(|p| *p < u) as f64
                };
                count / k
            }),
        }
    }
}

impl Field for RandomField {
    fn width(&self) -> u32 {
        self.opts.width
    }

    fn height(&self) -> u32 {
        self.opts.height
    }

    fn fill_row(&self, y: u32, x0: u32, out: &mut [[f64; 4]]) {
        let alpha = self.opts.base[3];
        for (i, o) in out.iter_mut().enumerate() {
            let [r, g, b] = self.pixel(x0 + i as u32, y);
            *o = [r, g, b, alpha];
        }
    }
}

/// `generate://random?…` entry point (output format from the query).
pub fn render_image(query: &BTreeMap<String, String>) -> Result<Image> {
    let opts = RandomOptions::from_query(query)?;
    Ok(canvas::render(
        &opts.field(),
        &RenderConfig::from_query(query)?,
    ))
}

/// `generate://random?…` as 8-bit RGBA.
pub fn render(query: &BTreeMap<String, String>) -> Result<super::Rgba8Image> {
    let opts = RandomOptions::from_query(query)?;
    Ok(canvas::render(&opts.field(), &RenderConfig::new()).into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::canvas::{Channels, SampleDepth};

    /// Mean and variance of the red channel of a float render.
    fn moments(opts: RandomOptions) -> (f64, f64) {
        let img = canvas::render(
            &opts.field(),
            &RenderConfig::new()
                .with_depth(SampleDepth::F32)
                .with_channels(Channels::Rgb),
        );
        let v: Vec<f64> = img
            .as_f32()
            .unwrap()
            .chunks(3)
            .map(|p| f64::from(p[0]))
            .collect();
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / v.len() as f64;
        (mean, var)
    }

    fn opts(d: NoiseDistribution) -> RandomOptions {
        RandomOptions::new(256, 256)
            .with_seed(7)
            .with_distribution(d)
    }

    #[test]
    fn every_distribution_has_its_documented_moments() {
        // 65 536 samples: the mean of a variance-v field is within
        // 5·√(v / 65536) ≈ 0.02·√v of its expectation.
        let near = |got: f64, want: f64, tol: f64, what: &str| {
            assert!((got - want).abs() < tol, "{what}: {got} vs {want}");
        };
        let (m, v) = moments(opts(NoiseDistribution::Random));
        near(m, 0.5, 0.006, "random mean");
        near(v, 1.0 / 12.0, 0.003, "random variance");
        let (m, v) = moments(opts(NoiseDistribution::Uniform));
        near(m, 0.5, 0.002, "uniform mean");
        near(v, 0.25 * 0.25 / 12.0, 0.0003, "uniform variance");
        let (m, v) = moments(opts(NoiseDistribution::Gaussian));
        near(m, 0.5, 0.002, "gaussian mean");
        near(v, 0.01, 0.0005, "gaussian variance");
        let (m, v) = moments(opts(NoiseDistribution::Laplacian));
        near(m, 0.5, 0.002, "laplacian mean");
        near(v, 2.0 * 0.07 * 0.07, 0.0008, "laplacian variance");
        let (m, v) =
            moments(opts(NoiseDistribution::Multiplicative).with_base([0.8, 0.8, 0.8, 1.0]));
        near(m, 0.8, 0.002, "multiplicative mean");
        near(v, 0.08 * 0.08, 0.0004, "multiplicative variance");
        // Poisson: mean c, variance c / k.
        let (m, v) = moments(opts(NoiseDistribution::Poisson));
        near(m, 0.5, 0.003, "poisson mean");
        near(v, 0.5 / 64.0, 0.0005, "poisson variance");
        let (m, v) = moments(opts(NoiseDistribution::Poisson).with_amount(400.0));
        near(m, 0.5, 0.002, "poisson (normal branch) mean");
        near(v, 0.5 / 400.0, 0.0001, "poisson (normal branch) variance");
    }

    #[test]
    fn impulse_noise_hits_the_requested_fraction() {
        let img = canvas::render(
            &opts(NoiseDistribution::Impulse)
                .with_amount(0.2)
                .with_mono(true)
                .field(),
            &RenderConfig::new().with_channels(Channels::Gray),
        );
        let v = img.as_u8().unwrap();
        let salt = v.iter().filter(|s| **s == 255).count() as f64 / v.len() as f64;
        let pepper = v.iter().filter(|s| **s == 0).count() as f64 / v.len() as f64;
        assert!(v.iter().all(|s| matches!(*s, 0 | 128 | 255)));
        assert!((salt - 0.1).abs() < 0.008, "salt {salt}");
        assert!((pepper - 0.1).abs() < 0.008, "pepper {pepper}");
    }

    #[test]
    fn poisson_samples_are_whole_counts() {
        let img = canvas::render(
            &opts(NoiseDistribution::Poisson).with_amount(10.0).field(),
            &RenderConfig::new().with_depth(SampleDepth::F32),
        );
        for v in img.as_f32().unwrap().chunks(4) {
            let count = f64::from(v[0]) * 10.0;
            assert!((count - count.round()).abs() < 1e-5);
            assert_eq!(v[3], 1.0);
        }
        // A black base stays black; zero strength leaves the base.
        let black = opts(NoiseDistribution::Poisson).with_base([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(moments(black), (0.0, 0.0));
    }

    #[test]
    fn seed_decides_everything() {
        for d in [
            NoiseDistribution::Random,
            NoiseDistribution::Uniform,
            NoiseDistribution::Gaussian,
            NoiseDistribution::Impulse,
            NoiseDistribution::Laplacian,
            NoiseDistribution::Poisson,
            NoiseDistribution::Multiplicative,
        ] {
            let cfg = RenderConfig::new().with_depth(SampleDepth::U16);
            let a = canvas::render(&opts(d).field(), &cfg);
            let b = canvas::render(&opts(d).field(), &cfg.with_threads(5));
            assert_eq!(a, b, "{d:?}: thread budget changed the field");
            let c = canvas::render(&opts(d).with_seed(8).field(), &cfg);
            assert_ne!(a, c, "{d:?}: seed had no effect");
            assert_eq!(NoiseDistribution::parse(d.name()), Some(d));
        }
    }

    #[test]
    fn pinned_samples_guard_the_stream_layout() {
        // First pixels of the seed-1 uniform-range field, 16-bit: any
        // change to the hash, the stream numbering or the mapping to
        // (0, 1) shows up here.
        let img = canvas::render(
            &RandomOptions::new(2, 1)
                .with_seed(1)
                .with_distribution(NoiseDistribution::Random)
                .field(),
            &RenderConfig::new().with_depth(SampleDepth::U16),
        );
        let want: Vec<u16> = [(0u32, 0u32), (1, 0)]
            .iter()
            .flat_map(|&(x, y)| {
                let ch = move |c: u32| {
                    let u = (f64::from(mix(1, 0xA24B_AED5 ^ c, x, y)) + 0.5) / 4_294_967_296.0;
                    (u * 65535.0).round() as u16
                };
                [ch(0), ch(1), ch(2), 65535]
            })
            .collect();
        assert_eq!(img.as_u16().unwrap(), &want[..]);
    }

    #[test]
    fn mono_noise_is_achromatic_and_colour_noise_is_not() {
        let mono = render(
            &[("mono", "1"), ("w", "16"), ("h", "16")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
        .unwrap();
        assert!(
            mono.pixels
                .chunks(4)
                .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255)
        );
        let colour = canvas::render(&RandomOptions::new(16, 16).field(), &RenderConfig::new());
        assert!(colour.as_u8().unwrap().chunks(4).any(|p| p[0] != p[1]));
    }

    #[test]
    fn eight_bit_noise_is_the_rounded_clamped_float_noise() {
        let f = opts(NoiseDistribution::Gaussian).with_amount(0.4).field();
        let low = canvas::render(&f, &RenderConfig::new());
        let float = canvas::render(&f, &RenderConfig::new().with_depth(SampleDepth::F32));
        let mut clipped = 0;
        for (a, v) in low.as_u8().unwrap().iter().zip(float.as_f32().unwrap()) {
            let scaled = f64::from(*v).clamp(0.0, 1.0) * 255.0;
            if !(0.0..=1.0).contains(v) {
                clipped += 1;
            }
            if (scaled - scaled.floor() - 0.5).abs() > 1e-4 {
                assert_eq!(f64::from(*a), scaled.round());
            }
        }
        assert!(clipped > 0, "σ = 0.4 should exceed the range somewhere");
    }

    #[test]
    fn bad_parameters_are_refused() {
        let q = |k: &str, v: &str| -> BTreeMap<String, String> {
            [(k.to_string(), v.to_string())].into_iter().collect()
        };
        assert!(render(&q("type", "cauchy")).is_err());
        assert!(render(&q("amount", "-1")).is_err());
        assert!(render(&q("color", "nocolour")).is_err());
    }

    #[test]
    fn exp_neg_matches_the_library_exponential() {
        for i in 0..=480 {
            let m = f64::from(i) / 10.0;
            let want = (-m).exp();
            assert!((exp_neg(m) - want).abs() <= 1e-13 * want, "{m}");
        }
    }
}

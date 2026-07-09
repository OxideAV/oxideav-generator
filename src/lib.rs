//! Pure-Rust synthetic media generator for the oxideav framework.
//!
//! Two integration shapes:
//!
//! 1. **Source driver** — `generate://...` URIs. Register via
//!    [`register_source`] on a [`SourceRegistry`]. Opening a generate URI
//!    returns a [`SourceOutput::Frames`](oxideav_core::SourceOutput)
//!    handle (`Box<dyn FrameSource>`) — frames are produced natively;
//!    no container or decoder runs in front of them.
//!
//! 2. **Zero-input filter** — every generator is also exposed as a
//!    [`StreamFilter`](oxideav_core::StreamFilter) factory under the
//!    `audio.synth`, `image.xc`, `image.gradient`, `image.pattern`,
//!    `image.grating`, `image.fractal`, `image.plasma`, `image.noise`,
//!    `video.testsrc`, `video.smptebars`, `video.fractal_zoom`,
//!    `video.gradient_animate`, `video.zoneplate`, `video.scroll`,
//!    `video.colorwheel`, `video.movingbox`, `video.snow`
//!    names. Register them
//!    via [`register_filters`] on a
//!    [`RuntimeContext`](oxideav_core::RuntimeContext).
//!
//! For the common case where you want both shapes installed at once,
//! use the unified [`register`] entry point — it threads a single
//! [`RuntimeContext`](oxideav_core::RuntimeContext) through both
//! [`register_source`] (on `ctx.sources`) and [`register_filters`].
//!
//! ## CLI shorthands
//!
//! [`shorthand::translate`] takes short, colon-prefixed CLI inputs in the
//! traditional Unix media-tool style — `xc:red`, `gradient:red-blue`,
//! `synth:5,sine,440`, `testsrc:` — and rewrites them to canonical
//! `generate://...` URIs. The CLI's `convert` verb runs every input
//! through this translator before handing it to the source registry.
//! Other verbs (probe / transcode / remux / run) accept the canonical
//! URI form only.
//!
//! ## Catalog
//!
//! - **Audio** — sine / square / triangle / sawtooth / supersaw
//!   (detuned-sawtooth stack) / pwm (pulse-width modulated rectangular
//!   wave) / pluck (Karplus-Strong) / chirp (linear or exponential
//!   sweep) / fm (frequency modulation) / am (sinusoidal amplitude
//!   modulation) / tremolo (sub-audio unipolar-cosine envelope on any
//!   carrier) / ringmod (carrier-suppressed product of two sines) /
//!   dtmf (telephone touch-tones) /
//!   formant (Klatt-style two-formant vowel synthesizer) / multitone
//!   (sum of sines) / white-pink-brown-blue-violet noise / silence /
//!   dc (constant `s[n] = level` offset signal) / impulse (unipolar
//!   impulse train — `width` samples at `+amplitude` every `period`
//!   samples, drift-free integer arithmetic). `type=sine` additionally
//!   honours `phase=` (initial phase, degrees) and `chphase=`
//!   (per-channel phase offset, degrees — channel `c` is rendered at
//!   `phase + c·chphase`, the stereo / inter-channel-correlation
//!   probe).
//! - **Image basics** — solid colour (`xc`), linear / radial gradient,
//!   checkerboard / horizontal / vertical / diagonal patterns,
//!   sinusoidal grating (single-frequency cos at a chosen orientation).
//! - **Procedural images** — Mandelbrot + Julia fractals, plasma
//!   (recursive midpoint displacement), Perlin + simplex gradient
//!   noise, Worley cellular noise (`type=worley`, alias `cellular`;
//!   `dist=euclidean|manhattan|chebyshev`, `k ∈ [1, 4]`,
//!   `points ∈ [1, 4]` feature-points per cell).
//! - **Video** — `testsrc` (animated timestamp + colour bars + circle —
//!   the classical broadcast-engineering test signal), `smptebars`
//!   (SMPTE 75% colour bars), `fractal_zoom` (animated Mandelbrot zoom),
//!   `gradient_animate` (hue-rotating gradient), `zoneplate` (radial
//!   `cos(k·r²)` chirp — spatial-frequency probe), `scroll`
//!   (constant-velocity toroidal translation of a base pattern —
//!   bit-exact ground-truth motion-estimation probe), `colorwheel`
//!   (rotating polar hue wheel — hue from `atan2` angle, saturation
//!   from normalised radius; a chroma + angular-motion probe),
//!   `movingbox` (a solid `bw × bh` rectangle translating at signed
//!   integer pixels-per-frame `(vx, vy)` over a solid background with
//!   toroidal wrap — the local-motion counterpart of `scroll`'s global
//!   motion, with the object's true motion vector known exactly),
//!   `snow` (seeded temporal noise — every pixel of every frame is a
//!   stateless counter-mode hash of `(seed, frame, x, y)`, the
//!   worst-case-entropy rate-control stress input, byte-reproducible
//!   across runs and machines).

#![allow(clippy::too_many_arguments)]

pub mod audio;
pub mod image;
pub mod shorthand;
pub mod source;
pub mod video;

mod filters;

pub use filters::register_filters;
pub use source::{open_generate_frames, register_source};

/// Install every generator integration into a full runtime context.
///
/// This is the unified `register(&mut RuntimeContext)` entry point
/// every sibling crate exposes. It calls
/// [`register_source`] on `ctx.sources` (so `generate://...` URIs
/// open as `SourceOutput::Frames`) and [`register_filters`] on `ctx`
/// (so the `audio.synth` / `image.*` / `video.*` factories show up in
/// `ctx.filters`). Callers that only want one half can keep using the
/// helpers directly.
///
/// Also wired into [`oxideav_meta::register_all`] via the
/// [`oxideav_core::register!`] macro below.
pub fn register(ctx: &mut oxideav_core::RuntimeContext) {
    source::register_source(&mut ctx.sources);
    filters::register_filters(ctx);
}

oxideav_core::register!("generator", register);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn register_via_runtime_context_installs_source_and_filters() {
        let mut ctx = oxideav_core::RuntimeContext::new();
        register(&mut ctx);

        // Source side: the `generate` scheme must be installed on
        // `ctx.sources`.
        let schemes: BTreeSet<&str> = ctx.sources.schemes().collect();
        assert!(
            schemes.contains("generate"),
            "register did not install the generate URI scheme; got {schemes:?}"
        );

        // Filter side: every published filter name must land in
        // `ctx.filters`. Names mirror the catalogue documented on
        // `register_filters`.
        for name in [
            "audio.synth",
            "image.xc",
            "image.gradient",
            "image.pattern",
            "image.grating",
            "image.fractal",
            "image.plasma",
            "image.noise",
            "video.testsrc",
            "video.smptebars",
            "video.fractal_zoom",
            "video.gradient_animate",
            "video.zoneplate",
            "video.scroll",
            "video.colorwheel",
            "video.movingbox",
            "video.snow",
        ] {
            assert!(
                ctx.filters.contains(name),
                "register did not install the {name} filter"
            );
        }
    }
}

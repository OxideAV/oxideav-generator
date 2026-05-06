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
//!    `image.fractal`, `image.plasma`, `image.noise`, `video.testsrc`,
//!    `video.smptebars`, `video.fractal_zoom`, `video.gradient_animate`
//!    names. Register them via [`register_filters`] on a
//!    [`RuntimeContext`](oxideav_core::RuntimeContext).
//!
//! For the common case where you want both shapes installed at once,
//! use the unified [`register`] entry point — it threads a single
//! [`RuntimeContext`](oxideav_core::RuntimeContext) through both
//! [`register_source`] (on `ctx.sources`) and [`register_filters`].
//!
//! ## CLI shorthands
//!
//! [`shorthand::translate`] takes ImageMagick / sox style inputs
//! (`xc:red`, `gradient:red-blue`, `synth:5,sine,440`, `testsrc:`, …)
//! and rewrites them to canonical `generate://...` URIs. The CLI's
//! `convert` verb runs every input through this translator before
//! handing it to the source registry. Other verbs (probe / transcode /
//! remux / run) accept the canonical URI form only.
//!
//! ## Catalog
//!
//! - **Audio** — sine / square / triangle / sawtooth / pluck (Karplus-
//!   Strong) / white-pink-brown noise / silence.
//! - **Image basics** — solid colour (`xc`), linear / radial gradient,
//!   checkerboard / horizontal / vertical / diagonal patterns.
//! - **Procedural images** — Mandelbrot + Julia fractals, plasma
//!   (recursive midpoint displacement), Perlin noise.
//! - **Video** — `testsrc` (ffmpeg-equivalent timestamp + colour bars +
//!   circle), `smptebars` (SMPTE 75% colour bars), `fractal_zoom`
//!   (animated Mandelbrot zoom), `gradient_animate` (hue-rotating
//!   gradient).

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
            "image.fractal",
            "image.plasma",
            "image.noise",
            "video.testsrc",
            "video.smptebars",
            "video.fractal_zoom",
            "video.gradient_animate",
        ] {
            assert!(
                ctx.filters.contains(name),
                "register did not install the {name} filter"
            );
        }
    }
}

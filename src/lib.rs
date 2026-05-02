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

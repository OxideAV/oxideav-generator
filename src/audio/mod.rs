//! Audio generators (synth + noise + silence).
//!
//! Every generator emits a canonical PCM WAV byte stream that the
//! `oxideav-basic` WAV demuxer consumes verbatim.

pub mod synth;
pub mod wav;

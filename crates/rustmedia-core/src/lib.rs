//! # rustmedia-core
//!
//! The shared type vocabulary of the [RustMedia](https://github.com/arbazkhan971/rustmedia)
//! toolkit: errors, timestamps, codecs, tracks, packets, metadata, and format
//! identifiers. Every other RustMedia crate — parsers, muxers, the CLI, the
//! bindings — speaks in these types.
//!
//! This crate is deliberately tiny and has **zero mandatory dependencies**, so
//! it is cheap to depend on. Enable the `serde` feature to derive
//! `Serialize`/`Deserialize` on the public types.
//!
//! You normally do not depend on `rustmedia-core` directly; the umbrella
//! [`rustmedia`](https://docs.rs/rustmedia) crate re-exports everything here.
//!
//! ## The vocabulary at a glance
//!
//! - [`ContainerFormat`] — which container a file uses (MP4, Matroska, …).
//! - [`Track`] — one elementary stream, described by its [`Codec`],
//!   [`MediaType`], and category-specific [`TrackParameters`].
//! - [`Packet`] — one coded, undecoded unit of a track's data.
//! - [`Timestamp`] / [`Rational`] — exact, timescale-aware timing.
//! - [`Metadata`] — container tags and [`Chapter`]s.
//! - [`Error`] / [`Result`] — the fallible surface of the whole toolkit.

pub mod codec;
pub mod error;
pub mod format;
pub mod metadata;
pub mod packet;
pub mod time;
pub mod track;

pub use codec::{Codec, MediaType};
pub use error::{Error, Result};
pub use format::ContainerFormat;
pub use metadata::{Chapter, Metadata};
pub use packet::Packet;
pub use time::{format_duration, parse_duration, Rational, Timestamp};
pub use track::{AudioParameters, SubtitleParameters, Track, TrackParameters, VideoParameters};

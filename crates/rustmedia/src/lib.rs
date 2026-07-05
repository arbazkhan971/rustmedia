//! # RustMedia
//!
//! Fast, safe, FFmpeg-free media parsing and processing for Rust.
//!
//! RustMedia inspects, parses, and moves media between containers — MP4, MOV,
//! Matroska/WebM, WAV, and MP3 — without shelling out to FFmpeg and without a
//! line of `unsafe`. This umbrella crate is the front door: it re-exports the
//! core type vocabulary and adds the ergonomic [`Media`] facade.
//!
//! ```no_run
//! use rustmedia::Media;
//!
//! let media = Media::open("movie.mp4")?;
//! println!("{} · {:?}", media.format(), media.duration());
//! for track in media.tracks() {
//!     println!("#{} {} {}", track.id, track.media_type, track.codec);
//! }
//! # Ok::<(), rustmedia::Error>(())
//! ```
//!
//! ## Crate layout
//!
//! `rustmedia` re-exports the pieces you need, but each lives in a focused
//! crate you can depend on directly:
//!
//! - [`rustmedia_core`] — the dependency-free type vocabulary.
//! - [`rustmedia_io`] — endian-aware readers and seekable sources.
//! - [`rustmedia_formats`] — the native container parsers and the
//!   [`Demuxer`](rustmedia_formats::Demuxer) trait.

mod media;

pub use media::{Media, MediaError, Packets};

// Re-export the core type vocabulary so `use rustmedia::*` is enough.
pub use rustmedia_core::{
    format_duration, parse_duration, AudioParameters, Chapter, Codec, ContainerFormat, Error,
    MediaType, Metadata, Packet, Rational, Result, SubtitleParameters, Timestamp, Track,
    TrackParameters, VideoParameters,
};

// Re-export the demuxer surface for callers that want the lower-level API.
pub use rustmedia_formats::{detect, open, Demuxer};

/// Metadata tag-key constants (`title`, `artist`, …). See
/// [`rustmedia_core::metadata::keys`].
pub mod tags {
    pub use rustmedia_core::metadata::keys::*;
}

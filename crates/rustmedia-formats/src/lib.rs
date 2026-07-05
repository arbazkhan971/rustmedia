//! # rustmedia-formats
//!
//! Native container parsers for the RustMedia toolkit. No FFmpeg, no C, no
//! `unsafe`: every byte is parsed in safe Rust.
//!
//! Each format implements the [`Demuxer`] trait, so higher layers treat every
//! container the same way. [`open`] sniffs a source's magic bytes and returns
//! the right demuxer boxed behind that trait.
//!
//! ```no_run
//! use std::fs::File;
//! use rustmedia_formats::open;
//!
//! let file = File::open("movie.mp4")?;
//! let demuxer = open(file)?;
//! for track in demuxer.tracks() {
//!     println!("track {}: {}", track.id, track.codec);
//! }
//! # Ok::<(), rustmedia_core::Error>(())
//! ```
//!
//! ## Format support
//!
//! | Format         | Demux | Notes                                  |
//! |----------------|:-----:|----------------------------------------|
//! | MP4 / MOV      |   ✅   | ISO-BMFF, non-fragmented, `co64`, `ctts` |
//! | Matroska/WebM  |   🚧   | in progress                            |
//! | WAV            |   🚧   | in progress                            |
//! | MP3            |   🚧   | in progress                            |

use std::io::{Read, Seek};

use rustmedia_core::{ContainerFormat, Error, Result};

pub mod demux;
pub mod detect;
pub mod mp4;

pub use demux::Demuxer;
pub use detect::{detect, detect_bytes};
pub use mp4::Mp4Demuxer;

/// Detect the format of `reader` and return a demuxer for it.
///
/// The reader must be both readable and seekable (a [`File`](std::fs::File) or
/// in-memory [`Cursor`](std::io::Cursor)). Ownership is taken because the
/// returned demuxer reads packets lazily from the same source.
///
/// # Errors
/// - [`Error::UnknownFormat`] if the magic bytes match no supported container.
/// - [`Error::Unsupported`] if the format is recognised but not yet parseable.
/// - [`Error::Malformed`] if the container is corrupt.
pub fn open<R: Read + Seek + 'static>(mut reader: R) -> Result<Box<dyn Demuxer>> {
    match detect(&mut reader)? {
        Some(ContainerFormat::Mp4 | ContainerFormat::Mov) => Ok(Box::new(Mp4Demuxer::new(reader)?)),
        Some(other) => Err(Error::unsupported(format!(
            "{other} demuxing is not yet implemented"
        ))),
        None => Err(Error::UnknownFormat),
    }
}

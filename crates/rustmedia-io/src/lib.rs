//! # rustmedia-io
//!
//! Byte-level I/O primitives for the [RustMedia](https://github.com/rustmedia/rustmedia)
//! toolkit: endian-aware reading ([`ReadBytes`]) and seekable sources
//! ([`Source`]).
//!
//! These are the low-level building blocks the format parsers stand on. They
//! depend only on `rustmedia-core` for the shared error type and add no
//! third-party dependencies.
//!
//! ```
//! use std::io::Cursor;
//! use rustmedia_io::ReadBytes;
//!
//! let mut r = Cursor::new(b"\0\0\0\x18ftyp".to_vec());
//! let size = r.read_u32_be().unwrap();
//! let kind = r.read_fourcc().unwrap();
//! assert_eq!(size, 24);
//! assert_eq!(&kind, b"ftyp");
//! ```

pub mod reader;
pub mod source;

pub use reader::ReadBytes;
pub use source::{open_file, Source};

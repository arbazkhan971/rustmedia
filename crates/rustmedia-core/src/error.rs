//! The crate-wide [`Error`] type and [`Result`] alias.
//!
//! RustMedia deliberately hand-rolls its error type rather than pulling in a
//! derive macro: the core vocabulary crate stays dependency-free, and media
//! parsing errors carry the two things that actually help when a file refuses
//! to open — *which format* rejected it and *at what byte offset*.

use std::fmt;

/// A specialised [`Result`](std::result::Result) for RustMedia operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The error type returned throughout RustMedia.
///
/// The variants are intentionally coarse; the human-readable `message` and the
/// optional byte `offset` carry the detail. Match on the variant when you need
/// to react programmatically (for example, treating [`Error::Unsupported`] as a
/// soft failure), and rely on the [`Display`](fmt::Display) impl for reporting.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An underlying I/O operation failed.
    Io(std::io::Error),

    /// The input ended before a complete structure could be read.
    ///
    /// This usually means the file is truncated. The string describes what was
    /// being read when the stream ran out.
    UnexpectedEof(String),

    /// The container format could not be recognised from its contents.
    UnknownFormat,

    /// The data is malformed or violates the format specification.
    Malformed {
        /// Short identifier of the parser that raised the error (e.g. `"mp4"`).
        format: &'static str,
        /// Byte offset into the source where the problem was detected, if known.
        offset: Option<u64>,
        /// Human-readable description of what went wrong.
        message: String,
    },

    /// A recognised feature that RustMedia does not yet support was encountered.
    ///
    /// Distinct from [`Error::Malformed`]: the input is valid, RustMedia simply
    /// cannot handle it (yet). Callers may reasonably choose to skip or degrade.
    Unsupported(String),

    /// An argument supplied by the caller was invalid.
    InvalidArgument(String),
}

impl Error {
    /// Construct a [`Error::Malformed`] with no known offset.
    pub fn malformed(format: &'static str, message: impl Into<String>) -> Self {
        Error::Malformed {
            format,
            offset: None,
            message: message.into(),
        }
    }

    /// Construct a [`Error::Malformed`] pinned to a byte `offset`.
    pub fn malformed_at(format: &'static str, offset: u64, message: impl Into<String>) -> Self {
        Error::Malformed {
            format,
            offset: Some(offset),
            message: message.into(),
        }
    }

    /// Construct an [`Error::Unsupported`].
    pub fn unsupported(message: impl Into<String>) -> Self {
        Error::Unsupported(message.into())
    }

    /// Construct an [`Error::InvalidArgument`].
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Error::InvalidArgument(message.into())
    }

    /// Attach a byte `offset` to a [`Error::Malformed`], if it does not already
    /// have one. Other variants pass through unchanged.
    #[must_use]
    pub fn at_offset(self, offset: u64) -> Self {
        match self {
            Error::Malformed {
                format,
                offset: None,
                message,
            } => Error::Malformed {
                format,
                offset: Some(offset),
                message,
            },
            other => other,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::UnexpectedEof(what) => {
                write!(f, "unexpected end of input while reading {what}")
            }
            Error::UnknownFormat => {
                f.write_str("unrecognised media format: no known container matched the input")
            }
            Error::Malformed {
                format,
                offset: Some(off),
                message,
            } => {
                write!(f, "malformed {format} at byte {off}: {message}")
            }
            Error::Malformed {
                format,
                offset: None,
                message,
            } => {
                write!(f, "malformed {format}: {message}")
            }
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        // An EOF at the I/O layer is almost always a truncated file; surface it
        // as the more descriptive variant so callers get a useful message.
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::UnexpectedEof("input stream".to_string())
        } else {
            Error::Io(e)
        }
    }
}

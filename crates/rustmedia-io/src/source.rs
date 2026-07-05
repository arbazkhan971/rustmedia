//! Seekable media sources.
//!
//! Parsers in RustMedia are generic over `R: Read + Seek`, which covers files
//! (buffered), in-memory `Cursor`s, and anything else random-access. The
//! [`Source`] extension trait adds the couple of positioning helpers those
//! parsers reach for constantly.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use rustmedia_core::error::Result;

/// A random-access byte source: readable and seekable.
///
/// Implemented for every `T: Read + Seek`, so you rarely name it directly. Use
/// [`Seek::stream_position`] for the current offset; this trait adds the two
/// helpers parsers reach for that std does not provide directly.
pub trait Source: Read + Seek {
    /// The total length of the stream in bytes.
    ///
    /// Implemented by seeking to the end and restoring the previous position,
    /// so it is cheap for files and cursors but does perturb the seek cursor
    /// mid-call.
    fn size(&mut self) -> Result<u64> {
        let cur = self.stream_position()?;
        let end = self.seek(SeekFrom::End(0))?;
        self.seek(SeekFrom::Start(cur))?;
        Ok(end)
    }

    /// Seek to an absolute byte `offset` from the start of the stream.
    fn seek_to(&mut self, offset: u64) -> Result<()> {
        self.seek(SeekFrom::Start(offset))?;
        Ok(())
    }
}

impl<T: Read + Seek + ?Sized> Source for T {}

/// Open a file as a buffered, seekable [`Source`].
///
/// # Errors
/// Propagates any [`std::io::Error`] from opening the file.
pub fn open_file(path: impl AsRef<Path>) -> Result<BufReader<File>> {
    let file = File::open(path)?;
    Ok(BufReader::new(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn size_and_position_round_trip() {
        let mut c = Cursor::new(vec![0u8; 42]);
        assert_eq!(c.size().unwrap(), 42);
        c.seek_to(10).unwrap();
        assert_eq!(c.stream_position().unwrap(), 10);
        // size() must restore the position it found.
        assert_eq!(c.size().unwrap(), 42);
        assert_eq!(c.stream_position().unwrap(), 10);
    }
}

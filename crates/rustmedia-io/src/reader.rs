//! Endian-aware reading helpers layered on top of [`std::io::Read`].
//!
//! Media formats are a soup of fixed-width big- and little-endian integers.
//! Rather than depend on `byteorder`, RustMedia ships a small, focused
//! extension trait so `rustmedia-io` stays dependency-free (apart from
//! `rustmedia-core` for the error type).

use std::io::Read;

use rustmedia_core::error::{Error, Result};

/// Read fixed-width integers and byte runs from any [`Read`]er.
///
/// Every method reads exactly the requested number of bytes and maps a short
/// read to [`Error::UnexpectedEof`] labelled with what was being read, which
/// turns "file ended" into an actionable message.
///
/// The trait is implemented for all `R: Read`, so it is available on files,
/// cursors, network streams, and decompressors alike.
pub trait ReadBytes: Read {
    /// Read exactly `N` bytes into a fixed-size array.
    fn read_arr<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut buf = [0u8; N];
        self.read_exact(&mut buf)
            .map_err(|e| eof(e, "fixed-size field"))?;
        Ok(buf)
    }

    /// Read exactly `n` bytes into a freshly allocated `Vec`.
    ///
    /// The size `n` often comes from the file being parsed and is therefore
    /// untrusted: a corrupt length field must not be able to trigger a huge
    /// speculative allocation. So rather than pre-allocating `n` bytes, this
    /// reads *what the stream actually provides* (capped at `n`) and only then
    /// checks that the full `n` bytes arrived — a truncated or lying length
    /// yields [`Error::UnexpectedEof`], not an out-of-memory abort.
    fn read_vec(&mut self, n: usize) -> Result<Vec<u8>> {
        // Cap the initial allocation; the buffer grows as real bytes arrive.
        // The reborrow (`&mut *self`) yields a `Sized` reader so `take` applies
        // even when `Self` is `?Sized`.
        const INITIAL_CAP: usize = 64 * 1024;
        let mut buf = Vec::with_capacity(n.min(INITIAL_CAP));
        let read = (&mut *self)
            .take(n as u64)
            .read_to_end(&mut buf)
            .map_err(|e| eof(e, "byte run"))?;
        if read != n {
            return Err(Error::UnexpectedEof("byte run".to_string()));
        }
        Ok(buf)
    }

    /// Read a single unsigned byte.
    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_arr::<1>()?[0])
    }

    /// Read a single signed byte.
    fn read_i8(&mut self) -> Result<i8> {
        Ok(self.read_u8()? as i8)
    }

    /// Read a big-endian `u16`.
    fn read_u16_be(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.read_arr()?))
    }

    /// Read a little-endian `u16`.
    fn read_u16_le(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read_arr()?))
    }

    /// Read a big-endian 24-bit unsigned integer, returned in a `u32`.
    fn read_u24_be(&mut self) -> Result<u32> {
        let b = self.read_arr::<3>()?;
        Ok(u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]))
    }

    /// Read a big-endian `u32`.
    fn read_u32_be(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.read_arr()?))
    }

    /// Read a little-endian `u32`.
    fn read_u32_le(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_arr()?))
    }

    /// Read a big-endian `u64`.
    fn read_u64_be(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.read_arr()?))
    }

    /// Read a little-endian `u64`.
    fn read_u64_le(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read_arr()?))
    }

    /// Read a big-endian `i16`.
    fn read_i16_be(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.read_arr()?))
    }

    /// Read a big-endian `i32`.
    fn read_i32_be(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.read_arr()?))
    }

    /// Read a big-endian IEEE-754 `f32`.
    fn read_f32_be(&mut self) -> Result<f32> {
        Ok(f32::from_be_bytes(self.read_arr()?))
    }

    /// Read a little-endian IEEE-754 `f32`.
    fn read_f32_le(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.read_arr()?))
    }

    /// Read a four-byte type/chunk identifier ("fourcc"), as used by ISO-BMFF
    /// boxes and RIFF chunks.
    fn read_fourcc(&mut self) -> Result<[u8; 4]> {
        self.read_arr::<4>()
    }

    /// Read a 16.16 fixed-point number (as found in MP4 `mvhd`/`tkhd`), returned
    /// as an `f64`.
    fn read_fixed16_16(&mut self) -> Result<f64> {
        Ok(f64::from(self.read_i32_be()?) / f64::from(1 << 16))
    }

    /// Discard `n` bytes from the stream by reading and throwing them away.
    ///
    /// Prefer a real seek when the source is [`Seek`](std::io::Seek)able; this
    /// exists for the read-only case and for skipping small padding runs.
    fn skip(&mut self, n: u64) -> Result<()> {
        let mut remaining = n;
        let mut scratch = [0u8; 4096];
        while remaining > 0 {
            let want = remaining.min(scratch.len() as u64) as usize;
            let read = self.read(&mut scratch[..want]).map_err(Error::from)?;
            if read == 0 {
                return Err(Error::UnexpectedEof("skipped region".to_string()));
            }
            remaining -= read as u64;
        }
        Ok(())
    }
}

impl<R: Read + ?Sized> ReadBytes for R {}

fn eof(e: std::io::Error, what: &str) -> Error {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::UnexpectedEof(what.to_string())
    } else {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_multi_byte_integers() {
        let mut c = Cursor::new(vec![0x01, 0x02, 0x03, 0x04, 0xAA, 0xBB]);
        assert_eq!(c.read_u32_be().unwrap(), 0x0102_0304);
        assert_eq!(c.read_u16_le().unwrap(), 0xBBAA);
    }

    #[test]
    fn reads_u24_and_fourcc() {
        let mut c = Cursor::new(b"\x00\x01\x00ftyp".to_vec());
        assert_eq!(c.read_u24_be().unwrap(), 0x0000_0100);
        assert_eq!(&c.read_fourcc().unwrap(), b"ftyp");
    }

    #[test]
    fn short_read_is_unexpected_eof() {
        let mut c = Cursor::new(vec![0x01, 0x02]);
        assert!(matches!(c.read_u32_be(), Err(Error::UnexpectedEof(_))));
    }

    #[test]
    fn skip_advances() {
        let mut c = Cursor::new(vec![0u8; 10]);
        c.skip(8).unwrap();
        assert_eq!(c.read_u16_be().unwrap(), 0);
        assert!(c.read_u8().is_err());
    }
}

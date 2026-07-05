//! A minimal EBML reader.
//!
//! Matroska and WebM are EBML documents: a tree of elements, each a variable-
//! length **ID** followed by a variable-length **size** and then its data. Both
//! the ID and the size are "vint"s — the number of leading zero bits in the
//! first byte gives the total length. This module reads those primitives; the
//! Matroska demuxer maps element IDs to meaning.

use std::io::Read;

use rustmedia_core::{Error, Result};
use rustmedia_io::ReadBytes;

/// The result of reading one element header: its ID and its declared data size
/// (`None` for the "unknown size" encoding used by live streams).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ElementHeader {
    pub id: u32,
    pub size: Option<u64>,
}

/// Read an EBML element ID (1–4 bytes), keeping its length-marker bits.
pub(crate) fn read_id<R: Read>(r: &mut R) -> Result<u32> {
    let b0 = r.read_u8()?;
    let len = leading_length(b0)
        .ok_or_else(|| Error::malformed("matroska", "invalid EBML element id"))?;
    if len > 4 {
        return Err(Error::malformed("matroska", "EBML id longer than 4 bytes"));
    }
    let mut value = u32::from(b0);
    for _ in 1..len {
        value = (value << 8) | u32::from(r.read_u8()?);
    }
    Ok(value)
}

/// Read an EBML data size (1–8 bytes). Returns `Ok(None)` for the all-ones
/// "unknown size" encoding.
pub(crate) fn read_size<R: Read>(r: &mut R) -> Result<Option<u64>> {
    let b0 = r.read_u8()?;
    let len =
        leading_length(b0).ok_or_else(|| Error::malformed("matroska", "invalid EBML size"))?;
    // Clear the length-marker bit from the first byte.
    let mask = 0xFFu16 >> len;
    let mut value = u64::from(b0 & mask as u8);
    let mut all_ones = value == u64::from(mask as u8);
    for _ in 1..len {
        let byte = r.read_u8()?;
        value = (value << 8) | u64::from(byte);
        all_ones = all_ones && byte == 0xFF;
    }
    // The reserved "unknown size" value is all data bits set to 1.
    if all_ones {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

/// Read a full element header (ID then size).
pub(crate) fn read_element<R: Read>(r: &mut R) -> Result<ElementHeader> {
    let id = read_id(r)?;
    let size = read_size(r)?;
    Ok(ElementHeader { id, size })
}

/// Read a big-endian unsigned integer of `len` bytes (0–8).
pub(crate) fn read_uint<R: Read>(r: &mut R, len: usize) -> Result<u64> {
    let mut value = 0u64;
    for _ in 0..len.min(8) {
        value = (value << 8) | u64::from(r.read_u8()?);
    }
    Ok(value)
}

/// Read an EBML float (`len` is 0, 4, or 8; 0 means the value 0.0).
pub(crate) fn read_float<R: Read>(r: &mut R, len: usize) -> Result<f64> {
    match len {
        0 => Ok(0.0),
        4 => Ok(f64::from(f32::from_bits(read_uint(r, 4)? as u32))),
        8 => Ok(f64::from_bits(read_uint(r, 8)?)),
        other => Err(Error::malformed(
            "matroska",
            format!("bad float length {other}"),
        )),
    }
}

/// Read a UTF-8 string of `len` bytes, trimming trailing NULs.
pub(crate) fn read_string<R: Read>(r: &mut R, len: usize) -> Result<String> {
    let bytes = r.read_vec(len)?;
    let s = String::from_utf8_lossy(&bytes);
    Ok(s.trim_end_matches('\0').to_string())
}

/// The total vint length encoded by the first byte's leading bit position, or
/// `None` if the byte is all zeros (invalid).
fn leading_length(first: u8) -> Option<u32> {
    if first == 0 {
        None
    } else {
        Some(first.leading_zeros() + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_four_byte_id() {
        // EBML header magic.
        let mut c = Cursor::new(vec![0x1A, 0x45, 0xDF, 0xA3]);
        assert_eq!(read_id(&mut c).unwrap(), 0x1A45_DFA3);
    }

    #[test]
    fn reads_one_byte_id() {
        let mut c = Cursor::new(vec![0xAE]); // TrackEntry
        assert_eq!(read_id(&mut c).unwrap(), 0xAE);
    }

    #[test]
    fn reads_sizes() {
        // 0x81 -> length 1, value 1.
        let mut c = Cursor::new(vec![0x81]);
        assert_eq!(read_size(&mut c).unwrap(), Some(1));
        // 0x40 0x02 -> length 2, value 2.
        let mut c = Cursor::new(vec![0x40, 0x02]);
        assert_eq!(read_size(&mut c).unwrap(), Some(2));
        // 0xFF -> length 1, all-ones -> unknown.
        let mut c = Cursor::new(vec![0xFF]);
        assert_eq!(read_size(&mut c).unwrap(), None);
    }

    #[test]
    fn reads_uint_and_float() {
        let mut c = Cursor::new(vec![0x01, 0x00]);
        assert_eq!(read_uint(&mut c, 2).unwrap(), 256);
        let mut c = Cursor::new(48_000.0f32.to_bits().to_be_bytes().to_vec());
        assert!((read_float(&mut c, 4).unwrap() - 48_000.0).abs() < 0.01);
    }
}

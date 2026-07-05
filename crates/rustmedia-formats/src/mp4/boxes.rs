//! ISO-BMFF box headers.
//!
//! An ISO Base Media File is a tree of *boxes* (called "atoms" in QuickTime).
//! Every box starts with a 32-bit big-endian size and a four-character type.
//! A size of `1` means the real size is a 64-bit value that follows the type;
//! a size of `0` means the box runs to the end of the file. This module reads
//! those headers; the higher-level parsers walk the tree.

use std::io::Read;

use rustmedia_core::{Error, Result};
use rustmedia_io::ReadBytes;

/// A four-character box/atom type code.
pub(crate) type FourCc = [u8; 4];

/// Render a four-character code for human-readable diagnostics, replacing
/// non-printable bytes with `.`.
pub(crate) fn fourcc_str(code: &FourCc) -> String {
    code.iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

/// A parsed box header.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoxHeader {
    /// The four-character type code.
    pub kind: FourCc,
    /// Total box size in bytes including this header. `0` means "to end of
    /// stream" (only legal for the last top-level box).
    pub size: u64,
    /// Length of this header in bytes (8 for a normal box, 16 for a 64-bit one).
    pub header_len: u64,
}

impl BoxHeader {
    /// Length of the box payload (everything after the header). For a
    /// "to-EOF" box the caller must supply `bytes_remaining` in the enclosing
    /// scope; otherwise it is derived from [`BoxHeader::size`].
    pub(crate) fn payload_len(&self, bytes_remaining: u64) -> u64 {
        if self.size == 0 {
            bytes_remaining.saturating_sub(self.header_len)
        } else {
            self.size - self.header_len
        }
    }
}

/// Read one box header from `r`.
///
/// Returns `Ok(None)` on a clean end-of-stream (no bytes left), so callers can
/// loop until the children of a container are exhausted.
pub(crate) fn read_box_header<R: Read>(r: &mut R) -> Result<Option<BoxHeader>> {
    let mut size_buf = [0u8; 4];
    if !read_full_or_eof(r, &mut size_buf)? {
        return Ok(None);
    }
    let size32 = u32::from_be_bytes(size_buf);
    let kind = r.read_fourcc()?;

    let (size, header_len) = match size32 {
        1 => (r.read_u64_be()?, 16u64),
        0 => (0u64, 8u64),
        n => (u64::from(n), 8u64),
    };

    if size != 0 && size < header_len {
        return Err(Error::malformed(
            "mp4",
            format!(
                "box '{}' declares size {size} smaller than its header",
                fourcc_str(&kind)
            ),
        ));
    }

    Ok(Some(BoxHeader {
        kind,
        size,
        header_len,
    }))
}

/// Iterate the child boxes contained in a byte region, returning each child's
/// type code and payload slice. Handles both 32-bit and 64-bit box sizes.
/// Iteration stops at the first malformed or out-of-bounds child rather than
/// erroring, so partially-recoverable structures still yield what they can.
pub(crate) fn boxes_in(data: &[u8]) -> Vec<(FourCc, &[u8])> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size32 = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let kind: FourCc = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let (header, total) = match size32 {
            1 => {
                if pos + 16 > data.len() {
                    break;
                }
                let large = u64::from_be_bytes([
                    data[pos + 8],
                    data[pos + 9],
                    data[pos + 10],
                    data[pos + 11],
                    data[pos + 12],
                    data[pos + 13],
                    data[pos + 14],
                    data[pos + 15],
                ]);
                (16usize, large as usize)
            }
            0 => (8usize, data.len() - pos),
            n => (8usize, n as usize),
        };
        if total < header || pos + total > data.len() {
            break;
        }
        out.push((kind, &data[pos + header..pos + total]));
        pos += total;
    }
    out
}

/// The version byte and 24-bit flags that begin a "full box".
#[derive(Debug, Clone, Copy)]
pub(crate) struct FullBoxHeader {
    /// Box version (0 or 1 for the boxes RustMedia reads).
    pub(crate) version: u8,
    /// 24-bit flags field. Parsed for completeness; not every box consults it
    /// yet (e.g. edit lists and the `tkhd` enabled bit will).
    #[allow(dead_code)]
    pub(crate) flags: u32,
}

/// Read the version + flags of a full box.
pub(crate) fn read_full_box_header<R: Read>(r: &mut R) -> Result<FullBoxHeader> {
    let version = r.read_u8()?;
    let flags = r.read_u24_be()?;
    Ok(FullBoxHeader { version, flags })
}

/// Read exactly `buf.len()` bytes, returning `Ok(false)` if the stream was
/// already at EOF (zero bytes read) and an error on a partial read.
fn read_full_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    if filled == 0 {
        Ok(false)
    } else if filled == buf.len() {
        Ok(true)
    } else {
        Err(Error::UnexpectedEof("box header".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_32bit_box_header() {
        let mut c = Cursor::new(b"\x00\x00\x00\x18ftyp".to_vec());
        let h = read_box_header(&mut c).unwrap().unwrap();
        assert_eq!(&h.kind, b"ftyp");
        assert_eq!(h.size, 24);
        assert_eq!(h.header_len, 8);
        assert_eq!(h.payload_len(0), 16);
    }

    #[test]
    fn reads_64bit_box_header() {
        let mut data = b"\x00\x00\x00\x01mdat".to_vec();
        data.extend_from_slice(&1_000_000u64.to_be_bytes());
        let mut c = Cursor::new(data);
        let h = read_box_header(&mut c).unwrap().unwrap();
        assert_eq!(&h.kind, b"mdat");
        assert_eq!(h.size, 1_000_000);
        assert_eq!(h.header_len, 16);
    }

    #[test]
    fn clean_eof_returns_none() {
        let mut c = Cursor::new(Vec::new());
        assert!(read_box_header(&mut c).unwrap().is_none());
    }

    #[test]
    fn undersized_box_is_malformed() {
        let mut c = Cursor::new(b"\x00\x00\x00\x04moov".to_vec());
        assert!(read_box_header(&mut c).is_err());
    }
}

//! Low-level helpers for building ISO-BMFF boxes when muxing.
//!
//! These are the write-side counterpart to [`boxes`](super::boxes): tiny
//! functions that assemble a box (or "full box") from its type code and body.
//! Building into `Vec<u8>` keeps the muxer straightforward — sizes are filled
//! in once the body is known, so there is no back-patching.

/// Assemble a box: a 4-byte big-endian size, the 4-byte type, then `body`.
pub(crate) fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(body.len() + 8);
    v.extend_from_slice(&((body.len() as u32 + 8).to_be_bytes()));
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

/// Assemble a "full box": like [`atom`] but prefixing the body with a version
/// byte and 24-bit flags.
pub(crate) fn full_atom(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 4);
    b.push(version);
    b.extend_from_slice(&flags.to_be_bytes()[1..]); // low 3 bytes
    b.extend_from_slice(body);
    atom(kind, &b)
}

/// A growable byte buffer with big-endian integer push helpers, used to build
/// box bodies fluently.
#[derive(Default)]
pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    pub(crate) fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn i32(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub(crate) fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(v);
        self
    }

    /// Push `n` zero bytes.
    pub(crate) fn zeros(&mut self, n: usize) -> &mut Self {
        self.buf.resize(self.buf.len() + n, 0);
        self
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

/// The 3x3 video transformation matrix in 16.16 fixed point — the identity.
pub(crate) const IDENTITY_MATRIX: [u32; 9] =
    [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// Push the identity matrix onto a writer.
pub(crate) fn push_matrix(w: &mut Writer) {
    for v in IDENTITY_MATRIX {
        w.u32(v);
    }
}

/// Encode an ISO-639-2/T language code (three lowercase letters) into the
/// 15-bit packed form used by `mdhd`. Falls back to `und` (undefined).
pub(crate) fn pack_language(lang: Option<&str>) -> u16 {
    let code = lang.unwrap_or("und");
    let bytes: Vec<u8> = code.bytes().take(3).collect();
    if bytes.len() != 3 {
        return pack_language(None);
    }
    let mut packed = 0u16;
    for &b in &bytes {
        let v = b.wrapping_sub(0x60);
        packed = (packed << 5) | u16::from(v & 0x1F);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atom_prefixes_size_and_type() {
        let a = atom(b"free", &[1, 2, 3]);
        assert_eq!(&a[0..4], &[0, 0, 0, 11]);
        assert_eq!(&a[4..8], b"free");
        assert_eq!(&a[8..], &[1, 2, 3]);
    }

    #[test]
    fn full_atom_inserts_version_flags() {
        let a = full_atom(b"mvhd", 0, 0, &[0xAA]);
        // size(4) type(4) version(1) flags(3) body(1) = 13
        assert_eq!(&a[0..4], &[0, 0, 0, 13]);
        assert_eq!(a[8], 0); // version
        assert_eq!(&a[9..12], &[0, 0, 0]); // flags
        assert_eq!(a[12], 0xAA);
    }

    #[test]
    fn language_round_trips() {
        // 'eng' -> packed -> same bits the reader would decode.
        let packed = pack_language(Some("eng"));
        let c1 = (((packed >> 10) & 0x1F) as u8) + 0x60;
        let c2 = (((packed >> 5) & 0x1F) as u8) + 0x60;
        let c3 = ((packed & 0x1F) as u8) + 0x60;
        assert_eq!([c1, c2, c3], *b"eng");
    }
}

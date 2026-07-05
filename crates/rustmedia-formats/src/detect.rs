//! Container-format detection from a stream's leading bytes ("magic").

use std::io::{Read, Seek, SeekFrom};

use rustmedia_core::{ContainerFormat, Result};

/// Number of leading bytes sniffed for format detection.
const PROBE_LEN: usize = 16;

/// Detect the container format of `source` by inspecting its magic bytes.
///
/// The stream position is restored to where it started, so the caller can hand
/// the same source straight to the matching parser. Returns `None` if no known
/// format matches.
///
/// Note that Matroska and WebM share a magic number; both are reported as
/// [`ContainerFormat::Matroska`] here (they use the same parser, which then
/// refines the distinction from the EBML `DocType`).
pub fn detect<R: Read + Seek>(source: &mut R) -> Result<Option<ContainerFormat>> {
    let start = source.stream_position()?;
    let mut buf = [0u8; PROBE_LEN];
    let n = read_up_to(source, &mut buf)?;
    source.seek(SeekFrom::Start(start))?;
    Ok(detect_bytes(&buf[..n]))
}

/// Detect a container format from an in-memory prefix of a file.
///
/// This is the pure, allocation-free core of [`detect`]; useful for testing and
/// for callers that already hold the leading bytes.
#[must_use]
pub fn detect_bytes(buf: &[u8]) -> Option<ContainerFormat> {
    // EBML (Matroska / WebM): 1A 45 DF A3.
    if buf.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(ContainerFormat::Matroska);
    }
    // RIFF WAVE: "RIFF" .... "WAVE".
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WAVE" {
        return Some(ContainerFormat::Wav);
    }
    // FLAC native stream.
    if buf.starts_with(b"fLaC") {
        return Some(ContainerFormat::Flac);
    }
    // Ogg.
    if buf.starts_with(b"OggS") {
        return Some(ContainerFormat::Ogg);
    }
    // ISO-BMFF (MP4/MOV): a `ftyp`/`moov`/`mdat`/... box at offset 4.
    if buf.len() >= 8 && is_bmff_box(&buf[4..8]) {
        // Distinguish QuickTime from MP4 via the ftyp major brand.
        if &buf[4..8] == b"ftyp" && buf.len() >= 12 {
            let brand = &buf[8..12];
            if brand == b"qt  " {
                return Some(ContainerFormat::Mov);
            }
        }
        return Some(ContainerFormat::Mp4);
    }
    // MP3: an ID3v2 tag or an MPEG audio frame sync.
    if buf.starts_with(b"ID3") {
        return Some(ContainerFormat::Mp3);
    }
    if buf.len() >= 2 && buf[0] == 0xFF && (buf[1] & 0xE0) == 0xE0 {
        return Some(ContainerFormat::Mp3);
    }
    None
}

fn is_bmff_box(kind: &[u8]) -> bool {
    matches!(
        kind,
        b"ftyp" | b"moov" | b"mdat" | b"free" | b"skip" | b"wide" | b"pnot" | b"styp"
    )
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_magics() {
        assert_eq!(
            detect_bytes(&[0x1A, 0x45, 0xDF, 0xA3, 0, 0]),
            Some(ContainerFormat::Matroska)
        );
        assert_eq!(
            detect_bytes(b"RIFF\0\0\0\0WAVE"),
            Some(ContainerFormat::Wav)
        );
        assert_eq!(detect_bytes(b"fLaC\0\0"), Some(ContainerFormat::Flac));
        assert_eq!(detect_bytes(b"OggS\0\0"), Some(ContainerFormat::Ogg));
        assert_eq!(detect_bytes(b"ID3\x04\0\0"), Some(ContainerFormat::Mp3));
        assert_eq!(
            detect_bytes(b"\0\0\0\x18ftypmp42"),
            Some(ContainerFormat::Mp4)
        );
        assert_eq!(
            detect_bytes(b"\0\0\0\x18ftypqt  "),
            Some(ContainerFormat::Mov)
        );
        assert_eq!(
            detect_bytes(&[0xFF, 0xFB, 0x90, 0x00]),
            Some(ContainerFormat::Mp3)
        );
        assert_eq!(detect_bytes(b"not media!"), None);
    }
}

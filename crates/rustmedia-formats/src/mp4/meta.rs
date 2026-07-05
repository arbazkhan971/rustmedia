//! Metadata extraction from `moov/udta`: iTunes-style `ilst` tags and Nero
//! `chpl` chapters.

use rustmedia_core::metadata::keys;
use rustmedia_core::{Chapter, Metadata, Timestamp};

use super::boxes::boxes_in;

/// 100-nanosecond ticks per second — the timescale Nero `chpl` chapters use.
const NERO_TIMESCALE: u32 = 10_000_000;

/// Parse a `udta` payload, filling `meta` with any tags and chapters found.
/// Best-effort: malformed sub-structures are skipped rather than propagated.
pub(crate) fn parse_udta(payload: &[u8], meta: &mut Metadata) {
    for (kind, data) in boxes_in(payload) {
        match &kind {
            b"meta" => parse_meta(data, meta),
            b"chpl" => parse_chpl(data, &mut meta.chapters),
            _ => {}
        }
    }
}

/// Parse a `meta` box, which contains an `ilst` of tag atoms.
///
/// The `meta` box is a full box (4-byte version/flags prefix) in ISO/iTunes
/// files but a plain container in QuickTime. We detect which by trying both and
/// keeping whichever exposes recognisable children.
fn parse_meta(payload: &[u8], meta: &mut Metadata) {
    let as_full = payload.get(4..).map(boxes_in).unwrap_or_default();
    let children = if as_full.iter().any(|(k, _)| k == b"ilst" || k == b"hdlr") {
        as_full
    } else {
        boxes_in(payload)
    };

    for (kind, data) in children {
        if &kind == b"ilst" {
            parse_ilst(data, meta);
        }
    }
}

/// Parse an `ilst` (iTunes metadata list): each child atom is a tag whose value
/// lives in a nested `data` box.
fn parse_ilst(payload: &[u8], meta: &mut Metadata) {
    for (atom, data) in boxes_in(payload) {
        let Some(key) = tag_key(&atom) else { continue };
        // The value is inside a `data` box: 4-byte type indicator + 4-byte
        // locale, then the payload.
        let Some((_, data_payload)) = boxes_in(data).into_iter().find(|(k, _)| k == b"data") else {
            continue;
        };
        if data_payload.len() < 8 {
            continue;
        }
        let type_indicator = u32::from_be_bytes([
            data_payload[0],
            data_payload[1],
            data_payload[2],
            data_payload[3],
        ]);
        let value_bytes = &data_payload[8..];

        match key {
            keys::TRACK | keys::DISC => {
                // Binary: reserved(2), index(2), total(2).
                if value_bytes.len() >= 4 {
                    let index = u16::from_be_bytes([value_bytes[2], value_bytes[3]]);
                    let total = value_bytes
                        .get(4..6)
                        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
                    let rendered = if total > 0 {
                        format!("{index}/{total}")
                    } else {
                        index.to_string()
                    };
                    meta.insert(key, rendered);
                }
            }
            _ => {
                // type_indicator 1 == UTF-8 text; treat everything else that is
                // valid UTF-8 as text too (covers the common cases).
                let _ = type_indicator;
                if let Ok(text) = std::str::from_utf8(value_bytes) {
                    let text = text.trim_end_matches('\0');
                    if !text.is_empty() {
                        meta.insert(key, text);
                    }
                }
            }
        }
    }
}

/// Map an iTunes atom code to a normalised metadata key.
fn tag_key(atom: &[u8; 4]) -> Option<&'static str> {
    Some(match atom {
        b"\xA9nam" => keys::TITLE,
        b"\xA9ART" => keys::ARTIST,
        b"aART" => keys::ALBUM_ARTIST,
        b"\xA9alb" => keys::ALBUM,
        b"\xA9wrt" => keys::COMPOSER,
        b"\xA9gen" => keys::GENRE,
        b"\xA9day" => keys::DATE,
        b"\xA9cmt" => keys::COMMENT,
        b"\xA9too" => keys::ENCODER,
        b"cprt" => keys::COPYRIGHT,
        b"trkn" => keys::TRACK,
        b"disk" => keys::DISC,
        _ => return None,
    })
}

/// Parse a Nero `chpl` chapter list (best-effort; layout is only loosely
/// standardised, so bounds failures simply stop parsing).
fn parse_chpl(payload: &[u8], chapters: &mut Vec<Chapter>) {
    // version(1) + flags(3).
    let Some(&version) = payload.first() else {
        return;
    };
    let mut pos = 4usize;
    // Version 1 has a 1-byte reserved field before the count.
    if version == 1 {
        pos += 1;
    }
    let Some(&count) = payload.get(pos) else {
        return;
    };
    pos += 1;

    for _ in 0..count {
        // start time: u64 in 100ns units.
        let Some(time_bytes) = payload.get(pos..pos + 8) else {
            break;
        };
        let start_100ns = u64::from_be_bytes(time_bytes.try_into().unwrap());
        pos += 8;
        let Some(&title_len) = payload.get(pos) else {
            break;
        };
        pos += 1;
        let Some(title_bytes) = payload.get(pos..pos + title_len as usize) else {
            break;
        };
        pos += title_len as usize;
        let title = String::from_utf8_lossy(title_bytes).into_owned();
        chapters.push(Chapter {
            start: Timestamp::new(start_100ns as i64, NERO_TIMESCALE),
            end: None,
            title,
        });
    }
}

//! Parsing of `stsd` sample entries into codecs and track parameters.
//!
//! The Sample Description box (`stsd`) holds one entry per coded format used by
//! a track. Each entry starts with a four-character codec code and is followed
//! by codec-specific fields and child boxes (`avcC`, `esds`, `dOps`, …) that
//! carry the initialisation data needed to decode — or, in RustMedia's case, to
//! remux — the stream.

use rustmedia_core::{
    AudioParameters, Codec, SubtitleParameters, TrackParameters, VideoParameters,
};

use super::boxes::{boxes_in, FourCc};

/// Which family of sample entry to expect, taken from the track's `hdlr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandlerKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

/// The distilled result of parsing an `stsd` entry.
pub(crate) struct StsdInfo {
    pub codec: Codec,
    pub parameters: TrackParameters,
    /// Codec initialisation data (extradata) needed to remux the track.
    pub codec_private: Option<Vec<u8>>,
}

/// Parse the payload of an `stsd` box (after its own box header) and return the
/// codec and parameters of its first sample entry.
pub(crate) fn parse_stsd(payload: &[u8], handler: HandlerKind) -> Option<StsdInfo> {
    // Full-box header (4) + entry_count (4). The first entry follows.
    if payload.len() < 16 {
        return None;
    }
    let entry = &payload[8..];
    // Entry box header: size (4) + format code (4).
    if entry.len() < 8 {
        return None;
    }
    let size = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
    let format: FourCc = [entry[4], entry[5], entry[6], entry[7]];
    let entry_end = size.clamp(8, entry.len());
    let body = &entry[8..entry_end];

    match handler {
        HandlerKind::Video => Some(parse_video_entry(&format, body)),
        HandlerKind::Audio => Some(parse_audio_entry(&format, body)),
        HandlerKind::Subtitle => Some(parse_subtitle_entry(&format)),
        HandlerKind::Other => Some(StsdInfo {
            codec: Codec::Other(fourcc_lossy(&format)),
            parameters: TrackParameters::None,
            codec_private: None,
        }),
    }
}

fn parse_video_entry(format: &FourCc, body: &[u8]) -> StsdInfo {
    // Visual sample entry fixed fields are 70 bytes (after the 8-byte base of
    // reserved + data_reference_index that `body` already starts with... which
    // itself is 8 bytes). Layout of `body`:
    //   [0..6]  reserved   [6..8]  data_reference_index
    //   [8..10] predefined [10..12] reserved [12..24] predefined[3]
    //   [24..26] width     [26..28] height    [74..76] depth   [78..] children
    // The `depth` field (usually 24) describes colour depth, not the codec's
    // per-component bit depth, so we do not derive `bit_depth` from it — that
    // must come from the codec config (SPS / vpcC / av1C) if needed.
    let width = read_u16(body, 24);
    let height = read_u16(body, 26);
    let children = body.get(78..).unwrap_or(&[]);

    let mut codec = video_codec_from_fourcc(format);
    let mut codec_private = None;
    for (kind, data) in boxes_in(children) {
        match &kind {
            b"avcC" => {
                codec = Codec::H264;
                codec_private = Some(data.to_vec());
            }
            b"hvcC" => {
                codec = Codec::H265;
                codec_private = Some(data.to_vec());
            }
            b"av1C" => {
                codec = Codec::Av1;
                codec_private = Some(data.to_vec());
            }
            b"vpcC" => {
                if codec != Codec::Vp8 {
                    codec = Codec::Vp9;
                }
                codec_private = Some(data.to_vec());
            }
            _ => {}
        }
    }

    StsdInfo {
        codec,
        parameters: TrackParameters::Video(VideoParameters {
            width: u32::from(width),
            height: u32::from(height),
            frame_rate: None, // filled in from the sample table by the demuxer
            display_aspect_ratio: None,
            bit_depth: None, // derived from codec config later, not the depth field
        }),
        codec_private,
    }
}

fn parse_audio_entry(format: &FourCc, body: &[u8]) -> StsdInfo {
    // Audio sample entry `body` layout:
    //   [0..6] reserved [6..8] data_reference_index
    //   [8..10] version [10..12] revision [12..16] vendor
    //   [16..18] channelcount [18..20] samplesize [20..22] predefined
    //   [22..24] reserved [24..28] samplerate (16.16)
    let version = read_u16(body, 8);
    let channels = read_u16(body, 16).max(1);
    let sample_size = read_u16(body, 18);
    let sample_rate = u32::from(read_u16(body, 24)); // integer part of 16.16

    // Child boxes begin after the version-dependent fixed part.
    let child_start = match version {
        1 => 44,
        2 => 64,
        _ => 28,
    };
    let children = body.get(child_start..).unwrap_or(&[]);

    let mut codec = audio_codec_from_fourcc(format);
    let mut codec_private = None;
    for (kind, data) in boxes_in(children) {
        match &kind {
            b"esds" => {
                if let Some((oti, dsi)) = parse_esds(data) {
                    codec = codec_from_object_type(oti).unwrap_or(codec);
                    codec_private = dsi;
                }
            }
            b"dOps" => {
                codec = Codec::Opus;
                codec_private = Some(data.to_vec());
            }
            b"dfLa" => {
                codec = Codec::Flac;
                codec_private = Some(data.to_vec());
            }
            b"alac" => {
                codec = Codec::Alac;
                codec_private = Some(data.to_vec());
            }
            _ => {}
        }
    }

    let bits_per_sample = if codec.is_pcm() && sample_size != 0 {
        Some(sample_size)
    } else {
        None
    };

    StsdInfo {
        codec,
        parameters: TrackParameters::Audio(AudioParameters {
            sample_rate,
            channels,
            bits_per_sample,
        }),
        codec_private,
    }
}

fn parse_subtitle_entry(format: &FourCc) -> StsdInfo {
    let codec = match format {
        b"tx3g" => Codec::MovText,
        b"wvtt" => Codec::WebVtt,
        b"sbtt" | b"text" => Codec::Other("text".to_string()),
        other => Codec::Other(fourcc_lossy(other)),
    };
    StsdInfo {
        codec,
        parameters: TrackParameters::Subtitle(SubtitleParameters { is_bitmap: false }),
        codec_private: None,
    }
}

fn video_codec_from_fourcc(f: &FourCc) -> Codec {
    match f {
        b"avc1" | b"avc3" => Codec::H264,
        b"hev1" | b"hvc1" => Codec::H265,
        b"av01" => Codec::Av1,
        b"vp08" => Codec::Vp8,
        b"vp09" => Codec::Vp9,
        b"mp4v" => Codec::Mpeg4Visual,
        b"ap4h" | b"apcn" | b"apch" | b"apcs" | b"apco" | b"ap4x" => Codec::ProRes,
        other => Codec::Other(fourcc_lossy(other)),
    }
}

fn audio_codec_from_fourcc(f: &FourCc) -> Codec {
    match f {
        b"mp4a" => Codec::Aac, // refined by esds objectTypeIndication
        b".mp3" | b"mp3 " => Codec::Mp3,
        b"ac-3" => Codec::Ac3,
        b"ec-3" => Codec::Eac3,
        b"Opus" => Codec::Opus,
        b"fLaC" => Codec::Flac,
        b"alac" => Codec::Alac,
        b"twos" => Codec::PcmS16Be,
        b"sowt" => Codec::PcmS16Le,
        b"in24" => Codec::PcmS24Le,
        b"fl32" => Codec::PcmF32Le,
        b"raw " => Codec::PcmU8,
        other => Codec::Other(fourcc_lossy(other)),
    }
}

/// Map an MPEG-4 `objectTypeIndication` to a codec.
fn codec_from_object_type(oti: u8) -> Option<Codec> {
    match oti {
        0x40 | 0x66 | 0x67 | 0x68 => Some(Codec::Aac),
        0x69 | 0x6B => Some(Codec::Mp3),
        _ => None,
    }
}

/// Minimal `esds` parser: returns the `objectTypeIndication` and the
/// `DecoderSpecificInfo` bytes (the AAC `AudioSpecificConfig`), if present.
fn parse_esds(data: &[u8]) -> Option<(u8, Option<Vec<u8>>)> {
    // Skip the full-box version+flags.
    let mut pos = 4usize;
    // ES_Descriptor (tag 0x03).
    let (tag, len, next) = read_descriptor(data, pos)?;
    if tag != 0x03 {
        return None;
    }
    pos = next;
    let es_end = (pos + len).min(data.len());
    // ES_Descriptor: ES_ID (2) + flags (1), plus optional dependency/URL/OCR.
    let flags = *data.get(pos + 2)?;
    pos += 3;
    if flags & 0x80 != 0 {
        pos += 2; // dependsOn ES_ID
    }
    if flags & 0x40 != 0 {
        let url_len = *data.get(pos)? as usize;
        pos += 1 + url_len;
    }
    if flags & 0x20 != 0 {
        pos += 2; // OCR ES_ID
    }
    // DecoderConfigDescriptor (tag 0x04).
    let (tag, dcd_len, next) = read_descriptor(data, pos)?;
    if tag != 0x04 {
        return None;
    }
    let oti = *data.get(next)?;
    let dcd_end = (next + dcd_len).min(es_end);
    // Skip objectTypeIndication (1) + streamType/bufferSize (4) + bitrates (8).
    let mut p = next + 13;
    let mut dsi = None;
    // Optional DecoderSpecificInfo (tag 0x05).
    if p < dcd_end {
        if let Some((tag, dsi_len, dsi_start)) = read_descriptor(data, p) {
            if tag == 0x05 {
                let end = (dsi_start + dsi_len).min(data.len());
                if dsi_start <= end {
                    dsi = Some(data[dsi_start..end].to_vec());
                }
            }
        }
        p = p.max(dcd_end);
    }
    let _ = p;
    Some((oti, dsi))
}

/// Read an MPEG-4 descriptor header: a one-byte tag followed by an "expandable"
/// length (7 bits per byte, high bit = continue). Returns `(tag, length,
/// payload_start)`.
fn read_descriptor(data: &[u8], pos: usize) -> Option<(u8, usize, usize)> {
    let tag = *data.get(pos)?;
    let mut p = pos + 1;
    let mut len = 0usize;
    for _ in 0..4 {
        let b = *data.get(p)?;
        p += 1;
        len = (len << 7) | usize::from(b & 0x7F);
        if b & 0x80 == 0 {
            break;
        }
    }
    Some((tag, len, p))
}

fn read_u16(data: &[u8], off: usize) -> u16 {
    match (data.get(off), data.get(off + 1)) {
        (Some(&a), Some(&b)) => u16::from_be_bytes([a, b]),
        _ => 0,
    }
}

fn fourcc_lossy(f: &FourCc) -> String {
    f.iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_expandable_descriptor_length() {
        // tag 0x05, length 0x81 0x02 = (1<<7)|2 = 130, payload starts at +3.
        let data = [0x05, 0x81, 0x02, 0xAA];
        let (tag, len, start) = read_descriptor(&data, 0).unwrap();
        assert_eq!(tag, 0x05);
        assert_eq!(len, 130);
        assert_eq!(start, 3);
    }

    #[test]
    fn object_type_distinguishes_aac_and_mp3() {
        assert_eq!(codec_from_object_type(0x40), Some(Codec::Aac));
        assert_eq!(codec_from_object_type(0x6B), Some(Codec::Mp3));
        assert_eq!(codec_from_object_type(0x20), None);
    }
}

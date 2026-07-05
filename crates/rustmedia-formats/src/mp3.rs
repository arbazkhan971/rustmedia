//! Native MP3 (MPEG-1/2/2.5 Audio Layer III) demuxer.
//!
//! An MP3 file is a bare stream of MPEG audio frames, optionally wrapped in
//! ID3v2 (front) and ID3v1 (back) metadata tags. RustMedia skips the ID3 tags
//! (reading their text frames as metadata), locks onto the first valid frame to
//! learn the sample rate, channel count, and bitrate, reads a Xing/Info header
//! for an exact VBR duration when present, and then serves one packet per MPEG
//! frame.

use std::io::{Read, Seek};
use std::time::Duration;

use rustmedia_core::metadata::keys;
use rustmedia_core::{
    AudioParameters, Codec, ContainerFormat, Error, MediaType, Metadata, Packet, Result, Timestamp,
    Track, TrackParameters,
};
use rustmedia_io::{ReadBytes, Source};

use crate::demux::Demuxer;

/// A demuxer for MP3 elementary streams.
pub struct Mp3Demuxer<R: Read + Seek> {
    reader: R,
    track: Track,
    metadata: Metadata,
    duration: Option<Duration>,
    sample_rate: u32,
    samples_per_frame: u32,
    /// Byte offset of the first audio frame (after any ID3v2 tag).
    audio_start: u64,
    audio_end: u64,
    /// Current read offset and running sample count for packet timestamps.
    pos: u64,
    samples_emitted: i64,
}

impl<R: Read + Seek> Mp3Demuxer<R> {
    /// Parse an MP3 stream's tags and first frame.
    ///
    /// # Errors
    /// Returns [`Error::Malformed`] if no MPEG audio frame can be found.
    pub fn new(mut reader: R) -> Result<Self> {
        let file_size = reader.size()?;
        reader.seek_to(0)?;

        let mut metadata = Metadata::new();

        // ID3v2 tag at the front, if present.
        let mut audio_start = 0u64;
        let header = reader.read_vec(10.min(file_size as usize))?;
        if header.len() == 10 && &header[0..3] == b"ID3" {
            let tag_size = syncsafe_u32(&header[6..10]);
            let total = u64::from(tag_size) + 10;
            reader.seek_to(0)?;
            let tag = reader.read_vec((total.min(file_size)) as usize)?;
            parse_id3v2(&tag, &mut metadata);
            audio_start = total;
        }

        // ID3v1 tag at the very end (128 bytes), used only to fill gaps.
        let mut audio_end = file_size;
        if file_size >= 128 {
            reader.seek_to(file_size - 128)?;
            let tail = reader.read_vec(128)?;
            if &tail[0..3] == b"TAG" {
                audio_end = file_size - 128;
                parse_id3v1(&tail, &mut metadata);
            }
        }

        // Find and parse the first valid frame header.
        let first = find_first_frame(&mut reader, audio_start, audio_end)?;
        audio_start = first.offset;

        // Read the frame body to look for a Xing/Info VBR header.
        reader.seek_to(first.offset)?;
        let frame_bytes = reader.read_vec((first.header.frame_len as usize).min(2048))?;
        let xing = XingHeader::find(&frame_bytes, &first.header);

        let sample_rate = first.header.sample_rate;
        let samples_per_frame = first.header.samples_per_frame;
        let audio_len = audio_end.saturating_sub(audio_start);

        // Duration: prefer Xing's exact frame count, else estimate from bitrate.
        let duration = if let Some(frames) = xing.as_ref().and_then(|x| x.frames) {
            let total_samples = u64::from(frames) * u64::from(samples_per_frame);
            Some(Timestamp::new(total_samples as i64, sample_rate).to_duration())
        } else if first.header.bitrate > 0 {
            let secs = audio_len as f64 * 8.0 / f64::from(first.header.bitrate);
            Some(Duration::from_secs_f64(secs))
        } else {
            None
        };

        // Average bitrate: exact from Xing bytes+frames, else the header value.
        let bitrate = match xing
            .as_ref()
            .and_then(|x| x.average_bitrate(sample_rate, samples_per_frame))
        {
            Some(b) => Some(b),
            None => (first.header.bitrate > 0).then_some(u64::from(first.header.bitrate)),
        };

        let track = Track {
            id: 1,
            codec: Codec::Mp3,
            media_type: MediaType::Audio,
            timescale: sample_rate,
            duration: duration.map(|d| {
                Timestamp::new(
                    (d.as_secs_f64() * f64::from(sample_rate)) as i64,
                    sample_rate,
                )
            }),
            language: None,
            name: None,
            bitrate,
            codec_private: None,
            parameters: TrackParameters::Audio(AudioParameters {
                sample_rate,
                channels: first.header.channels,
                bits_per_sample: None,
            }),
        };

        Ok(Mp3Demuxer {
            reader,
            track,
            metadata,
            duration,
            sample_rate,
            samples_per_frame,
            audio_start,
            audio_end,
            pos: audio_start,
            samples_emitted: 0,
        })
    }
}

impl<R: Read + Seek> Demuxer for Mp3Demuxer<R> {
    fn format(&self) -> ContainerFormat {
        ContainerFormat::Mp3
    }

    fn tracks(&self) -> &[Track] {
        std::slice::from_ref(&self.track)
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn read_packet(&mut self) -> Result<Option<Packet>> {
        loop {
            if self.pos + 4 > self.audio_end {
                return Ok(None);
            }
            self.reader.seek_to(self.pos)?;
            let mut head = [0u8; 4];
            if self.reader.read_exact(&mut head).is_err() {
                return Ok(None);
            }
            let Some(header) = FrameHeader::parse(u32::from_be_bytes(head)) else {
                // Not a frame sync here; advance one byte and resync.
                self.pos += 1;
                continue;
            };
            let frame_len = u64::from(header.frame_len);
            if self.pos + frame_len > self.audio_end {
                // Truncated final frame.
                return Ok(None);
            }
            self.reader.seek_to(self.pos)?;
            let data = self.reader.read_vec(header.frame_len as usize)?;
            let pts = self.samples_emitted;
            self.samples_emitted += i64::from(header.samples_per_frame);
            self.pos += frame_len;

            return Ok(Some(Packet {
                track_id: 1,
                dts: Some(pts),
                pts: Some(pts),
                duration: Some(u64::from(header.samples_per_frame)),
                is_keyframe: true,
                data,
            }));
        }
    }

    fn seek(&mut self, target: Duration) -> Result<()> {
        // Estimate the byte position assuming a roughly constant frame size.
        let audio_len = self.audio_end.saturating_sub(self.audio_start) as f64;
        let fraction = match self.duration {
            Some(d) if d.as_secs_f64() > 0.0 => {
                (target.as_secs_f64() / d.as_secs_f64()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let approx = self.audio_start + (audio_len * fraction) as u64;

        // Snap forward to the next frame sync so packets stay aligned.
        self.reader.seek_to(approx)?;
        let scan_start = approx;
        let found = scan_for_sync(&mut self.reader, scan_start, self.audio_end)?;
        self.pos = found.unwrap_or(self.audio_start);
        self.samples_emitted = (target.as_secs_f64() * f64::from(self.sample_rate)) as i64
            - i64::from(self.samples_per_frame);
        if self.samples_emitted < 0 {
            self.samples_emitted = 0;
        }
        Ok(())
    }
}

// -------------------------------------------------------------------------
// Frame headers
// -------------------------------------------------------------------------

/// A decoded MPEG audio frame header.
struct FrameHeader {
    bitrate: u32, // bits per second (0 = free format / unknown)
    sample_rate: u32,
    channels: u16,
    samples_per_frame: u32,
    frame_len: u32,
    is_mpeg1: bool,
}

impl FrameHeader {
    /// Parse a 32-bit frame header, returning `None` if it is not a valid
    /// Layer III sync.
    fn parse(header: u32) -> Option<FrameHeader> {
        // Frame sync: 11 set bits.
        if header & 0xFFE0_0000 != 0xFFE0_0000 {
            return None;
        }
        let version_bits = (header >> 19) & 0x3;
        let layer_bits = (header >> 17) & 0x3;
        let bitrate_index = ((header >> 12) & 0xF) as usize;
        let sr_index = ((header >> 10) & 0x3) as usize;
        let padding = (header >> 9) & 0x1;
        let channel_mode = (header >> 6) & 0x3;

        // Only Layer III (layer_bits == 0b01) is supported here.
        if layer_bits != 0b01 || version_bits == 0b01 {
            return None;
        }
        if bitrate_index == 0 || bitrate_index == 15 || sr_index == 3 {
            return None;
        }

        let is_mpeg1 = version_bits == 0b11;
        let bitrate = if is_mpeg1 {
            BITRATE_MPEG1_L3[bitrate_index]
        } else {
            BITRATE_MPEG2_L3[bitrate_index]
        } * 1000;

        let sample_rate = match version_bits {
            0b11 => SR_MPEG1[sr_index],
            0b10 => SR_MPEG2[sr_index],
            _ => SR_MPEG25[sr_index], // 0b00 = MPEG 2.5
        };

        let samples_per_frame = if is_mpeg1 { 1152 } else { 576 };
        let channels = if channel_mode == 0b11 { 1 } else { 2 };

        // Layer III frame length in bytes.
        let frame_len = (u64::from(samples_per_frame) / 8 * u64::from(bitrate)
            / u64::from(sample_rate)
            + u64::from(padding)) as u32;
        if frame_len < 4 {
            return None;
        }

        Some(FrameHeader {
            bitrate,
            sample_rate,
            channels,
            samples_per_frame,
            frame_len,
            is_mpeg1,
        })
    }

    /// Byte offset of a Xing/Info tag within a frame: 4-byte header + side info.
    fn xing_offset(&self) -> usize {
        let side_info = match (self.is_mpeg1, self.channels) {
            (true, 1) => 17,
            (true, _) => 32,
            (false, 1) => 9,
            (false, _) => 17,
        };
        4 + side_info
    }
}

const BITRATE_MPEG1_L3: [u32; 16] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
];
const BITRATE_MPEG2_L3: [u32; 16] = [
    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
];
const SR_MPEG1: [u32; 3] = [44100, 48000, 32000];
const SR_MPEG2: [u32; 3] = [22050, 24000, 16000];
const SR_MPEG25: [u32; 3] = [11025, 12000, 8000];

/// Locate the first valid frame by scanning for a sync word, then confirming a
/// second frame follows at the computed offset (guards against false syncs).
struct FirstFrame {
    offset: u64,
    header: FrameHeader,
}

fn find_first_frame<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> Result<FirstFrame> {
    let mut pos = start;
    while pos + 4 <= end {
        reader.seek_to(pos)?;
        let mut head = [0u8; 4];
        if reader.read_exact(&mut head).is_err() {
            break;
        }
        if let Some(header) = FrameHeader::parse(u32::from_be_bytes(head)) {
            // Confirm a plausible next frame to reject random 0xFFE bytes.
            let next = pos + u64::from(header.frame_len);
            if next + 4 <= end {
                reader.seek_to(next)?;
                let mut next_head = [0u8; 4];
                if reader.read_exact(&mut next_head).is_ok()
                    && FrameHeader::parse(u32::from_be_bytes(next_head)).is_some()
                {
                    return Ok(FirstFrame {
                        offset: pos,
                        header,
                    });
                }
            } else {
                // Near EOF: accept the single frame we found.
                return Ok(FirstFrame {
                    offset: pos,
                    header,
                });
            }
        }
        pos += 1;
    }
    Err(Error::malformed("mp3", "no MPEG audio frame found"))
}

/// Scan for the next frame sync at or after `start`, returning its offset.
fn scan_for_sync<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> Result<Option<u64>> {
    let mut pos = start;
    while pos + 4 <= end {
        reader.seek_to(pos)?;
        let mut head = [0u8; 4];
        if reader.read_exact(&mut head).is_err() {
            break;
        }
        if FrameHeader::parse(u32::from_be_bytes(head)).is_some() {
            return Ok(Some(pos));
        }
        pos += 1;
    }
    Ok(None)
}

/// A Xing / Info VBR header from the first frame.
struct XingHeader {
    frames: Option<u32>,
    bytes: Option<u32>,
}

impl XingHeader {
    fn find(frame: &[u8], header: &FrameHeader) -> Option<XingHeader> {
        let off = header.xing_offset();
        let tag = frame.get(off..off + 4)?;
        if tag != b"Xing" && tag != b"Info" {
            return None;
        }
        let flags = u32::from_be_bytes(frame.get(off + 4..off + 8)?.try_into().ok()?);
        let mut p = off + 8;
        let mut frames = None;
        let mut bytes = None;
        if flags & 0x1 != 0 {
            frames = Some(u32::from_be_bytes(frame.get(p..p + 4)?.try_into().ok()?));
            p += 4;
        }
        if flags & 0x2 != 0 {
            bytes = Some(u32::from_be_bytes(frame.get(p..p + 4)?.try_into().ok()?));
        }
        Some(XingHeader { frames, bytes })
    }

    fn average_bitrate(&self, sample_rate: u32, samples_per_frame: u32) -> Option<u64> {
        let frames = u64::from(self.frames?);
        let bytes = u64::from(self.bytes?);
        let total_samples = frames * u64::from(samples_per_frame);
        if total_samples == 0 {
            return None;
        }
        let secs = total_samples as f64 / f64::from(sample_rate);
        Some((bytes as f64 * 8.0 / secs) as u64)
    }
}

// -------------------------------------------------------------------------
// ID3 metadata
// -------------------------------------------------------------------------

/// Decode a 28-bit "sync-safe" integer (7 bits per byte).
fn syncsafe_u32(b: &[u8]) -> u32 {
    (u32::from(b[0]) << 21) | (u32::from(b[1]) << 14) | (u32::from(b[2]) << 7) | u32::from(b[3])
}

/// Parse an ID3v2 tag's text frames into metadata.
fn parse_id3v2(tag: &[u8], metadata: &mut Metadata) {
    if tag.len() < 10 {
        return;
    }
    let major = tag[3];
    let mut pos = 10usize;
    let end = tag.len();

    while pos + 10 <= end {
        let id = &tag[pos..pos + 4];
        if id == [0, 0, 0, 0] {
            break; // padding
        }
        let size = if major >= 4 {
            syncsafe_u32(&tag[pos + 4..pos + 8]) as usize
        } else {
            u32::from_be_bytes([tag[pos + 4], tag[pos + 5], tag[pos + 6], tag[pos + 7]]) as usize
        };
        pos += 10;
        if size == 0 || pos + size > end {
            break;
        }
        let body = &tag[pos..pos + size];
        pos += size;

        let id_arr: [u8; 4] = [id[0], id[1], id[2], id[3]];
        if let Some(key) = id3v2_key(&id_arr) {
            if let Some(text) = decode_id3_text(body) {
                let text = text.trim().to_string();
                if !text.is_empty() {
                    metadata.insert(key, text);
                }
            }
        }
    }
}

/// Decode an ID3v2 text-frame body: a leading encoding byte then the text.
fn decode_id3_text(body: &[u8]) -> Option<String> {
    let (&encoding, rest) = body.split_first()?;
    let text = match encoding {
        0 => rest.iter().map(|&b| b as char).collect(), // ISO-8859-1
        1 | 2 => decode_utf16(rest),                    // UTF-16 (with/without BOM)
        // 3 is UTF-8; anything else we best-effort as UTF-8 too.
        _ => String::from_utf8_lossy(rest).into_owned(),
    };
    Some(text.trim_end_matches('\0').to_string())
}

fn decode_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = if bytes.starts_with(&[0xFF, 0xFE]) {
        bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect()
    } else {
        bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect()
    };
    String::from_utf16_lossy(&units)
}

fn id3v2_key(id: &[u8; 4]) -> Option<&'static str> {
    Some(match id {
        b"TIT2" => keys::TITLE,
        b"TPE1" => keys::ARTIST,
        b"TPE2" => keys::ALBUM_ARTIST,
        b"TALB" => keys::ALBUM,
        b"TCON" => keys::GENRE,
        b"TYER" | b"TDRC" => keys::DATE,
        b"TRCK" => keys::TRACK,
        b"TPOS" => keys::DISC,
        b"TCOM" => keys::COMPOSER,
        b"TENC" | b"TSSE" => keys::ENCODER,
        b"TCOP" => keys::COPYRIGHT,
        _ => return None,
    })
}

/// Parse a 128-byte ID3v1 tag, filling only tags not already set.
fn parse_id3v1(tag: &[u8], metadata: &mut Metadata) {
    let field = |range: std::ops::Range<usize>| -> Option<String> {
        let s: String = tag[range]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    if metadata.title().is_none() {
        if let Some(v) = field(3..33) {
            metadata.insert(keys::TITLE, v);
        }
    }
    if metadata.artist().is_none() {
        if let Some(v) = field(33..63) {
            metadata.insert(keys::ARTIST, v);
        }
    }
    if metadata.album().is_none() {
        if let Some(v) = field(63..93) {
            metadata.insert(keys::ALBUM, v);
        }
    }
}

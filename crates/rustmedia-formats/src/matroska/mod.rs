//! Native Matroska / WebM demuxer (EBML).
//!
//! Parses the EBML header (to tell Matroska from WebM), the `Info` and `Tracks`
//! elements, and streams packets out of `Cluster` `SimpleBlock`s and
//! `BlockGroup`s — including Xiph, EBML, and fixed-size lacing. Timestamps are
//! reported in a per-track timescale derived from the segment `TimestampScale`.

mod ebml;
mod ids;

use std::collections::VecDeque;
use std::io::{Read, Seek};
use std::time::Duration;

use rustmedia_core::{
    AudioParameters, Codec, ContainerFormat, Error, MediaType, Metadata, Packet, Rational, Result,
    Timestamp, Track, TrackParameters, VideoParameters,
};
use rustmedia_io::{ReadBytes, Source};

use crate::demux::Demuxer;
use ebml::{read_element, read_float, read_string, read_uint};
#[allow(clippy::wildcard_imports)] // a private module of element-ID constants
use ids::*;

/// Default `TimestampScale`: one millisecond (1,000,000 ns) per tick.
const DEFAULT_TIMESTAMP_SCALE: u64 = 1_000_000;

/// A demuxer for Matroska (`.mkv`) and WebM (`.webm`) files.
pub struct MatroskaDemuxer<R: Read + Seek> {
    reader: R,
    format: ContainerFormat,
    tracks: Vec<Track>,
    metadata: Metadata,
    duration: Option<Duration>,
    /// `1e9 / TimestampScale` — the timescale all packet timestamps use.
    timescale: u32,
    segment_end: u64,
    /// Cursor over the segment's top-level children (clusters).
    pos: u64,
    /// Frames decoded from the current cluster, awaiting delivery.
    pending: VecDeque<Packet>,
}

impl<R: Read + Seek> MatroskaDemuxer<R> {
    /// Parse a Matroska/WebM file's header and track list.
    ///
    /// # Errors
    /// Returns [`Error::Malformed`] if the EBML structure is invalid or the
    /// `Segment`/`Tracks` elements are missing.
    pub fn new(mut reader: R) -> Result<Self> {
        let file_size = reader.size()?;
        reader.seek_to(0)?;

        // EBML header.
        let header = read_element(&mut reader)?;
        if header.id != EBML_HEADER {
            return Err(Error::malformed("matroska", "missing EBML header"));
        }
        let header_start = reader.stream_position()?;
        let header_size = header.size.unwrap_or(0);
        let doctype = read_doctype(&mut reader, header_start, header_start + header_size)?;
        let format = if doctype == "webm" {
            ContainerFormat::WebM
        } else {
            ContainerFormat::Matroska
        };

        // Segment.
        reader.seek_to(header_start + header_size)?;
        let segment = read_element(&mut reader)?;
        if segment.id != SEGMENT {
            return Err(Error::malformed("matroska", "missing Segment element"));
        }
        let segment_start = reader.stream_position()?;
        let segment_end = segment
            .size
            .map_or(file_size, |s| (segment_start + s).min(file_size));

        let mut demuxer = MatroskaDemuxer {
            reader,
            format,
            tracks: Vec::new(),
            metadata: Metadata::new(),
            duration: None,
            timescale: 1000,
            segment_end,
            pos: segment_start,
            pending: VecDeque::new(),
        };
        demuxer.parse_headers(segment_start)?;
        Ok(demuxer)
    }

    /// Walk the segment's top-level children up to the first cluster, parsing
    /// `Info` and `Tracks`. Leaves `pos` at the first cluster for streaming.
    fn parse_headers(&mut self, segment_start: u64) -> Result<()> {
        let mut timestamp_scale = DEFAULT_TIMESTAMP_SCALE;
        let mut duration_ticks = 0f64;
        let mut p = segment_start;

        while let Some((id, body_start, size)) = self.next_child(p, self.segment_end)? {
            match id {
                INFO => self.parse_info(
                    body_start,
                    body_start + size,
                    &mut timestamp_scale,
                    &mut duration_ticks,
                )?,
                TRACKS => self.parse_tracks(body_start, body_start + size)?,
                CLUSTER => {
                    self.pos = p;
                    break;
                }
                _ => {}
            }
            p = body_start + size;
            if p >= self.segment_end {
                self.pos = self.segment_end;
                break;
            }
        }

        self.timescale = u32::try_from(1_000_000_000 / timestamp_scale.max(1)).unwrap_or(1000);
        if duration_ticks > 0.0 {
            // Duration is expressed in TimestampScale units == track ticks.
            let secs = duration_ticks * timestamp_scale as f64 / 1e9;
            self.duration = Some(Duration::from_secs_f64(secs));
            let ticks = duration_ticks as i64;
            for track in &mut self.tracks {
                track.duration = Some(Timestamp::new(ticks, self.timescale));
            }
        }
        Ok(())
    }

    fn parse_info(
        &mut self,
        start: u64,
        end: u64,
        timestamp_scale: &mut u64,
        duration: &mut f64,
    ) -> Result<()> {
        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            match id {
                TIMESTAMP_SCALE => {
                    self.reader.seek_to(body_start)?;
                    *timestamp_scale = read_uint(&mut self.reader, size as usize)?.max(1);
                }
                DURATION => {
                    self.reader.seek_to(body_start)?;
                    *duration = read_float(&mut self.reader, size as usize)?;
                }
                TITLE => {
                    self.reader.seek_to(body_start)?;
                    let title = read_string(&mut self.reader, size as usize)?;
                    if !title.is_empty() {
                        self.metadata
                            .insert(rustmedia_core::metadata::keys::TITLE, title);
                    }
                }
                WRITING_APP | MUXING_APP => {
                    self.reader.seek_to(body_start)?;
                    let app = read_string(&mut self.reader, size as usize)?;
                    if !app.is_empty()
                        && self
                            .metadata
                            .get(rustmedia_core::metadata::keys::ENCODER)
                            .is_none()
                    {
                        self.metadata
                            .insert(rustmedia_core::metadata::keys::ENCODER, app);
                    }
                }
                _ => {}
            }
            p = body_start + size;
        }
        Ok(())
    }

    fn parse_tracks(&mut self, start: u64, end: u64) -> Result<()> {
        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            if id == TRACK_ENTRY {
                if let Some(track) = self.parse_track_entry(body_start, body_start + size)? {
                    self.tracks.push(track);
                }
            }
            p = body_start + size;
        }
        Ok(())
    }

    fn parse_track_entry(&mut self, start: u64, end: u64) -> Result<Option<Track>> {
        let mut number = 0u64;
        let mut track_type = 0u64;
        let mut codec_id = String::new();
        let mut codec_private = None;
        let mut name = None;
        let mut language = None;
        let mut default_duration = 0u64;
        let mut video: Option<(u32, u32)> = None;
        let mut audio: Option<(f64, u16, Option<u16>)> = None;

        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            match id {
                TRACK_NUMBER => {
                    self.reader.seek_to(body_start)?;
                    number = read_uint(&mut self.reader, size as usize)?;
                }
                TRACK_TYPE => {
                    self.reader.seek_to(body_start)?;
                    track_type = read_uint(&mut self.reader, size as usize)?;
                }
                CODEC_ID => {
                    self.reader.seek_to(body_start)?;
                    codec_id = read_string(&mut self.reader, size as usize)?;
                }
                CODEC_PRIVATE => {
                    self.reader.seek_to(body_start)?;
                    codec_private = Some(self.reader.read_vec(size as usize)?);
                }
                TRACK_NAME => {
                    self.reader.seek_to(body_start)?;
                    name = Some(read_string(&mut self.reader, size as usize)?);
                }
                LANGUAGE => {
                    self.reader.seek_to(body_start)?;
                    language = Some(read_string(&mut self.reader, size as usize)?);
                }
                DEFAULT_DURATION => {
                    self.reader.seek_to(body_start)?;
                    default_duration = read_uint(&mut self.reader, size as usize)?;
                }
                VIDEO => video = Some(self.parse_video(body_start, body_start + size)?),
                AUDIO => audio = Some(self.parse_audio(body_start, body_start + size)?),
                _ => {}
            }
            p = body_start + size;
        }

        if number == 0 {
            return Ok(None);
        }

        let media_type = match track_type {
            1 => MediaType::Video,
            2 => MediaType::Audio,
            0x11 => MediaType::Subtitle,
            _ => MediaType::Data,
        };
        let codec = codec_from_id(&codec_id);

        let frame_rate =
            (default_duration > 0).then(|| Rational::new(1_000_000_000, default_duration as i64));

        let parameters = match (media_type, video, audio) {
            (MediaType::Video, Some((w, h)), _) => TrackParameters::Video(VideoParameters {
                width: w,
                height: h,
                frame_rate,
                display_aspect_ratio: None,
                bit_depth: None,
            }),
            (MediaType::Audio, _, Some((rate, channels, bits))) => {
                TrackParameters::Audio(AudioParameters {
                    sample_rate: rate as u32,
                    channels: channels.max(1),
                    bits_per_sample: bits,
                })
            }
            _ => TrackParameters::None,
        };

        let language = language.filter(|l| l != "und" && !l.is_empty());

        Ok(Some(Track {
            id: number as u32,
            codec,
            media_type,
            timescale: self.timescale,
            duration: None, // filled from the segment duration after parsing
            language,
            name: name.filter(|n| !n.is_empty()),
            bitrate: None,
            codec_private,
            parameters,
        }))
    }

    fn parse_video(&mut self, start: u64, end: u64) -> Result<(u32, u32)> {
        let (mut w, mut h) = (0u32, 0u32);
        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            match id {
                PIXEL_WIDTH => {
                    self.reader.seek_to(body_start)?;
                    w = read_uint(&mut self.reader, size as usize)? as u32;
                }
                PIXEL_HEIGHT => {
                    self.reader.seek_to(body_start)?;
                    h = read_uint(&mut self.reader, size as usize)? as u32;
                }
                _ => {}
            }
            p = body_start + size;
        }
        Ok((w, h))
    }

    fn parse_audio(&mut self, start: u64, end: u64) -> Result<(f64, u16, Option<u16>)> {
        let (mut rate, mut channels, mut bits) = (8000.0, 1u16, None);
        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            match id {
                SAMPLING_FREQUENCY => {
                    self.reader.seek_to(body_start)?;
                    rate = read_float(&mut self.reader, size as usize)?;
                }
                CHANNELS => {
                    self.reader.seek_to(body_start)?;
                    channels = read_uint(&mut self.reader, size as usize)? as u16;
                }
                BIT_DEPTH => {
                    self.reader.seek_to(body_start)?;
                    bits = Some(read_uint(&mut self.reader, size as usize)? as u16);
                }
                _ => {}
            }
            p = body_start + size;
        }
        Ok((rate, channels, bits))
    }

    /// Read the next child element header at `pos`, returning `(id, body_start,
    /// body_size)`. Returns `Ok(None)` at or past `region_end` or on a clean EOF.
    fn next_child(&mut self, pos: u64, region_end: u64) -> Result<Option<(u32, u64, u64)>> {
        if pos + 2 > region_end {
            return Ok(None);
        }
        self.reader.seek_to(pos)?;
        let Ok(header) = read_element(&mut self.reader) else {
            return Ok(None);
        };
        let body_start = self.reader.stream_position()?;
        let size = header
            .size
            .unwrap_or_else(|| region_end.saturating_sub(body_start));
        if body_start + size > region_end && header.size.is_some() {
            // Element overruns its parent; clamp so we still make progress.
            return Ok(Some((
                header.id,
                body_start,
                region_end.saturating_sub(body_start),
            )));
        }
        Ok(Some((header.id, body_start, size)))
    }

    /// Read one cluster into `pending`.
    fn load_cluster(&mut self, start: u64, end: u64) -> Result<()> {
        let mut cluster_ts = 0i64;
        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            match id {
                CLUSTER_TIMESTAMP => {
                    self.reader.seek_to(body_start)?;
                    cluster_ts = read_uint(&mut self.reader, size as usize)? as i64;
                }
                SIMPLE_BLOCK => {
                    self.reader.seek_to(body_start)?;
                    let data = self.reader.read_vec(size as usize)?;
                    push_block(&data, cluster_ts, None, &mut self.pending);
                }
                BLOCK_GROUP => self.load_block_group(body_start, body_start + size, cluster_ts)?,
                _ => {}
            }
            p = body_start + size;
        }
        Ok(())
    }

    fn load_block_group(&mut self, start: u64, end: u64, cluster_ts: i64) -> Result<()> {
        let mut block: Option<Vec<u8>> = None;
        let mut has_reference = false;
        let mut p = start;
        while let Some((id, body_start, size)) = self.next_child(p, end)? {
            match id {
                BLOCK => {
                    self.reader.seek_to(body_start)?;
                    block = Some(self.reader.read_vec(size as usize)?);
                }
                REFERENCE_BLOCK => has_reference = true,
                _ => {}
            }
            p = body_start + size;
        }
        if let Some(data) = block {
            // In a BlockGroup, keyframe-ness is "has no reference".
            push_block(&data, cluster_ts, Some(!has_reference), &mut self.pending);
        }
        Ok(())
    }
}

impl<R: Read + Seek> Demuxer for MatroskaDemuxer<R> {
    fn format(&self) -> ContainerFormat {
        self.format
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn read_packet(&mut self) -> Result<Option<Packet>> {
        loop {
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
            if self.pos + 2 > self.segment_end {
                return Ok(None);
            }
            let Some((id, body_start, size)) = self.next_child(self.pos, self.segment_end)? else {
                return Ok(None);
            };
            self.pos = body_start + size;
            if id == CLUSTER {
                self.load_cluster(body_start, body_start + size)?;
            }
        }
    }

    fn seek(&mut self, target: Duration) -> Result<()> {
        // Linear seek: restart from the first cluster and skip clusters whose
        // timestamp is before the target. (Cue-based seeking is a future step.)
        let _ = target;
        Err(Error::unsupported(
            "Matroska seeking is not yet implemented",
        ))
    }
}

/// Read the `DocType` string from the EBML header body.
fn read_doctype<R: Read + Seek>(reader: &mut R, start: u64, end: u64) -> Result<String> {
    let mut p = start;
    while p + 2 <= end {
        reader.seek_to(p)?;
        let Ok(header) = read_element(reader) else {
            break;
        };
        let body_start = reader.stream_position()?;
        let size = header.size.unwrap_or(0);
        if header.id == DOCTYPE {
            reader.seek_to(body_start)?;
            return read_string(reader, size as usize);
        }
        p = body_start + size;
    }
    Ok("matroska".to_string())
}

/// Parse a (Simple)Block's header and push its frame(s) as packets.
fn push_block(
    data: &[u8],
    cluster_ts: i64,
    forced_keyframe: Option<bool>,
    out: &mut VecDeque<Packet>,
) {
    let mut pos = 0usize;
    let Some((track_number, _)) = read_vint(data, &mut pos) else {
        return;
    };
    if pos + 3 > data.len() {
        return;
    }
    let rel_ts = i16::from_be_bytes([data[pos], data[pos + 1]]);
    pos += 2;
    let flags = data[pos];
    pos += 1;

    let keyframe = forced_keyframe.unwrap_or(flags & 0x80 != 0);
    let lacing = (flags >> 1) & 0x03;
    let pts = cluster_ts + i64::from(rel_ts);

    let Some(frames) = split_frames(data, pos, lacing) else {
        return;
    };
    for (start, end) in frames {
        if start > end || end > data.len() {
            continue;
        }
        out.push_back(Packet {
            track_id: track_number as u32,
            dts: Some(pts),
            pts: Some(pts),
            duration: None,
            is_keyframe: keyframe,
            data: data[start..end].to_vec(),
        });
    }
}

/// Compute the byte ranges of the frames in a block body, honoring lacing.
fn split_frames(data: &[u8], mut pos: usize, lacing: u8) -> Option<Vec<(usize, usize)>> {
    if lacing == 0 {
        return Some(vec![(pos, data.len())]);
    }
    let frame_count = usize::from(*data.get(pos)?) + 1;
    pos += 1;
    let mut ranges = Vec::with_capacity(frame_count);

    match lacing {
        0b10 => {
            // Fixed-size lacing: equal-sized frames.
            let total = data.len().checked_sub(pos)?;
            let each = total / frame_count;
            for i in 0..frame_count {
                let start = pos + i * each;
                ranges.push((start, start + each));
            }
        }
        0b01 => {
            // Xiph lacing: sizes as sums of 0xFF-terminated byte runs.
            let mut sizes = Vec::with_capacity(frame_count);
            for _ in 0..frame_count - 1 {
                let mut size = 0usize;
                loop {
                    let b = *data.get(pos)?;
                    pos += 1;
                    size += usize::from(b);
                    if b != 0xFF {
                        break;
                    }
                }
                sizes.push(size);
            }
            emit_from_sizes(data, pos, &sizes, &mut ranges);
        }
        0b11 => {
            // EBML lacing: first size is a vint, the rest are signed deltas.
            let (first, _) = read_vint(data, &mut pos)?;
            let mut sizes = vec![first as usize];
            let mut prev = first as i64;
            for _ in 0..frame_count.saturating_sub(2) {
                let (raw, len) = read_vint(data, &mut pos)?;
                let bias = (1i64 << (7 * len - 1)) - 1;
                prev += raw as i64 - bias;
                sizes.push(prev.max(0) as usize);
            }
            emit_from_sizes(data, pos, &sizes, &mut ranges);
        }
        _ => return None,
    }
    Some(ranges)
}

/// Emit `sizes.len()` frames of the given sizes plus a final frame that runs to
/// the end of the data.
fn emit_from_sizes(data: &[u8], mut pos: usize, sizes: &[usize], ranges: &mut Vec<(usize, usize)>) {
    for &size in sizes {
        ranges.push((pos, pos + size));
        pos += size;
    }
    ranges.push((pos, data.len()));
}

/// Read an EBML vint from a byte slice, clearing the length-marker bit. Returns
/// `(value, byte_length)`.
fn read_vint(data: &[u8], pos: &mut usize) -> Option<(u64, u32)> {
    let b0 = *data.get(*pos)?;
    if b0 == 0 {
        return None;
    }
    let len = b0.leading_zeros() + 1;
    let mask = 0xFFu16 >> len;
    let mut value = u64::from(b0 & mask as u8);
    for i in 1..len as usize {
        value = (value << 8) | u64::from(*data.get(*pos + i)?);
    }
    *pos += len as usize;
    Some((value, len))
}

/// Map a Matroska `CodecID` to a [`Codec`].
fn codec_from_id(codec_id: &str) -> Codec {
    match codec_id {
        "V_MPEG4/ISO/AVC" => Codec::H264,
        "V_MPEGH/ISO/HEVC" => Codec::H265,
        "V_VP8" => Codec::Vp8,
        "V_VP9" => Codec::Vp9,
        "V_AV1" => Codec::Av1,
        "V_MPEG4/ISO/ASP" | "V_MPEG4/ISO/SP" | "V_MPEG4/ISO/AP" => Codec::Mpeg4Visual,
        "A_AAC" => Codec::Aac,
        "A_OPUS" => Codec::Opus,
        "A_VORBIS" => Codec::Vorbis,
        "A_FLAC" => Codec::Flac,
        "A_AC3" => Codec::Ac3,
        "A_EAC3" => Codec::Eac3,
        "A_MPEG/L3" => Codec::Mp3,
        "A_PCM/INT/LIT" => Codec::PcmS16Le,
        "A_PCM/INT/BIG" => Codec::PcmS16Be,
        "A_PCM/FLOAT/IEEE" => Codec::PcmF32Le,
        "S_TEXT/UTF8" => Codec::SubRip,
        "S_TEXT/ASS" | "S_TEXT/SSA" => Codec::Ass,
        "S_TEXT/WEBVTT" => Codec::WebVtt,
        other => Codec::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lacing_yields_one_frame() {
        // track_number(0x81) + rel_ts(0,0) + flags(0x80 keyframe) + 3 bytes.
        let block = [0x81, 0x00, 0x00, 0x80, 0xAA, 0xBB, 0xCC];
        let mut out = VecDeque::new();
        push_block(&block, 100, None, &mut out);
        assert_eq!(out.len(), 1);
        let p = out.pop_front().unwrap();
        assert_eq!(p.track_id, 1);
        assert_eq!(p.pts, Some(100));
        assert!(p.is_keyframe);
        assert_eq!(p.data, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn fixed_lacing_splits_evenly() {
        // flags 0x04 = fixed lacing (bits 10); frame_count byte 1 => 2 frames.
        let block = [0x81, 0x00, 0x00, 0x04, 0x01, 0xAA, 0xBB, 0xCC, 0xDD];
        let mut out = VecDeque::new();
        push_block(&block, 0, None, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].data, vec![0xAA, 0xBB]);
        assert_eq!(out[1].data, vec![0xCC, 0xDD]);
    }

    #[test]
    fn maps_webm_codecs() {
        assert_eq!(codec_from_id("V_VP9"), Codec::Vp9);
        assert_eq!(codec_from_id("A_OPUS"), Codec::Opus);
        assert_eq!(codec_from_id("V_MPEG4/ISO/AVC"), Codec::H264);
    }
}

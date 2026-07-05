//! Native MP4 / MOV (ISO Base Media File Format) demuxer.
//!
//! Parses the `moov` box into tracks and per-track sample tables, then serves
//! packets by reading sample byte-ranges out of `mdat` on demand. No data is
//! decoded — packets carry coded bytes straight from the file, which is exactly
//! what remuxing and trimming need.
//!
//! Supported: plain (non-fragmented) MP4 and QuickTime `.mov`, including 64-bit
//! offsets (`co64`), composition offsets (`ctts`), and iTunes metadata. Sample
//! layout, timing, and keyframe flags are recovered from the standard `stbl`
//! tables.

mod boxes;
mod meta;
mod mux;
mod sample_entry;
mod sample_table;
mod writer;

pub use mux::Mp4Muxer;

use std::io::{Read, Seek};
use std::time::Duration;

use rustmedia_core::{
    Codec, ContainerFormat, Error, MediaType, Metadata, Packet, Rational, Result, Timestamp, Track,
    TrackParameters,
};
use rustmedia_io::{ReadBytes, Source};

use crate::demux::Demuxer;
use boxes::{boxes_in, read_box_header, read_full_box_header};
use sample_entry::{parse_stsd, HandlerKind};
use sample_table::{parse_sample_table, Sample};

use std::io::Cursor;

/// A demuxer for MP4 and QuickTime files.
pub struct Mp4Demuxer<R: Read + Seek> {
    reader: R,
    format: ContainerFormat,
    tracks: Vec<Track>,
    /// Per-track flattened sample tables, parallel to `tracks`.
    samples: Vec<Vec<Sample>>,
    metadata: Metadata,
    duration: Option<Duration>,
    /// Global read order: `(track_index, sample_index)` sorted by file offset.
    order: Vec<(u32, u32)>,
    /// Cursor into `order` for `read_packet`.
    cursor: usize,
}

impl<R: Read + Seek> Mp4Demuxer<R> {
    /// Open a source and parse its movie header, leaving it ready to serve
    /// packets.
    ///
    /// # Errors
    /// Returns [`Error::Malformed`] if the file is not a valid ISO-BMFF file or
    /// its `moov` box is missing.
    pub fn new(mut reader: R) -> Result<Self> {
        let file_size = reader.size()?;
        reader.seek_to(0)?;

        let mut moov_payload: Option<Vec<u8>> = None;
        let mut format = ContainerFormat::Mp4;

        loop {
            let pos = reader.stream_position()?;
            let Some(header) = read_box_header(&mut reader)? else {
                break;
            };
            let payload_start = pos + header.header_len;
            let payload_len = header.payload_len(file_size.saturating_sub(pos));
            let next = payload_start + payload_len;

            match &header.kind {
                b"ftyp" => {
                    reader.seek_to(payload_start)?;
                    let brand = reader.read_fourcc()?;
                    if &brand[..2] == b"qt" {
                        format = ContainerFormat::Mov;
                    }
                }
                b"moov" => {
                    reader.seek_to(payload_start)?;
                    let len = usize::try_from(payload_len).map_err(|_| {
                        Error::malformed("mp4", "moov box too large to load into memory")
                    })?;
                    moov_payload = Some(reader.read_vec(len)?);
                }
                _ => {}
            }

            if next <= pos {
                break; // malformed / zero-progress; stop to avoid looping.
            }
            reader.seek_to(next)?;
        }

        let moov = moov_payload
            .ok_or_else(|| Error::malformed("mp4", "no 'moov' box found; not a valid MP4/MOV"))?;

        let mut parsed = ParsedMoov::default();
        parsed.parse(&moov);

        // Compute the duration (borrows tracks) and take metadata before the
        // tracks are consumed below.
        let duration = parsed.movie_duration();
        let metadata = std::mem::take(&mut parsed.metadata);

        let mut tracks = Vec::with_capacity(parsed.tracks.len());
        let mut samples = Vec::with_capacity(parsed.tracks.len());
        for tb in parsed.tracks {
            tracks.push(tb.track);
            samples.push(tb.samples);
        }

        let mut demuxer = Mp4Demuxer {
            reader,
            format,
            tracks,
            samples,
            metadata,
            duration,
            order: Vec::new(),
            cursor: 0,
        };
        demuxer.build_order();
        Ok(demuxer)
    }

    /// Build the global file-order iteration list across all tracks.
    fn build_order(&mut self) {
        let mut order: Vec<(u32, u32)> = Vec::new();
        for (ti, samples) in self.samples.iter().enumerate() {
            for si in 0..samples.len() {
                order.push((ti as u32, si as u32));
            }
        }
        order.sort_by_key(|&(ti, si)| self.samples[ti as usize][si as usize].offset);
        self.order = order;
    }

    fn sample_at(&self, track_index: u32, sample_index: u32) -> &Sample {
        &self.samples[track_index as usize][sample_index as usize]
    }
}

impl<R: Read + Seek> Demuxer for Mp4Demuxer<R> {
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
        if self.cursor >= self.order.len() {
            return Ok(None);
        }
        let (ti, si) = self.order[self.cursor];
        self.cursor += 1;

        let sample = *self.sample_at(ti, si);
        let track = &self.tracks[ti as usize];
        self.reader.seek_to(sample.offset)?;
        let data = self.reader.read_vec(sample.size as usize)?;

        Ok(Some(Packet {
            track_id: track.id,
            dts: Some(sample.dts),
            pts: Some(sample.pts),
            duration: Some(u64::from(sample.duration)),
            is_keyframe: sample.is_sync,
            data,
        }))
    }

    fn seek(&mut self, target: Duration) -> Result<()> {
        // Choose a reference track: prefer the first video track, else track 0.
        let ref_track = self.tracks.iter().position(Track::is_video).unwrap_or(0);
        let Some(samples) = self.samples.get(ref_track) else {
            self.cursor = 0;
            return Ok(());
        };
        if samples.is_empty() {
            self.cursor = 0;
            return Ok(());
        }
        let timescale = self.tracks[ref_track].timescale.max(1);
        let target_ticks = (target.as_secs_f64() * f64::from(timescale)) as i64;

        // Last sync sample whose dts <= target.
        let mut keyframe_offset = samples[0].offset;
        for s in samples {
            if s.is_sync && s.dts <= target_ticks {
                keyframe_offset = s.offset;
            } else if s.dts > target_ticks {
                break;
            }
        }

        // Resume from the first packet in file order at/after that keyframe.
        self.cursor = self
            .order
            .iter()
            .position(|&(ti, si)| self.samples[ti as usize][si as usize].offset >= keyframe_offset)
            .unwrap_or(self.order.len());
        Ok(())
    }
}

// --------------------------------------------------------------------------
// moov tree parsing
// --------------------------------------------------------------------------

/// Intermediate result of walking the `moov` tree.
#[derive(Default)]
struct ParsedMoov {
    movie_timescale: u32,
    movie_duration_ticks: u64,
    tracks: Vec<TrackBuild>,
    metadata: Metadata,
}

struct TrackBuild {
    track: Track,
    samples: Vec<Sample>,
}

impl ParsedMoov {
    fn parse(&mut self, moov: &[u8]) {
        for (kind, data) in boxes_in(moov) {
            match &kind {
                b"mvhd" => self.parse_mvhd(data),
                b"trak" => {
                    if let Some(tb) = parse_trak(data) {
                        self.tracks.push(tb);
                    }
                }
                b"udta" => meta::parse_udta(data, &mut self.metadata),
                b"meta" => {
                    // Some files put metadata directly under moov/meta.
                    let mut m = Metadata::new();
                    // Reuse udta parser semantics by wrapping: parse the meta box.
                    meta::parse_udta(&wrap_box(b"meta", data), &mut m);
                    merge_metadata(&mut self.metadata, m);
                }
                _ => {}
            }
        }
    }

    fn parse_mvhd(&mut self, payload: &[u8]) {
        let mut c = Cursor::new(payload);
        let Ok(full) = read_full_box_header(&mut c) else {
            return;
        };
        let ok = if full.version >= 1 {
            // creation(8) + modification(8), then timescale(4), duration(8).
            skip(&mut c, 16).is_ok()
                && read_u32(&mut c).map(|ts| self.movie_timescale = ts).is_ok()
                && read_u64(&mut c)
                    .map(|d| self.movie_duration_ticks = d)
                    .is_ok()
        } else {
            skip(&mut c, 8).is_ok()
                && read_u32(&mut c).map(|ts| self.movie_timescale = ts).is_ok()
                && read_u32(&mut c)
                    .map(|d| self.movie_duration_ticks = u64::from(d))
                    .is_ok()
        };
        let _ = ok;
    }

    fn movie_duration(&self) -> Option<Duration> {
        if self.movie_timescale > 0 && self.movie_duration_ticks > 0 {
            return Some(
                Timestamp::new(self.movie_duration_ticks as i64, self.movie_timescale)
                    .to_duration(),
            );
        }
        // Fall back to the longest track duration.
        self.tracks.iter().filter_map(|t| t.track.duration()).max()
    }
}

/// Parse one `trak` box into a track and its sample list.
fn parse_trak(payload: &[u8]) -> Option<TrackBuild> {
    let mut track_id = 0u32;
    let mut tkhd_dims = (0u32, 0u32);
    let mut mdia: Option<MdiaInfo> = None;

    for (kind, data) in boxes_in(payload) {
        match &kind {
            b"tkhd" => {
                if let Some((id, w, h)) = parse_tkhd(data) {
                    track_id = id;
                    tkhd_dims = (w, h);
                }
            }
            b"mdia" => mdia = parse_mdia(data),
            _ => {}
        }
    }

    let mdia = mdia?;
    let timescale = mdia.timescale.max(1);

    // Total coded bytes and duration → bitrate.
    let total_bytes: u64 = mdia.samples.iter().map(|s| u64::from(s.size)).sum();
    let duration_ticks = mdia.duration_ticks.max(
        mdia.samples
            .last()
            .map_or(0, |s| (s.dts + i64::from(s.duration)).max(0) as u64),
    );
    let duration_secs = duration_ticks as f64 / f64::from(timescale);
    let bitrate = if duration_secs > 0.0 {
        Some((total_bytes as f64 * 8.0 / duration_secs) as u64)
    } else {
        None
    };

    // Resolve media type: prefer the handler, fall back to the codec.
    let media_type = match mdia.handler {
        HandlerKind::Video => MediaType::Video,
        HandlerKind::Audio => MediaType::Audio,
        HandlerKind::Subtitle => MediaType::Subtitle,
        HandlerKind::Other => mdia.codec.media_type(),
    };

    // Fill in video frame rate (from the sample table) and fall back to tkhd
    // dimensions when the sample entry lacks them.
    let mut parameters = mdia.parameters;
    if let TrackParameters::Video(ref mut v) = parameters {
        if v.width == 0 || v.height == 0 {
            v.width = tkhd_dims.0;
            v.height = tkhd_dims.1;
        }
        let n = mdia.samples.len();
        if n > 1 && duration_ticks > 0 {
            v.frame_rate = Some(Rational::new(
                n as i64 * i64::from(timescale),
                duration_ticks as i64,
            ));
        }
    }

    let duration = if duration_ticks > 0 {
        Some(Timestamp::new(duration_ticks as i64, timescale))
    } else {
        None
    };

    Some(TrackBuild {
        track: Track {
            id: track_id,
            codec: mdia.codec,
            media_type,
            timescale,
            duration,
            language: mdia.language,
            name: None,
            bitrate,
            codec_private: mdia.codec_private,
            parameters,
        },
        samples: mdia.samples,
    })
}

struct MdiaInfo {
    timescale: u32,
    duration_ticks: u64,
    language: Option<String>,
    handler: HandlerKind,
    codec: Codec,
    parameters: TrackParameters,
    codec_private: Option<Vec<u8>>,
    samples: Vec<Sample>,
}

fn parse_mdia(payload: &[u8]) -> Option<MdiaInfo> {
    let mut timescale = 0u32;
    let mut duration_ticks = 0u64;
    let mut language = None;
    let mut handler = HandlerKind::Other;
    let mut minf_payload: Option<Vec<u8>> = None;

    for (kind, data) in boxes_in(payload) {
        match &kind {
            b"mdhd" => {
                if let Some((ts, dur, lang)) = parse_mdhd(data) {
                    timescale = ts;
                    duration_ticks = dur;
                    language = lang;
                }
            }
            b"hdlr" => handler = parse_hdlr(data),
            b"minf" => minf_payload = Some(data.to_vec()),
            _ => {}
        }
    }

    let minf = minf_payload?;
    let stbl = boxes_in(&minf)
        .into_iter()
        .find(|(k, _)| k == b"stbl")
        .map(|(_, d)| d.to_vec())?;

    let stsd = boxes_in(&stbl)
        .into_iter()
        .find(|(k, _)| k == b"stsd")
        .map(|(_, d)| d.to_vec());
    let stsd_info = stsd.and_then(|payload| parse_stsd(&payload, handler));

    let samples = parse_sample_table(&stbl).unwrap_or_default();

    let (codec, parameters, codec_private) = match stsd_info {
        Some(info) => (info.codec, info.parameters, info.codec_private),
        None => (Codec::Unknown, TrackParameters::None, None),
    };

    Some(MdiaInfo {
        timescale,
        duration_ticks,
        language,
        handler,
        codec,
        parameters,
        codec_private,
        samples,
    })
}

fn parse_tkhd(payload: &[u8]) -> Option<(u32, u32, u32)> {
    let mut c = Cursor::new(payload);
    let full = read_full_box_header(&mut c).ok()?;
    let track_id = if full.version >= 1 {
        skip(&mut c, 16).ok()?; // creation + modification (8 each)
        let id = read_u32(&mut c).ok()?;
        skip(&mut c, 12).ok()?; // reserved(4) + duration(8)
        id
    } else {
        skip(&mut c, 8).ok()?; // creation + modification (4 each)
        let id = read_u32(&mut c).ok()?;
        skip(&mut c, 8).ok()?; // reserved(4) + duration(4)
        id
    };
    // reserved(8) + layer(2) + alt_group(2) + volume(2) + reserved(2) + matrix(36).
    skip(&mut c, 8 + 2 + 2 + 2 + 2 + 36).ok()?;
    let width = read_u32(&mut c).ok()? >> 16;
    let height = read_u32(&mut c).ok()? >> 16;
    Some((track_id, width, height))
}

fn parse_mdhd(payload: &[u8]) -> Option<(u32, u64, Option<String>)> {
    let mut c = Cursor::new(payload);
    let full = read_full_box_header(&mut c).ok()?;
    let (timescale, duration) = if full.version >= 1 {
        skip(&mut c, 16).ok()?;
        (read_u32(&mut c).ok()?, read_u64(&mut c).ok()?)
    } else {
        skip(&mut c, 8).ok()?;
        (read_u32(&mut c).ok()?, u64::from(read_u32(&mut c).ok()?))
    };
    let lang_code = read_u16(&mut c).ok()?;
    Some((timescale, duration, decode_language(lang_code)))
}

fn parse_hdlr(payload: &[u8]) -> HandlerKind {
    let mut c = Cursor::new(payload);
    if read_full_box_header(&mut c).is_err() {
        return HandlerKind::Other;
    }
    if skip(&mut c, 4).is_err() {
        return HandlerKind::Other;
    }
    let Ok(handler) = c.read_fourcc() else {
        return HandlerKind::Other;
    };
    match &handler {
        b"vide" => HandlerKind::Video,
        b"soun" => HandlerKind::Audio,
        b"text" | b"sbtl" | b"subt" | b"clcp" => HandlerKind::Subtitle,
        _ => HandlerKind::Other,
    }
}

/// Decode an ISO-639-2/T language packed into 15 bits (three 5-bit letters).
fn decode_language(code: u16) -> Option<String> {
    if code == 0 {
        return None;
    }
    let chars = [
        (((code >> 10) & 0x1F) as u8) + 0x60,
        (((code >> 5) & 0x1F) as u8) + 0x60,
        ((code & 0x1F) as u8) + 0x60,
    ];
    if chars.iter().all(|&b| b.is_ascii_lowercase()) {
        let s: String = chars.iter().map(|&b| b as char).collect();
        if s == "und" {
            None
        } else {
            Some(s)
        }
    } else {
        None
    }
}

/// Wrap a payload back into a box with the given type, so a metadata parser
/// that expects a container can be reused.
fn wrap_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(payload.len() + 8);
    v.extend_from_slice(&((payload.len() as u32 + 8).to_be_bytes()));
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn merge_metadata(into: &mut Metadata, from: Metadata) {
    for (k, v) in from.iter() {
        if into.get(k).is_none() {
            into.insert(k, v);
        }
    }
    into.chapters.extend(from.chapters);
}

// Small Cursor helpers that return core `Result` for use with `?`/`.ok()?`.

fn skip(c: &mut Cursor<&[u8]>, n: u64) -> Result<()> {
    c.skip(n)
}

fn read_u16(c: &mut Cursor<&[u8]>) -> Result<u16> {
    c.read_u16_be()
}

fn read_u32(c: &mut Cursor<&[u8]>) -> Result<u32> {
    c.read_u32_be()
}

fn read_u64(c: &mut Cursor<&[u8]>) -> Result<u64> {
    c.read_u64_be()
}

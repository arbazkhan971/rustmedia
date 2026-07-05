//! Native MP4 muxer — writes a valid, faststart (moov-first) ISO-BMFF file.
//!
//! The muxer buffers coded sample data and per-sample timing, then assembles a
//! complete `moov` (with real `stbl` tables and codec `stsd` entries) followed
//! by an `mdat`. Because `moov` precedes `mdat`, output is streaming-friendly
//! ("faststart"). Nothing is ever re-encoded: packets are copied verbatim, so
//! remuxing and trimming are lossless.

use std::collections::HashMap;
use std::io::Write;

use rustmedia_core::{Codec, Error, MediaType, Packet, Result, Track, TrackParameters};

use super::writer::{atom, full_atom, pack_language, push_matrix, Writer};
use crate::mux::Muxer;

/// Movie-level timescale used for `mvhd`/`tkhd` durations (milliseconds).
const MOVIE_TIMESCALE: u32 = 1000;

/// A muxer that writes MP4 files to any [`Write`] sink.
pub struct Mp4Muxer<W: Write> {
    sink: W,
    tracks: Vec<MuxTrack>,
    /// Maps a source [`Track::id`] to an index into `tracks`.
    id_map: HashMap<u32, usize>,
    mdat: Vec<u8>,
    started: bool,
    finished: bool,
}

struct MuxTrack {
    out_id: u32,
    media_type: MediaType,
    codec: Codec,
    timescale: u32,
    language: Option<String>,
    codec_private: Option<Vec<u8>>,
    parameters: TrackParameters,
    samples: Vec<MuxSample>,
}

struct MuxSample {
    offset: u64, // relative to the start of mdat payload
    size: u32,
    dts: i64,
    pts: i64,
    duration: u32,
    is_sync: bool,
}

impl<W: Write> Mp4Muxer<W> {
    /// Create a muxer writing to `sink`.
    pub fn new(sink: W) -> Self {
        Mp4Muxer {
            sink,
            tracks: Vec::new(),
            id_map: HashMap::new(),
            mdat: Vec::new(),
            started: false,
            finished: false,
        }
    }
}

impl<W: Write> Muxer for Mp4Muxer<W> {
    fn start(&mut self, tracks: &[Track]) -> Result<()> {
        if self.started {
            return Err(Error::invalid_argument("Mp4Muxer::start called twice"));
        }
        self.started = true;
        for (index, track) in tracks.iter().enumerate() {
            // Validate we can encode a sample entry for this codec up front.
            codec_fourcc(&track.codec).ok_or_else(|| {
                Error::unsupported(format!("cannot mux codec '{}' into MP4", track.codec))
            })?;
            self.id_map.insert(track.id, index);
            self.tracks.push(MuxTrack {
                out_id: index as u32 + 1,
                media_type: track.media_type,
                codec: track.codec.clone(),
                timescale: track.timescale.max(1),
                language: track.language.clone(),
                codec_private: track.codec_private.clone(),
                parameters: track.parameters.clone(),
                samples: Vec::new(),
            });
        }
        Ok(())
    }

    fn write_packet(&mut self, packet: &Packet) -> Result<()> {
        let &index = self.id_map.get(&packet.track_id).ok_or_else(|| {
            Error::invalid_argument(format!("packet for unknown track {}", packet.track_id))
        })?;
        let offset = self.mdat.len() as u64;
        self.mdat.extend_from_slice(&packet.data);

        let dts = packet.dts.or(packet.pts).unwrap_or(0);
        let pts = packet.pts.unwrap_or(dts);
        self.tracks[index].samples.push(MuxSample {
            offset,
            size: packet.data.len() as u32,
            dts,
            pts,
            duration: packet.duration.unwrap_or(0) as u32,
            is_sync: packet.is_keyframe,
        });
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;

        // Derive any missing sample durations from dts deltas.
        for track in &mut self.tracks {
            fix_durations(&mut track.samples, track.timescale);
        }

        let use_co64 = self.mdat.len() as u64 > u64::from(u32::MAX) - (64 << 20);
        let ftyp = build_ftyp();

        // Build `moov` once to learn its size (offsets don't affect the size),
        // then again with the real mdat base offset.
        let measure = build_moov(&self.tracks, 0, use_co64);
        let mdat_header_len = if use_co64 { 16 } else { 8 };
        let data_start = (ftyp.len() + measure.len() + mdat_header_len) as u64;
        let moov = build_moov(&self.tracks, data_start, use_co64);

        self.sink.write_all(&ftyp)?;
        self.sink.write_all(&moov)?;
        write_mdat_header(&mut self.sink, self.mdat.len(), use_co64)?;
        self.sink.write_all(&self.mdat)?;
        self.sink.flush()?;
        Ok(())
    }
}

/// Compute per-sample durations for any samples that arrived without one.
fn fix_durations(samples: &mut [MuxSample], timescale: u32) {
    let n = samples.len();
    for i in 0..n {
        if samples[i].duration != 0 {
            continue;
        }
        let derived = if i + 1 < n {
            (samples[i + 1].dts - samples[i].dts).max(0) as u32
        } else if i > 0 {
            samples[i - 1].duration
        } else {
            timescale / 25 // last resort: assume ~25 units/sec
        };
        samples[i].duration = derived;
    }
}

fn write_mdat_header<W: Write>(sink: &mut W, payload_len: usize, use_co64: bool) -> Result<()> {
    if use_co64 {
        let total = payload_len as u64 + 16;
        sink.write_all(&1u32.to_be_bytes())?;
        sink.write_all(b"mdat")?;
        sink.write_all(&total.to_be_bytes())?;
    } else {
        let total = payload_len as u32 + 8;
        sink.write_all(&total.to_be_bytes())?;
        sink.write_all(b"mdat")?;
    }
    Ok(())
}

fn build_ftyp() -> Vec<u8> {
    let mut b = Writer::new();
    b.bytes(b"isom") // major brand
        .u32(0x0000_0200) // minor version
        .bytes(b"isom")
        .bytes(b"iso2")
        .bytes(b"mp41")
        .bytes(b"avc1");
    atom(b"ftyp", b.as_slice())
}

fn build_moov(tracks: &[MuxTrack], data_start: u64, use_co64: bool) -> Vec<u8> {
    let mut movie_duration = 0u64;
    let mut body = Vec::new();

    // Reserve mvhd space by building traks first (need movie_duration).
    let mut traks = Vec::new();
    for track in tracks {
        let trak = build_trak(track, data_start, use_co64);
        let track_ticks: u64 = track.samples.iter().map(|s| u64::from(s.duration)).sum();
        let movie_ticks = rescale(track_ticks, track.timescale, MOVIE_TIMESCALE);
        movie_duration = movie_duration.max(movie_ticks);
        traks.push(trak);
    }

    let next_track_id = tracks.iter().map(|t| t.out_id).max().unwrap_or(0) + 1;
    body.extend(build_mvhd(movie_duration, next_track_id));
    for trak in traks {
        body.extend(trak);
    }
    atom(b"moov", &body)
}

fn build_mvhd(duration: u64, next_track_id: u32) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(0).u32(0) // creation, modification
        .u32(MOVIE_TIMESCALE)
        .u32(duration as u32)
        .u32(0x0001_0000) // rate 1.0
        .u16(0x0100) // volume 1.0
        .u16(0)
        .u32(0)
        .u32(0); // reserved
    push_matrix(&mut w);
    w.zeros(24); // predefined
    w.u32(next_track_id);
    full_atom(b"mvhd", 0, 0, w.as_slice())
}

fn build_trak(track: &MuxTrack, data_start: u64, use_co64: bool) -> Vec<u8> {
    let track_ticks: u64 = track.samples.iter().map(|s| u64::from(s.duration)).sum();
    let movie_ticks = rescale(track_ticks, track.timescale, MOVIE_TIMESCALE);

    let (width, height) = match &track.parameters {
        TrackParameters::Video(v) => (v.width, v.height),
        _ => (0, 0),
    };

    // tkhd
    let mut t = Writer::new();
    t.u32(0).u32(0) // creation, modification
        .u32(track.out_id)
        .u32(0) // reserved
        .u32(movie_ticks as u32)
        .u32(0)
        .u32(0) // reserved
        .u16(0) // layer
        .u16(0) // alternate group
        .u16(if track.media_type == MediaType::Audio { 0x0100 } else { 0 }) // volume
        .u16(0); // reserved
    push_matrix(&mut t);
    t.u32(width << 16).u32(height << 16);
    let tkhd = full_atom(b"tkhd", 0, 0x7, t.as_slice());

    let mdia = build_mdia(track, track_ticks, data_start, use_co64);

    let mut body = tkhd;
    body.extend(mdia);
    atom(b"trak", &body)
}

fn build_mdia(track: &MuxTrack, track_ticks: u64, data_start: u64, use_co64: bool) -> Vec<u8> {
    // mdhd
    let mut m = Writer::new();
    m.u32(0).u32(0) // creation, modification
        .u32(track.timescale)
        .u32(track_ticks as u32)
        .u16(pack_language(track.language.as_deref()))
        .u16(0); // predefined
    let mdhd = full_atom(b"mdhd", 0, 0, m.as_slice());

    // hdlr
    let (handler, name): (&[u8; 4], &str) = match track.media_type {
        MediaType::Video => (b"vide", "RustMedia Video Handler"),
        MediaType::Audio => (b"soun", "RustMedia Audio Handler"),
        _ => (b"text", "RustMedia Text Handler"),
    };
    let mut h = Writer::new();
    h.u32(0)
        .bytes(handler)
        .u32(0)
        .u32(0)
        .u32(0)
        .bytes(name.as_bytes())
        .u8(0);
    let hdlr = full_atom(b"hdlr", 0, 0, h.as_slice());

    let minf = build_minf(track, data_start, use_co64);

    let mut body = mdhd;
    body.extend(hdlr);
    body.extend(minf);
    atom(b"mdia", &body)
}

fn build_minf(track: &MuxTrack, data_start: u64, use_co64: bool) -> Vec<u8> {
    // Media-information header.
    let media_header = match track.media_type {
        MediaType::Video => {
            let mut w = Writer::new();
            w.u16(0).u16(0).u16(0).u16(0); // graphicsmode + opcolor[3]
            full_atom(b"vmhd", 0, 1, w.as_slice())
        }
        MediaType::Audio => {
            let mut w = Writer::new();
            w.u16(0).u16(0); // balance + reserved
            full_atom(b"smhd", 0, 0, w.as_slice())
        }
        _ => {
            // Null media header for text/other tracks.
            full_atom(b"nmhd", 0, 0, &[])
        }
    };

    // dinf/dref with one self-contained URL entry.
    let url = full_atom(b"url ", 0, 1, &[]);
    let mut dref_body = Writer::new();
    dref_body.u32(1); // entry count
    dref_body.bytes(&url);
    let dref = full_atom(b"dref", 0, 0, dref_body.as_slice());
    let dinf = atom(b"dinf", &dref);

    let stbl = build_stbl(track, data_start, use_co64);

    let mut body = media_header;
    body.extend(dinf);
    body.extend(stbl);
    atom(b"minf", &body)
}

fn build_stbl(track: &MuxTrack, data_start: u64, use_co64: bool) -> Vec<u8> {
    let samples = &track.samples;

    // stsd
    let stsd = build_stsd(track);

    // stts (time-to-sample), run-length over durations.
    let stts = {
        let runs = run_length(samples.iter().map(|s| s.duration));
        let mut w = Writer::new();
        w.u32(runs.len() as u32);
        for (count, delta) in runs {
            w.u32(count).u32(delta);
        }
        full_atom(b"stts", 0, 0, w.as_slice())
    };

    // ctts (composition offsets), only if any sample needs one.
    let needs_ctts = samples.iter().any(|s| s.pts != s.dts);
    let ctts = needs_ctts.then(|| {
        let runs = run_length(samples.iter().map(|s| (s.pts - s.dts) as i32));
        let mut w = Writer::new();
        w.u32(runs.len() as u32);
        for (count, offset) in runs {
            w.u32(count).i32(offset);
        }
        full_atom(b"ctts", 1, 0, w.as_slice())
    });

    // stsc: every chunk holds exactly one sample (preserves interleaving).
    let stsc = {
        let mut w = Writer::new();
        w.u32(1).u32(1).u32(1).u32(1);
        full_atom(b"stsc", 0, 0, w.as_slice())
    };

    // stsz: explicit per-sample sizes.
    let stsz = {
        let mut w = Writer::new();
        w.u32(0).u32(samples.len() as u32);
        for s in samples {
            w.u32(s.size);
        }
        full_atom(b"stsz", 0, 0, w.as_slice())
    };

    // stco / co64: absolute chunk (== sample) offsets.
    let stco = if use_co64 {
        let mut w = Writer::new();
        w.u32(samples.len() as u32);
        for s in samples {
            w.u64(data_start + s.offset);
        }
        full_atom(b"co64", 0, 0, w.as_slice())
    } else {
        let mut w = Writer::new();
        w.u32(samples.len() as u32);
        for s in samples {
            w.u32((data_start + s.offset) as u32);
        }
        full_atom(b"stco", 0, 0, w.as_slice())
    };

    // stss: sync-sample list, omitted when every sample is a sync sample.
    let all_sync = samples.iter().all(|s| s.is_sync);
    let stss = (!all_sync).then(|| {
        let syncs: Vec<u32> = samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_sync)
            .map(|(i, _)| i as u32 + 1)
            .collect();
        let mut w = Writer::new();
        w.u32(syncs.len() as u32);
        for n in syncs {
            w.u32(n);
        }
        full_atom(b"stss", 0, 0, w.as_slice())
    });

    let mut body = stsd;
    body.extend(stts);
    if let Some(ctts) = ctts {
        body.extend(ctts);
    }
    body.extend(stsc);
    body.extend(stsz);
    body.extend(stco);
    if let Some(stss) = stss {
        body.extend(stss);
    }
    atom(b"stbl", &body)
}

fn build_stsd(track: &MuxTrack) -> Vec<u8> {
    let entry = match track.media_type {
        MediaType::Audio => build_audio_sample_entry(track),
        // Video and, best-effort, everything else use a visual sample entry.
        _ => build_video_sample_entry(track),
    };
    let mut w = Writer::new();
    w.u32(1); // entry count
    w.bytes(&entry);
    full_atom(b"stsd", 0, 0, w.as_slice())
}

fn build_video_sample_entry(track: &MuxTrack) -> Vec<u8> {
    let (width, height) = match &track.parameters {
        TrackParameters::Video(v) => (v.width as u16, v.height as u16),
        _ => (0, 0),
    };
    let fourcc = codec_fourcc(&track.codec).unwrap_or(*b"avc1");

    let mut b = Writer::new();
    b.zeros(6).u16(1); // reserved + data_reference_index
    b.u16(0).u16(0).zeros(12); // predefined + reserved + predefined[3]
    b.u16(width).u16(height);
    b.u32(0x0048_0000).u32(0x0048_0000).u32(0); // 72 dpi h/v res + reserved
    b.u16(1); // frame count
    b.zeros(32); // compressor name
    b.u16(0x0018).u16(0xFFFF); // depth + predefined(-1)

    if let Some(config) = video_config_box(track) {
        b.bytes(&config);
    }
    atom(&fourcc, b.as_slice())
}

fn build_audio_sample_entry(track: &MuxTrack) -> Vec<u8> {
    let (sample_rate, channels) = match &track.parameters {
        TrackParameters::Audio(a) => (a.sample_rate, a.channels),
        _ => (48_000, 2),
    };
    let fourcc = codec_fourcc(&track.codec).unwrap_or(*b"mp4a");

    let mut b = Writer::new();
    b.zeros(6).u16(1); // reserved + data_reference_index
    b.u16(0).u16(0).u32(0); // version + revision + vendor
    b.u16(channels).u16(16); // channel count + sample size
    b.u16(0).u16(0); // predefined + reserved
    b.u32(sample_rate << 16); // 16.16 sample rate

    if let Some(config) = audio_config_box(track) {
        b.bytes(&config);
    }
    atom(&fourcc, b.as_slice())
}

/// Build the codec-config child box for a video sample entry.
fn video_config_box(track: &MuxTrack) -> Option<Vec<u8>> {
    let cp = track.codec_private.as_deref()?;
    let kind: &[u8; 4] = match track.codec {
        Codec::H264 => b"avcC",
        Codec::H265 => b"hvcC",
        Codec::Av1 => b"av1C",
        Codec::Vp8 | Codec::Vp9 => b"vpcC",
        _ => return None,
    };
    Some(atom(kind, cp))
}

/// Build the codec-config child box for an audio sample entry.
fn audio_config_box(track: &MuxTrack) -> Option<Vec<u8>> {
    match track.codec {
        Codec::Aac => Some(build_esds(0x40, track.codec_private.as_deref())),
        Codec::Mp3 => Some(build_esds(0x6B, track.codec_private.as_deref())),
        Codec::Opus => track.codec_private.as_deref().map(|cp| atom(b"dOps", cp)),
        Codec::Flac => track.codec_private.as_deref().map(|cp| atom(b"dfLa", cp)),
        _ => None,
    }
}

/// Build an `esds` box for an MPEG-4 audio stream.
fn build_esds(object_type: u8, asc: Option<&[u8]>) -> Vec<u8> {
    let dsi = match asc {
        Some(bytes) if !bytes.is_empty() => descriptor(0x05, bytes),
        _ => Vec::new(),
    };

    // DecoderConfigDescriptor (0x04).
    let mut dcd = Vec::new();
    dcd.push(object_type);
    dcd.push(0x15); // streamType=audio(5)<<2 | upStream(0) | reserved(1)
    dcd.extend_from_slice(&[0, 0, 0]); // bufferSizeDB
    dcd.extend_from_slice(&0u32.to_be_bytes()); // maxBitrate
    dcd.extend_from_slice(&0u32.to_be_bytes()); // avgBitrate
    dcd.extend_from_slice(&dsi);

    // ES_Descriptor (0x03).
    let mut es = Vec::new();
    es.extend_from_slice(&0u16.to_be_bytes()); // ES_ID
    es.push(0); // flags
    es.extend_from_slice(&descriptor(0x04, &dcd));
    es.extend_from_slice(&descriptor(0x06, &[0x02])); // SLConfigDescriptor

    full_atom(b"esds", 0, 0, &descriptor(0x03, &es))
}

/// Wrap `payload` in an MPEG-4 descriptor: a tag byte and an expandable length.
fn descriptor(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(encode_descriptor_len(payload.len()));
    out.extend_from_slice(payload);
    out
}

fn encode_descriptor_len(len: usize) -> Vec<u8> {
    let mut groups = vec![(len & 0x7F) as u8];
    let mut v = len >> 7;
    while v > 0 {
        groups.push((v & 0x7F) as u8);
        v >>= 7;
    }
    groups.reverse();
    let n = groups.len();
    groups
        .into_iter()
        .enumerate()
        .map(|(i, g)| if i + 1 < n { g | 0x80 } else { g })
        .collect()
}

/// The sample-entry fourcc RustMedia writes for a codec, or `None` if the codec
/// cannot be muxed into MP4.
fn codec_fourcc(codec: &Codec) -> Option<[u8; 4]> {
    Some(match codec {
        Codec::H264 => *b"avc1",
        Codec::H265 => *b"hvc1",
        Codec::Av1 => *b"av01",
        Codec::Vp9 => *b"vp09",
        Codec::Vp8 => *b"vp08",
        Codec::Aac | Codec::Mp3 => *b"mp4a",
        Codec::Opus => *b"Opus",
        Codec::Flac => *b"fLaC",
        Codec::PcmS16Le => *b"sowt",
        Codec::PcmS16Be => *b"twos",
        Codec::MovText => *b"tx3g",
        _ => return None,
    })
}

fn rescale(value: u64, from: u32, to: u32) -> u64 {
    if from == to || from == 0 {
        return value;
    }
    (u128::from(value) * u128::from(to) / u128::from(from)) as u64
}

/// Run-length encode a sequence into `(count, value)` pairs.
fn run_length<T: PartialEq + Copy>(items: impl Iterator<Item = T>) -> Vec<(u32, T)> {
    let mut runs: Vec<(u32, T)> = Vec::new();
    for item in items {
        if let Some((count, value)) = runs.last_mut() {
            if *value == item {
                *count += 1;
                continue;
            }
        }
        runs.push((1, item));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_length_encoding() {
        assert_eq!(encode_descriptor_len(5), vec![5]);
        assert_eq!(encode_descriptor_len(0x80), vec![0x81, 0x00]);
        assert_eq!(encode_descriptor_len(130), vec![0x81, 0x02]);
    }

    #[test]
    fn run_length_groups_runs() {
        let runs = run_length([1000u32, 1000, 1000, 512, 512].into_iter());
        assert_eq!(runs, vec![(3, 1000), (2, 512)]);
    }

    #[test]
    fn rescale_converts_timebase() {
        assert_eq!(rescale(30_000, 15_000, 1000), 2000);
        assert_eq!(rescale(500, 1000, 1000), 500);
    }
}

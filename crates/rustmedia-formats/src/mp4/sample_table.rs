//! The sample table (`stbl`): where every sample lives and when it plays.
//!
//! MP4 stores a track's samples in a set of parallel tables — sizes (`stsz`),
//! chunk offsets (`stco`/`co64`), the sample-to-chunk map (`stsc`), decode
//! durations (`stts`), composition offsets (`ctts`), and sync-sample flags
//! (`stss`). This module reads those tables and *flattens* them into one
//! [`Sample`] per media sample, which is the representation the demuxer, the
//! trimmer, and the remuxer all operate on.

use std::io::Cursor;

use rustmedia_core::{Error, Result};
use rustmedia_io::ReadBytes;

use super::boxes::boxes_in;

/// Upper bound on speculative pre-allocation for sample-table vectors. Entry
/// counts come from the (untrusted) file, so a corrupt count must not be able
/// to request gigabytes up front — the vectors grow past this only as real
/// bytes are actually read.
const MAX_TABLE_PREALLOC: usize = 1 << 20;

/// A single flattened sample: where its bytes are and when it plays.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    /// Absolute byte offset of the sample's data within the file.
    pub offset: u64,
    /// Size of the sample's data in bytes.
    pub size: u32,
    /// Decode timestamp in the track timescale.
    pub dts: i64,
    /// Presentation timestamp in the track timescale (`dts` + composition offset).
    pub pts: i64,
    /// Sample duration in the track timescale.
    pub duration: u32,
    /// Whether this sample is a sync sample (keyframe / random-access point).
    pub is_sync: bool,
}

/// The raw `stbl` tables, before flattening.
#[derive(Default)]
struct RawTables {
    /// `(sample_count, delta)` runs from `stts`.
    stts: Vec<(u32, u32)>,
    /// `(sample_count, offset)` runs from `ctts` (offset may be signed in v1).
    ctts: Vec<(u32, i32)>,
    /// `(first_chunk, samples_per_chunk)` runs from `stsc`.
    stsc: Vec<(u32, u32)>,
    /// Per-sample sizes, or a single value if all samples share a size.
    sizes: SampleSizes,
    /// Absolute chunk offsets from `stco` or `co64`.
    chunk_offsets: Vec<u64>,
    /// 1-based sync-sample numbers from `stss`; `None` means every sample syncs.
    sync_samples: Option<Vec<u32>>,
}

enum SampleSizes {
    /// Every sample is this many bytes.
    Uniform { size: u32, count: u32 },
    /// Explicit per-sample sizes.
    PerSample(Vec<u32>),
}

impl Default for SampleSizes {
    fn default() -> Self {
        SampleSizes::Uniform { size: 0, count: 0 }
    }
}

impl SampleSizes {
    fn count(&self) -> usize {
        match self {
            SampleSizes::Uniform { count, .. } => *count as usize,
            SampleSizes::PerSample(v) => v.len(),
        }
    }

    fn size_of(&self, index: usize) -> u32 {
        match self {
            SampleSizes::Uniform { size, .. } => *size,
            SampleSizes::PerSample(v) => v.get(index).copied().unwrap_or(0),
        }
    }
}

/// Parse an `stbl` payload and flatten it into a list of samples.
pub(crate) fn parse_sample_table(payload: &[u8]) -> Result<Vec<Sample>> {
    let mut raw = RawTables::default();

    for (kind, data) in boxes_in(payload) {
        match &kind {
            b"stts" => raw.stts = parse_runs_u32(data)?,
            b"ctts" => raw.ctts = parse_ctts(data)?,
            b"stsc" => raw.stsc = parse_stsc(data)?,
            b"stsz" => raw.sizes = parse_stsz(data)?,
            b"stz2" => raw.sizes = parse_stz2(data)?,
            b"stco" => raw.chunk_offsets = parse_stco(data)?,
            b"co64" => raw.chunk_offsets = parse_co64(data)?,
            b"stss" => raw.sync_samples = Some(parse_sync_samples(data)?),
            _ => {}
        }
    }

    flatten(&raw)
}

/// Combine the parallel tables into one [`Sample`] per media sample.
fn flatten(raw: &RawTables) -> Result<Vec<Sample>> {
    let sample_count = raw.sizes.count();
    if sample_count == 0 {
        return Ok(Vec::new());
    }

    let mut samples = Vec::with_capacity(sample_count.min(MAX_TABLE_PREALLOC));

    // 1. Assign each sample to a chunk and compute its byte offset.
    //    stsc runs describe how many samples each chunk holds; the final run
    //    extends to the last chunk.
    let num_chunks = raw.chunk_offsets.len();
    let mut sample_index = 0usize;
    'chunks: for chunk_idx in 0..num_chunks {
        let samples_in_chunk = samples_in_chunk(&raw.stsc, chunk_idx as u32 + 1);
        let mut offset = raw.chunk_offsets[chunk_idx];
        for _ in 0..samples_in_chunk {
            if sample_index >= sample_count {
                break 'chunks;
            }
            let size = raw.sizes.size_of(sample_index);
            samples.push(Sample {
                offset,
                size,
                dts: 0,
                pts: 0,
                duration: 0,
                is_sync: false,
            });
            offset = offset.saturating_add(u64::from(size));
            sample_index += 1;
        }
    }

    if samples.is_empty() {
        return Err(Error::malformed(
            "mp4",
            "sample table produced no samples (missing stco/stsc)",
        ));
    }

    // 2. Decode timestamps and durations from stts.
    let mut dts: i64 = 0;
    let mut idx = 0usize;
    for &(count, delta) in &raw.stts {
        for _ in 0..count {
            if idx >= samples.len() {
                break;
            }
            samples[idx].dts = dts;
            samples[idx].duration = delta;
            dts += i64::from(delta);
            idx += 1;
        }
    }

    // 3. Composition offsets from ctts give presentation timestamps.
    if raw.ctts.is_empty() {
        for s in &mut samples {
            s.pts = s.dts;
        }
    } else {
        let mut idx = 0usize;
        for &(count, offset) in &raw.ctts {
            for _ in 0..count {
                if idx >= samples.len() {
                    break;
                }
                samples[idx].pts = samples[idx].dts + i64::from(offset);
                idx += 1;
            }
        }
        // Any samples past the ctts runs default to pts == dts.
        let n = samples.len();
        for s in &mut samples[idx.min(n)..] {
            s.pts = s.dts;
        }
    }

    // 4. Sync-sample flags.
    match &raw.sync_samples {
        None => {
            // No stss: every sample is a sync sample (typical for audio).
            for s in &mut samples {
                s.is_sync = true;
            }
        }
        Some(list) => {
            for &num in list {
                if let Some(s) = samples.get_mut((num as usize).saturating_sub(1)) {
                    s.is_sync = true;
                }
            }
        }
    }

    Ok(samples)
}

/// How many samples the 1-based `chunk` holds, per the `stsc` run table.
fn samples_in_chunk(stsc: &[(u32, u32)], chunk: u32) -> u32 {
    // Find the last run whose first_chunk <= chunk.
    let mut result = 0;
    for &(first_chunk, per_chunk) in stsc {
        if first_chunk <= chunk {
            result = per_chunk;
        } else {
            break;
        }
    }
    result
}

fn parse_runs_u32(data: &[u8]) -> Result<Vec<(u32, u32)>> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    let count = c.read_u32_be()?;
    let mut out = Vec::with_capacity((count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..count {
        let a = c.read_u32_be()?;
        let b = c.read_u32_be()?;
        out.push((a, b));
    }
    Ok(out)
}

fn parse_ctts(data: &[u8]) -> Result<Vec<(u32, i32)>> {
    let mut c = Cursor::new(data);
    let full = super::boxes::read_full_box_header(&mut c)?;
    let count = c.read_u32_be()?;
    let mut out = Vec::with_capacity((count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..count {
        let sample_count = c.read_u32_be()?;
        // Version 1 stores signed offsets; version 0 unsigned (but fits i32
        // for any realistic composition offset).
        let offset = if full.version >= 1 {
            c.read_i32_be()?
        } else {
            c.read_u32_be()? as i32
        };
        out.push((sample_count, offset));
    }
    Ok(out)
}

fn parse_stsc(data: &[u8]) -> Result<Vec<(u32, u32)>> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    let count = c.read_u32_be()?;
    let mut out = Vec::with_capacity((count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..count {
        let first_chunk = c.read_u32_be()?;
        let samples_per_chunk = c.read_u32_be()?;
        let _sample_description_index = c.read_u32_be()?;
        out.push((first_chunk, samples_per_chunk));
    }
    Ok(out)
}

fn parse_stsz(data: &[u8]) -> Result<SampleSizes> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    let sample_size = c.read_u32_be()?;
    let sample_count = c.read_u32_be()?;
    if sample_size != 0 {
        return Ok(SampleSizes::Uniform {
            size: sample_size,
            count: sample_count,
        });
    }
    let mut sizes = Vec::with_capacity((sample_count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..sample_count {
        sizes.push(c.read_u32_be()?);
    }
    Ok(SampleSizes::PerSample(sizes))
}

fn parse_stz2(data: &[u8]) -> Result<SampleSizes> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    // 24 bits reserved + 8 bits field_size.
    let _reserved = c.read_u24_be()?;
    let field_size = c.read_u8()?;
    let sample_count = c.read_u32_be()?;
    let mut sizes = Vec::with_capacity((sample_count as usize).min(MAX_TABLE_PREALLOC));
    match field_size {
        16 => {
            for _ in 0..sample_count {
                sizes.push(u32::from(c.read_u16_be()?));
            }
        }
        8 => {
            for _ in 0..sample_count {
                sizes.push(u32::from(c.read_u8()?));
            }
        }
        4 => {
            // Two 4-bit sizes packed per byte.
            let mut remaining = sample_count;
            while remaining > 0 {
                let byte = c.read_u8()?;
                sizes.push(u32::from(byte >> 4));
                remaining -= 1;
                if remaining > 0 {
                    sizes.push(u32::from(byte & 0x0F));
                    remaining -= 1;
                }
            }
        }
        other => {
            return Err(Error::malformed(
                "mp4",
                format!("stz2 field size {other} unsupported"),
            ));
        }
    }
    Ok(SampleSizes::PerSample(sizes))
}

fn parse_stco(data: &[u8]) -> Result<Vec<u64>> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    let count = c.read_u32_be()?;
    let mut out = Vec::with_capacity((count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..count {
        out.push(u64::from(c.read_u32_be()?));
    }
    Ok(out)
}

fn parse_co64(data: &[u8]) -> Result<Vec<u64>> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    let count = c.read_u32_be()?;
    let mut out = Vec::with_capacity((count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..count {
        out.push(c.read_u64_be()?);
    }
    Ok(out)
}

fn parse_sync_samples(data: &[u8]) -> Result<Vec<u32>> {
    let mut c = Cursor::new(data);
    skip_full_header(&mut c)?;
    let count = c.read_u32_be()?;
    let mut out = Vec::with_capacity((count as usize).min(MAX_TABLE_PREALLOC));
    for _ in 0..count {
        out.push(c.read_u32_be()?);
    }
    Ok(out)
}

fn skip_full_header(c: &mut Cursor<&[u8]>) -> Result<()> {
    super::boxes::read_full_box_header(c)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a minimal stbl with two chunks, three samples, all sync.
    fn full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        let size = 8 + body.len() as u32;
        v.extend_from_slice(&size.to_be_bytes());
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn flattens_simple_table() {
        // stts: 3 samples, delta 1000.
        let stts_body = [
            &1u32.to_be_bytes()[..],
            &3u32.to_be_bytes(),
            &1000u32.to_be_bytes(),
        ]
        .concat();
        let mut stts = vec![0u8; 4]; // version+flags
        stts.extend_from_slice(&stts_body);

        // stsc: chunk 1 has 2 samples, chunk 2 has 1.
        let stsc_body = [
            &2u32.to_be_bytes()[..], // entry count
            &1u32.to_be_bytes(),
            &2u32.to_be_bytes(),
            &1u32.to_be_bytes(), // desc idx
            &2u32.to_be_bytes(),
            &1u32.to_be_bytes(),
            &1u32.to_be_bytes(),
        ]
        .concat();
        let mut stsc = vec![0u8; 4];
        stsc.extend_from_slice(&stsc_body);

        // stsz: per-sample sizes 100, 200, 300.
        let stsz_body = [
            &0u32.to_be_bytes()[..], // sample_size = 0 -> per sample
            &3u32.to_be_bytes(),     // count
            &100u32.to_be_bytes(),
            &200u32.to_be_bytes(),
            &300u32.to_be_bytes(),
        ]
        .concat();
        let mut stsz = vec![0u8; 4];
        stsz.extend_from_slice(&stsz_body);

        // stco: chunk offsets 1000, 5000.
        let stco_body = [
            &2u32.to_be_bytes()[..],
            &1000u32.to_be_bytes(),
            &5000u32.to_be_bytes(),
        ]
        .concat();
        let mut stco = vec![0u8; 4];
        stco.extend_from_slice(&stco_body);

        let mut stbl = Vec::new();
        stbl.extend(full_box(b"stts", &stts));
        stbl.extend(full_box(b"stsc", &stsc));
        stbl.extend(full_box(b"stsz", &stsz));
        stbl.extend(full_box(b"stco", &stco));

        let samples = parse_sample_table(&stbl).unwrap();
        assert_eq!(samples.len(), 3);
        // Chunk 1: samples at 1000 (size 100) and 1100 (size 200).
        assert_eq!(samples[0].offset, 1000);
        assert_eq!(samples[0].size, 100);
        assert_eq!(samples[1].offset, 1100);
        assert_eq!(samples[1].size, 200);
        // Chunk 2: sample at 5000 (size 300).
        assert_eq!(samples[2].offset, 5000);
        assert_eq!(samples[2].size, 300);
        // Timing: dts 0, 1000, 2000; all sync (no stss).
        assert_eq!(samples[2].dts, 2000);
        assert!(samples.iter().all(|s| s.is_sync));
        assert_eq!(samples[0].pts, 0);
    }
}

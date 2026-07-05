//! High-level, lossless media operations: [`remux`], [`trim`], and [`extract`].
//!
//! All three copy coded packets straight from the input to a new container
//! without decoding or re-encoding. They share one engine ([`copy_stream`]) and
//! differ only in which tracks and time range they keep.

use std::collections::{HashMap, HashSet};
use std::io::BufWriter;
use std::path::Path;
use std::time::Duration;

use rustmedia_core::{Error, MediaType, Result, Track};
use rustmedia_formats::{Mp4Muxer, Muxer};

use crate::Media;

/// What a copy operation produced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyStats {
    /// Number of packets written.
    pub packets: u64,
    /// Number of payload bytes written.
    pub bytes: u64,
    /// Number of tracks written.
    pub tracks: usize,
}

/// Selects which tracks an operation keeps.
#[derive(Debug, Clone)]
pub enum TrackSelector {
    /// Keep every track.
    All,
    /// Keep all tracks of a media type (video/audio/subtitle).
    Kind(MediaType),
    /// Keep the single track with this id.
    Id(u32),
}

impl TrackSelector {
    fn matches(&self, track: &Track) -> bool {
        match self {
            TrackSelector::All => true,
            TrackSelector::Kind(kind) => track.media_type == *kind,
            TrackSelector::Id(id) => track.id == *id,
        }
    }
}

/// Remux `input` into `output`, copying every track without re-encoding.
///
/// The output container is chosen from `output`'s file extension. Only MP4-family
/// outputs (`.mp4`, `.m4a`, `.m4v`, `.mov`) are supported today.
///
/// # Errors
/// Fails if the input cannot be opened, the output extension is unsupported, or
/// a track's codec cannot be written to the target container.
pub fn remux(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<CopyStats> {
    let mut media = Media::open(input)?;
    let mut muxer = muxer_for(output.as_ref())?;
    copy_stream(&mut media, muxer.as_mut(), &TrackSelector::All, None, None)
}

/// Options controlling [`trim`].
#[derive(Debug, Clone, Default)]
pub struct TrimOptions {
    /// Start of the kept range. `None` means the beginning.
    pub start: Option<Duration>,
    /// End of the kept range. `None` means the end of the file.
    pub end: Option<Duration>,
}

/// Trim `input` to the `[start, end)` range in `options`, writing `output`.
///
/// The cut is keyframe-aware: the demuxer seeks to the keyframe at or before
/// `start` so the result is decodable, and timestamps are rebased so the output
/// begins at zero. This is always a lossless copy — no re-encoding.
///
/// # Errors
/// Fails like [`remux`], and if the input format cannot seek.
pub fn trim(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: &TrimOptions,
) -> Result<CopyStats> {
    let mut media = Media::open(input)?;
    let mut muxer = muxer_for(output.as_ref())?;
    copy_stream(
        &mut media,
        muxer.as_mut(),
        &TrackSelector::All,
        options.start,
        options.end,
    )
}

/// Extract the tracks selected by `selector` from `input` into `output`.
///
/// # Errors
/// Fails like [`remux`]; also errors if the selector matches no track.
pub fn extract(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    selector: &TrackSelector,
) -> Result<CopyStats> {
    let mut media = Media::open(input)?;
    if !media.tracks().iter().any(|t| selector.matches(t)) {
        return Err(Error::invalid_argument("no track matched the selection"));
    }
    let mut muxer = muxer_for(output.as_ref())?;
    copy_stream(&mut media, muxer.as_mut(), selector, None, None)
}

/// The engine behind all three operations: select tracks, optionally window by
/// time, rebase timestamps, and copy packets into the muxer.
fn copy_stream(
    media: &mut Media,
    muxer: &mut dyn Muxer,
    selector: &TrackSelector,
    start: Option<Duration>,
    end: Option<Duration>,
) -> Result<CopyStats> {
    // Which track ids are kept, plus their timescales for time math.
    let kept: Vec<Track> = media
        .tracks()
        .iter()
        .filter(|t| selector.matches(t))
        .cloned()
        .collect();
    let kept_ids: HashSet<u32> = kept.iter().map(|t| t.id).collect();
    let timescales: HashMap<u32, u32> = kept.iter().map(|t| (t.id, t.timescale.max(1))).collect();

    muxer.start(&kept)?;
    muxer.set_metadata(media.metadata())?;

    if let Some(start) = start {
        media.seek(start)?;
    }
    let end_secs = end.map(|d| d.as_secs_f64());

    // Rebase so the first emitted packet sits at timestamp zero (per track,
    // using one shared wall-clock reference to preserve A/V sync).
    let mut reference_secs: Option<f64> = None;
    let mut finished: HashSet<u32> = HashSet::new();

    let mut stats = CopyStats {
        tracks: kept.len(),
        ..Default::default()
    };

    while let Some(mut packet) = media.read_packet()? {
        if !kept_ids.contains(&packet.track_id) || finished.contains(&packet.track_id) {
            continue;
        }
        let timescale = *timescales.get(&packet.track_id).unwrap_or(&1);
        let ticks = packet.pts.or(packet.dts).unwrap_or(0);
        let time_secs = ticks as f64 / f64::from(timescale);

        if let Some(end_secs) = end_secs {
            if time_secs >= end_secs {
                finished.insert(packet.track_id);
                if finished.len() == kept_ids.len() {
                    break;
                }
                continue;
            }
        }

        // Establish the shared reference from the first packet we keep.
        let reference = *reference_secs.get_or_insert_with(|| {
            packet.dts.or(packet.pts).unwrap_or(0) as f64 / f64::from(timescale)
        });
        let shift = (reference * f64::from(timescale)) as i64;
        if let Some(dts) = packet.dts.as_mut() {
            *dts = (*dts - shift).max(0);
        }
        if let Some(pts) = packet.pts.as_mut() {
            *pts = (*pts - shift).max(0);
        }

        stats.packets += 1;
        stats.bytes += packet.data.len() as u64;
        muxer.write_packet(&packet)?;
    }

    muxer.finish()?;
    Ok(stats)
}

/// Build a muxer for the given output path based on its extension.
fn muxer_for(output: &Path) -> Result<Box<dyn Muxer>> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "m4a" | "m4v" | "mov" | "m4b" => {
            let file = std::fs::File::create(output)?;
            Ok(Box::new(Mp4Muxer::new(BufWriter::new(file))))
        }
        other => Err(Error::unsupported(format!(
            "no muxer for output extension '.{other}'; try .mp4 or .m4a"
        ))),
    }
}

//! `remux`, `trim`, and `extract` — the lossless copy commands.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rustmedia::{ops, parse_duration, MediaType, TrackSelector, TrimOptions};

use crate::ui;

/// Arguments for `rustmedia remux`.
#[derive(clap::Args, Debug)]
pub(crate) struct RemuxArgs {
    /// Input media file.
    pub(crate) input: PathBuf,
    /// Target container format / extension (e.g. `mp4`, `m4a`).
    #[arg(long, default_value = "mp4")]
    pub(crate) to: String,
    /// Output file (defaults to the input name with the new extension).
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,
}

/// Arguments for `rustmedia trim`.
#[derive(clap::Args, Debug)]
pub(crate) struct TrimArgs {
    /// Input media file.
    pub(crate) input: PathBuf,
    /// Start time (e.g. `10s`, `1:30`, `00:01:30.5`).
    #[arg(long)]
    pub(crate) from: Option<String>,
    /// End time (e.g. `30s`, `2:15`).
    #[arg(long, value_name = "TIME")]
    pub(crate) to: Option<String>,
    /// Copy without re-encoding (the only supported mode; accepted for clarity).
    #[arg(long, default_value_t = true)]
    pub(crate) copy: bool,
    /// Output file.
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,
}

/// Arguments for `rustmedia extract`.
#[derive(clap::Args, Debug)]
pub(crate) struct ExtractArgs {
    /// Input media file.
    pub(crate) input: PathBuf,
    /// Which track(s) to extract: `video`, `audio`, `subtitle`, or a track id.
    #[arg(long)]
    pub(crate) track: String,
    /// Output file.
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,
}

pub(crate) fn run_remux(args: &RemuxArgs) -> Result<()> {
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| with_extension(&args.input, &args.to));
    let stats = ops::remux(&args.input, &output)
        .with_context(|| format!("remuxing '{}'", args.input.display()))?;
    report("remuxed", &args.input, &output, stats);
    Ok(())
}

pub(crate) fn run_trim(args: &TrimArgs) -> Result<()> {
    let start = args.from.as_deref().map(parse_duration).transpose()?;
    let end = args.to.as_deref().map(parse_duration).transpose()?;
    if let (Some(s), Some(e)) = (start, end) {
        if e <= s {
            bail!("--to ({e:?}) must be after --from ({s:?})");
        }
    }
    let _ = args.copy; // copy is always on; re-encode is a future mode.

    let output = args
        .output
        .clone()
        .unwrap_or_else(|| suffixed(&args.input, "trimmed"));
    let stats = ops::trim(&args.input, &output, &TrimOptions { start, end })
        .with_context(|| format!("trimming '{}'", args.input.display()))?;
    report("trimmed", &args.input, &output, stats);
    Ok(())
}

pub(crate) fn run_extract(args: &ExtractArgs) -> Result<()> {
    let selector = parse_selector(&args.track)?;
    let default_ext = "m4a";
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| suffixed_ext(&args.input, &args.track, default_ext));
    let stats = ops::extract(&args.input, &output, &selector)
        .with_context(|| format!("extracting from '{}'", args.input.display()))?;
    report("extracted", &args.input, &output, stats);
    Ok(())
}

fn parse_selector(spec: &str) -> Result<TrackSelector> {
    Ok(match spec.to_ascii_lowercase().as_str() {
        "video" | "v" => TrackSelector::Kind(MediaType::Video),
        "audio" | "a" => TrackSelector::Kind(MediaType::Audio),
        "subtitle" | "sub" | "s" => TrackSelector::Kind(MediaType::Subtitle),
        other => match other.parse::<u32>() {
            Ok(id) => TrackSelector::Id(id),
            Err(_) => bail!("invalid --track '{spec}': use video, audio, subtitle, or a track id"),
        },
    })
}

fn report(verb: &str, input: &Path, output: &Path, stats: ops::CopyStats) {
    let name = input.file_name().map_or_else(
        || input.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let summary = format!(
        "{verb} {name} → {} ({} tracks, {} packets, {})",
        output.display(),
        stats.tracks,
        stats.packets,
        ui::size(stats.bytes),
    );
    println!("{}", ui::green(&summary));
}

fn with_extension(input: &Path, ext: &str) -> PathBuf {
    input.with_extension(ext.trim_start_matches('.'))
}

fn suffixed(input: &Path, suffix: &str) -> PathBuf {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    suffixed_ext(input, suffix, ext)
}

fn suffixed_ext(input: &Path, suffix: &str, ext: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = format!("{stem}_{suffix}.{ext}").replace(['/', '\\'], "_");
    input.with_file_name(name)
}

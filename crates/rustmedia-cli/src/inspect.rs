//! `rustmedia inspect` — show a file's format, tracks, metadata, and chapters.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rustmedia::{format_duration, Media, MediaType, Track};

use crate::ui;

/// Arguments for the `inspect` subcommand.
#[derive(clap::Args, Debug)]
pub(crate) struct InspectArgs {
    /// Path to the media file to inspect.
    pub(crate) file: PathBuf,

    /// Emit machine-readable JSON instead of the human-readable summary.
    #[arg(long)]
    pub(crate) json: bool,
}

/// Run the `inspect` command.
pub(crate) fn run(args: &InspectArgs) -> Result<()> {
    let media = Media::open(&args.file)
        .with_context(|| format!("could not inspect '{}'", args.file.display()))?;

    if args.json {
        print_json(&media, &args.file)?;
    } else {
        print_human(&media, &args.file);
    }
    Ok(())
}

fn print_human(media: &Media, path: &std::path::Path) {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    println!("{}", ui::bold(&name));

    let format = format!("{} ({})", media.format(), media.format().mime_type());
    field("format", &format);

    if let Some(d) = media.duration() {
        field(
            "duration",
            &format!("{}  ({:.3} s)", format_duration(d), d.as_secs_f64()),
        );
    }
    if media.size_bytes() > 0 {
        field("size", &ui::size(media.size_bytes()));
    }

    let tracks = media.tracks();
    println!("\n  {} ({})", ui::bold("streams"), tracks.len());
    for track in tracks {
        println!("    {}", stream_line(track));
    }

    let meta = media.metadata();
    if meta.iter().next().is_some() {
        println!("\n  {}", ui::bold("metadata"));
        let width = meta.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in meta.iter() {
            println!("    {:<width$}  {}", ui::dim(k), v, width = width);
        }
    }

    if !meta.chapters.is_empty() {
        println!("\n  {} ({})", ui::bold("chapters"), meta.chapters.len());
        for (i, ch) in meta.chapters.iter().enumerate() {
            println!(
                "    {:>2}. {}  {}",
                i + 1,
                format_duration(ch.start.to_duration()),
                ch.title
            );
        }
    }
}

/// Render one stream as an aligned single line.
fn stream_line(track: &Track) -> String {
    let id = ui::dim(&format!("#{}", track.id));
    let kind = ui::cyan(&format!("{:<8}", track.media_type.as_str()));
    let codec = ui::green(&format!("{:<8}", track.codec.name()));

    let mut details = Vec::new();
    match track.media_type {
        MediaType::Video => {
            if let Some(v) = track.video() {
                details.push(format!("{}×{}", v.width, v.height));
                if let Some(rate) = v.fps() {
                    details.push(ui::fps(rate));
                }
                if let Some(depth) = v.bit_depth {
                    if depth != 8 {
                        details.push(format!("{depth}-bit"));
                    }
                }
            }
        }
        MediaType::Audio => {
            if let Some(a) = track.audio() {
                details.push(ui::sample_rate(a.sample_rate));
                details.push(ui::channels(a.channels));
            }
        }
        _ => {}
    }
    if let Some(lang) = &track.language {
        details.push(lang.clone());
    }
    if let Some(br) = track.bitrate {
        details.push(ui::bitrate(br));
    }

    format!("{id}  {kind}{codec}  {}", details.join("   "))
}

fn field(label: &str, value: &str) {
    println!("  {:<9} {}", ui::dim(label), value);
}

// -------------------------------------------------------------------------
// JSON output
// -------------------------------------------------------------------------

fn print_json(media: &Media, path: &std::path::Path) -> Result<()> {
    let report = build_report(media, path);
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

fn build_report(media: &Media, path: &std::path::Path) -> serde_json::Value {
    use serde_json::json;

    let tracks: Vec<serde_json::Value> = media.tracks().iter().map(track_json).collect();
    let metadata: serde_json::Map<String, serde_json::Value> = media
        .metadata()
        .iter()
        .map(|(k, v)| (k.to_string(), json!(v)))
        .collect();
    let chapters: Vec<serde_json::Value> = media
        .metadata()
        .chapters
        .iter()
        .map(|c| json!({ "start": c.start.seconds(), "title": c.title }))
        .collect();

    json!({
        "path": path.display().to_string(),
        "format": media.format().name(),
        "mime_type": media.format().mime_type(),
        "duration_secs": media.duration().map(|d| d.as_secs_f64()),
        "size_bytes": media.size_bytes(),
        "streams": tracks,
        "metadata": metadata,
        "chapters": chapters,
    })
}

fn track_json(track: &Track) -> serde_json::Value {
    use serde_json::json;

    let mut obj = json!({
        "id": track.id,
        "type": track.media_type.as_str(),
        "codec": track.codec.name(),
        "timescale": track.timescale,
        "duration_secs": track.duration().map(|d| d.as_secs_f64()),
        "language": track.language,
        "bitrate": track.bitrate,
    });
    let map = obj.as_object_mut().unwrap();
    if let Some(v) = track.video() {
        map.insert("width".into(), json!(v.width));
        map.insert("height".into(), json!(v.height));
        map.insert("fps".into(), json!(v.fps()));
    }
    if let Some(a) = track.audio() {
        map.insert("sample_rate".into(), json!(a.sample_rate));
        map.insert("channels".into(), json!(a.channels));
    }
    obj
}

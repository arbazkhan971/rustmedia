//! The `rustmedia` command-line tool.
//!
//! A fast, safe, FFmpeg-free media toolkit. Run `rustmedia --help` for the
//! full command list.

mod inspect;
mod transform;
mod ui;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Fast, safe, FFmpeg-free media toolkit: inspect, and (soon) remux, trim, and
/// extract media files.
#[derive(Parser, Debug)]
#[command(
    name = "rustmedia",
    version,
    about,
    long_about = None,
    propagate_version = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Inspect a media file: format, tracks, duration, metadata, chapters.
    Inspect(inspect::InspectArgs),
    /// Remux to another container without re-encoding (e.g. MOV → MP4).
    Remux(transform::RemuxArgs),
    /// Trim to a time range, keyframe-aware and lossless.
    Trim(transform::TrimArgs),
    /// Extract selected tracks into a new file without re-encoding.
    Extract(transform::ExtractArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Inspect(args) => inspect::run(&args),
        Command::Remux(args) => transform::run_remux(&args),
        Command::Trim(args) => transform::run_trim(&args),
        Command::Extract(args) => transform::run_extract(&args),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Print the full anyhow chain, most-recent cause last.
            eprintln!("{} {err:#}", ui::error_prefix());
            ExitCode::FAILURE
        }
    }
}

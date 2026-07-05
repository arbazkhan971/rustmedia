//! RustMedia development tasks, invoked as `cargo xtask <command>`.
//!
//! Currently supports fixture generation. Fixtures are small synthetic media
//! files produced with FFmpeg; they are used by the integration tests and are
//! regenerated rather than committed when large.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gen-fixtures") => match gen_fixtures() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask: {e}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("xtask: unknown command '{other}'");
            usage();
            ExitCode::FAILURE
        }
        None => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!("usage: cargo xtask <command>\n\ncommands:\n  gen-fixtures   generate synthetic test media with ffmpeg");
}

fn workspace_root() -> PathBuf {
    // xtask/ lives directly under the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Generate the synthetic media corpus used by integration tests.
fn gen_fixtures() -> Result<(), String> {
    let out = workspace_root().join("testdata/generated");
    std::fs::create_dir_all(&out).map_err(|e| format!("creating {}: {e}", out.display()))?;

    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return Err("ffmpeg not found on PATH; install it to generate fixtures".into());
    }

    // (filename, ffmpeg args) — each produces one small deterministic fixture.
    let jobs: &[(&str, &[&str])] = &[
        (
            "av_2s.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-metadata",
                "title=RustMedia Test",
                "-movflags",
                "+faststart",
            ],
        ),
        (
            "av_2s.mkv",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "200k",
                "-c:a",
                "libopus",
                "-metadata",
                "title=RustMedia Test",
            ],
        ),
        (
            "av_2s.webm",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=30:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "200k",
                "-c:a",
                "libopus",
            ],
        ),
        (
            "tone_1s.wav",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "pcm_s16le",
            ],
        ),
        (
            "tone_1s.mp3",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "128k",
                "-metadata",
                "title=RustMedia Tone",
                "-metadata",
                "artist=RustMedia",
            ],
        ),
    ];

    for (name, args) in jobs {
        let path = out.join(name);
        println!("generating {}", path.display());
        let status = Command::new("ffmpeg")
            .arg("-y")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .args(*args)
            .arg(&path)
            .status()
            .map_err(|e| format!("running ffmpeg for {name}: {e}"))?;
        if !status.success() {
            return Err(format!("ffmpeg failed for {name}"));
        }
    }

    println!("fixtures written to {}", out.display());
    Ok(())
}

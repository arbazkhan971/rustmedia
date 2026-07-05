//! Fixture-based round-trips for the remux/trim/extract operations. Skips
//! gracefully when the generated corpus is absent.

use std::path::PathBuf;
use std::time::Duration;

use rustmedia::{ops, Media, MediaType, TrackSelector, TrimOptions};

fn fixture(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/generated")
        .join(name);
    p.exists().then_some(p)
}

fn out(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rustmedia_test_{name}"))
}

#[test]
fn remux_preserves_tracks() {
    let Some(input) = fixture("av_2s.mp4") else {
        return;
    };
    let output = out("remux.mp4");

    let stats = ops::remux(&input, &output).expect("remux");
    assert_eq!(stats.tracks, 2);
    assert!(stats.packets > 60);

    // Reopen the muxed output with our own parser.
    let media = Media::open(&output).expect("reopen remuxed");
    assert_eq!(media.tracks().len(), 2);
    assert!(media.best_video().is_some());
    assert!(media.best_audio().is_some());
    assert_eq!(media.best_video().unwrap().codec.name(), "h264");
    let v = media.best_video().unwrap().video().unwrap();
    assert_eq!((v.width, v.height), (320, 240));

    let _ = std::fs::remove_file(&output);
}

#[test]
fn trim_shortens_output() {
    let Some(input) = fixture("av_2s.mp4") else {
        return;
    };
    let output = out("trim.mp4");

    ops::trim(
        &input,
        &output,
        &TrimOptions {
            start: Some(Duration::from_millis(0)),
            end: Some(Duration::from_secs(1)),
        },
    )
    .expect("trim");

    let media = Media::open(&output).expect("reopen trimmed");
    let dur = media.duration().unwrap().as_secs_f64();
    assert!(dur <= 1.3, "trimmed duration {dur} should be ~1s");
    assert_eq!(media.tracks().len(), 2);

    let _ = std::fs::remove_file(&output);
}

#[test]
fn extract_audio_only() {
    let Some(input) = fixture("av_2s.mp4") else {
        return;
    };
    let output = out("audio.m4a");

    ops::extract(&input, &output, &TrackSelector::Kind(MediaType::Audio)).expect("extract");

    let media = Media::open(&output).expect("reopen extracted");
    assert_eq!(media.tracks().len(), 1);
    assert_eq!(media.tracks()[0].media_type, MediaType::Audio);
    assert_eq!(media.tracks()[0].codec.name(), "aac");

    let _ = std::fs::remove_file(&output);
}

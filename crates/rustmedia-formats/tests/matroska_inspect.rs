//! Integration tests for the Matroska/WebM demuxer against the generated corpus.

use std::path::PathBuf;

use rustmedia_core::{Codec, ContainerFormat, MediaType};
use rustmedia_formats::open;

fn fixture(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/generated")
        .join(name);
    p.exists().then_some(p)
}

#[test]
fn inspects_vp9_opus_mkv() {
    let Some(path) = fixture("av_2s.mkv") else {
        return;
    };
    let mut demuxer = open(std::fs::File::open(path).unwrap()).expect("open mkv");

    assert_eq!(demuxer.format(), ContainerFormat::Matroska);
    assert_eq!(demuxer.tracks().len(), 2);

    let video = demuxer.tracks().iter().find(|t| t.is_video()).unwrap();
    assert_eq!(video.codec, Codec::Vp9);
    assert_eq!(
        (video.video().unwrap().width, video.video().unwrap().height),
        (320, 240)
    );

    let audio = demuxer.tracks().iter().find(|t| t.is_audio()).unwrap();
    assert_eq!(audio.codec, Codec::Opus);
    assert_eq!(audio.audio().unwrap().sample_rate, 48_000);

    assert_eq!(demuxer.metadata().title(), Some("RustMedia Test"));

    let dur = demuxer.duration().unwrap().as_secs_f64();
    assert!((dur - 2.0).abs() < 0.1, "duration {dur}");

    // Drain the whole file; both tracks should produce packets.
    let mut video_packets = 0;
    let mut audio_packets = 0;
    let (vid_id, aud_id) = (video.id, audio.id);
    while let Some(p) = demuxer.read_packet().unwrap() {
        if p.track_id == vid_id {
            video_packets += 1;
        } else if p.track_id == aud_id {
            audio_packets += 1;
        }
    }
    assert!(video_packets > 30, "video packets {video_packets}");
    assert!(audio_packets > 30, "audio packets {audio_packets}");
}

#[test]
fn detects_webm_doctype() {
    let Some(path) = fixture("av_2s.webm") else {
        return;
    };
    let demuxer = open(std::fs::File::open(path).unwrap()).expect("open webm");
    // Same bytes family as MKV, but the DocType makes it WebM.
    assert_eq!(demuxer.format(), ContainerFormat::WebM);
    let kinds: Vec<MediaType> = demuxer.tracks().iter().map(|t| t.media_type).collect();
    assert!(kinds.contains(&MediaType::Video));
    assert!(kinds.contains(&MediaType::Audio));
}

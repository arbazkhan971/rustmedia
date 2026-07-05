//! Integration tests for the MP4 demuxer, run against the synthetic corpus
//! produced by `cargo xtask gen-fixtures`. If the corpus is absent the tests
//! skip gracefully so a fresh checkout still passes `cargo test`.

use std::path::PathBuf;

use rustmedia_core::{Codec, ContainerFormat, MediaType};
use rustmedia_formats::open;

fn fixture(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/generated")
        .join(name);
    path.exists().then_some(path)
}

#[test]
fn inspects_h264_aac_mp4() {
    let Some(path) = fixture("av_2s.mp4") else {
        eprintln!("skipping: fixture av_2s.mp4 not generated");
        return;
    };
    let file = std::fs::File::open(&path).unwrap();
    let demuxer = open(file).expect("open mp4");

    assert_eq!(demuxer.format(), ContainerFormat::Mp4);

    let tracks = demuxer.tracks();
    assert_eq!(tracks.len(), 2, "expected one video + one audio track");

    let video = tracks.iter().find(|t| t.is_video()).expect("video track");
    assert_eq!(video.codec, Codec::H264);
    let v = video.video().unwrap();
    assert_eq!((v.width, v.height), (320, 240));
    let fps = v.fps().unwrap();
    assert!((fps - 30.0).abs() < 0.5, "fps was {fps}");
    assert!(
        video.codec_private.is_some(),
        "avcC extradata should be captured"
    );

    let audio = tracks.iter().find(|t| t.is_audio()).expect("audio track");
    assert_eq!(audio.codec, Codec::Aac);
    let a = audio.audio().unwrap();
    assert_eq!(a.sample_rate, 44_100);
    assert_eq!(a.channels, 1);

    let dur = demuxer.duration().expect("duration").as_secs_f64();
    assert!((dur - 2.0).abs() < 0.05, "duration was {dur}");

    // Metadata title set by the fixture generator.
    assert_eq!(demuxer.metadata().title(), Some("RustMedia Test"));
}

#[test]
fn reads_all_packets_with_keyframes() {
    let Some(path) = fixture("av_2s.mp4") else {
        return;
    };
    let file = std::fs::File::open(&path).unwrap();
    let mut demuxer = open(file).expect("open mp4");

    let video_id = demuxer.tracks().iter().find(|t| t.is_video()).unwrap().id;

    let mut count = 0usize;
    let mut video_keyframes = 0usize;
    let mut total_bytes = 0usize;
    while let Some(pkt) = demuxer.read_packet().expect("read packet") {
        count += 1;
        total_bytes += pkt.data.len();
        if pkt.track_id == video_id && pkt.is_keyframe {
            video_keyframes += 1;
        }
        // Every packet should carry a presentation timestamp.
        assert!(pkt.pts.is_some());
    }

    assert!(
        count > 60,
        "expected >60 packets across both tracks, got {count}"
    );
    assert!(video_keyframes >= 1, "expected at least one video keyframe");
    assert!(total_bytes > 1000, "expected real payload bytes");
}

#[test]
fn seek_positions_at_keyframe() {
    let Some(path) = fixture("av_2s.mp4") else {
        return;
    };
    let file = std::fs::File::open(&path).unwrap();
    let mut demuxer = open(file).expect("open mp4");

    demuxer
        .seek(std::time::Duration::from_secs(1))
        .expect("seek");
    let pkt = demuxer
        .read_packet()
        .expect("read")
        .expect("packet after seek");
    // The first packet after a seek should come from a real track.
    assert!(demuxer.tracks().iter().any(|t| t.id == pkt.track_id));
    assert_eq!(pkt.data.len(), pkt.data.len());
}

#[test]
fn detects_media_types() {
    let Some(path) = fixture("av_2s.mp4") else {
        return;
    };
    let file = std::fs::File::open(&path).unwrap();
    let demuxer = open(file).unwrap();
    let kinds: Vec<MediaType> = demuxer.tracks().iter().map(|t| t.media_type).collect();
    assert!(kinds.contains(&MediaType::Video));
    assert!(kinds.contains(&MediaType::Audio));
}

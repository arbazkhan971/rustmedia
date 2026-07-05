//! Integration tests for the WAV and MP3 demuxers against the generated corpus.

use std::path::PathBuf;

use rustmedia_core::{Codec, ContainerFormat};
use rustmedia_formats::open;

fn fixture(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/generated")
        .join(name);
    path.exists().then_some(path)
}

#[test]
fn inspects_pcm_wav() {
    let Some(path) = fixture("tone_1s.wav") else {
        return;
    };
    let mut demuxer = open(std::fs::File::open(path).unwrap()).expect("open wav");

    assert_eq!(demuxer.format(), ContainerFormat::Wav);
    assert_eq!(demuxer.tracks().len(), 1);

    let track = &demuxer.tracks()[0];
    assert_eq!(track.codec, Codec::PcmS16Le);
    let a = track.audio().unwrap();
    assert_eq!(a.sample_rate, 44_100);
    assert_eq!(a.channels, 1);
    assert_eq!(a.bits_per_sample, Some(16));

    let dur = demuxer.duration().unwrap().as_secs_f64();
    assert!((dur - 1.0).abs() < 0.01, "duration {dur}");

    // Stream the whole file; bytes should roughly equal 1s of 16-bit mono audio.
    let mut bytes = 0usize;
    while let Some(p) = demuxer.read_packet().unwrap() {
        bytes += p.data.len();
    }
    let expected: i64 = 44_100 * 2; // 1s * 44100 frames * 2 bytes
    assert!((bytes as i64 - expected).abs() < 4096, "got {bytes} bytes");
}

#[test]
fn inspects_mp3_with_id3() {
    let Some(path) = fixture("tone_1s.mp3") else {
        return;
    };
    let mut demuxer = open(std::fs::File::open(path).unwrap()).expect("open mp3");

    assert_eq!(demuxer.format(), ContainerFormat::Mp3);
    let track = &demuxer.tracks()[0];
    assert_eq!(track.codec, Codec::Mp3);
    let a = track.audio().unwrap();
    assert_eq!(a.sample_rate, 44_100);
    assert_eq!(a.channels, 1);

    let dur = demuxer.duration().unwrap().as_secs_f64();
    assert!((0.9..1.2).contains(&dur), "duration {dur}");

    assert_eq!(demuxer.metadata().title(), Some("RustMedia Tone"));
    assert_eq!(demuxer.metadata().artist(), Some("RustMedia"));

    // Every MPEG frame is a keyframe packet.
    let mut frames = 0usize;
    while let Some(p) = demuxer.read_packet().unwrap() {
        assert!(p.is_keyframe);
        assert!(!p.data.is_empty());
        frames += 1;
    }
    assert!(frames > 20, "expected many MP3 frames, got {frames}");
}

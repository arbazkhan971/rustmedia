//! In-memory round-trip: mux synthetic packets into an MP4, then demux them
//! back with RustMedia's own parser. Requires no external tools, so it always
//! runs in CI.

use std::io::Cursor;

use rustmedia_core::{
    AudioParameters, Codec, MediaType, Packet, Track, TrackParameters, VideoParameters,
};
use rustmedia_formats::{Demuxer, Mp4Demuxer, Mp4Muxer, Muxer};

fn pcm_track() -> Track {
    Track {
        id: 7, // deliberately not 1, to test id remapping
        codec: Codec::PcmS16Le,
        media_type: MediaType::Audio,
        timescale: 44_100,
        duration: None,
        language: Some("eng".to_string()),
        name: None,
        bitrate: None,
        codec_private: None,
        parameters: TrackParameters::Audio(AudioParameters {
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: Some(16),
        }),
    }
}

#[test]
fn mux_then_demux_pcm() {
    let mut buffer: Vec<u8> = Vec::new();
    {
        let mut muxer = Mp4Muxer::new(&mut buffer);
        muxer.start(std::slice::from_ref(&pcm_track())).unwrap();

        for i in 0..5u32 {
            let data = vec![i as u8; 1024];
            let pkt = Packet::new(7, data)
                .with_pts(i64::from(i) * 512)
                .with_dts(i64::from(i) * 512)
                .with_duration(512)
                .keyframe(true);
            muxer.write_packet(&pkt).unwrap();
        }
        muxer.finish().unwrap();
    }

    assert!(!buffer.is_empty());

    // Demux it back.
    let mut demuxer = Mp4Demuxer::new(Cursor::new(buffer)).expect("demux muxed file");
    let tracks = demuxer.tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].codec, Codec::PcmS16Le);
    assert_eq!(tracks[0].language.as_deref(), Some("eng"));
    let audio = tracks[0].audio().unwrap();
    assert_eq!(audio.sample_rate, 44_100);
    assert_eq!(audio.channels, 2);

    let mut count = 0;
    let mut total = 0usize;
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        count += 1;
        total += pkt.data.len();
        assert!(pkt.is_keyframe);
    }
    assert_eq!(count, 5);
    assert_eq!(total, 5 * 1024);
}

#[test]
fn mux_video_with_extradata_and_ctts() {
    // A video track with codec-private (avcC-shaped) data and B-frame-style
    // composition offsets, to exercise the stsd config box and ctts writing.
    let track = Track {
        id: 1,
        codec: Codec::H264,
        media_type: MediaType::Video,
        timescale: 30_000,
        duration: None,
        language: None,
        name: None,
        bitrate: None,
        codec_private: Some(vec![0x01, 0x64, 0x00, 0x1f, 0xff]), // stand-in avcC bytes
        parameters: TrackParameters::Video(VideoParameters {
            width: 640,
            height: 480,
            frame_rate: None,
            display_aspect_ratio: None,
            bit_depth: None,
        }),
    };

    let mut buffer: Vec<u8> = Vec::new();
    {
        let mut muxer = Mp4Muxer::new(&mut buffer);
        muxer.start(std::slice::from_ref(&track)).unwrap();
        // dts 0,1000,2000; pts reordered so ctts offsets are non-zero.
        let pts = [0i64, 2000, 1000];
        for (i, &p) in pts.iter().enumerate() {
            let pkt = Packet::new(1, vec![0xAB; 500])
                .with_dts(i as i64 * 1000)
                .with_pts(p)
                .with_duration(1000)
                .keyframe(i == 0);
            muxer.write_packet(&pkt).unwrap();
        }
        muxer.finish().unwrap();
    }

    let demuxer = Mp4Demuxer::new(Cursor::new(buffer)).expect("demux");
    let t = &demuxer.tracks()[0];
    assert_eq!(t.codec, Codec::H264);
    assert_eq!(
        t.codec_private.as_deref(),
        Some(&[0x01, 0x64, 0x00, 0x1f, 0xff][..])
    );
    let v = t.video().unwrap();
    assert_eq!((v.width, v.height), (640, 480));
}

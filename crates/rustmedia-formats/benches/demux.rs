//! Throughput benchmarks for RustMedia's demuxers.
//!
//! Everything is built in memory (no fixtures, no ffmpeg) so the benchmarks are
//! deterministic and run anywhere: `cargo bench -p rustmedia-formats`.
#![allow(missing_docs)] // criterion's generated harness exposes undocumented items

use std::hint::black_box;
use std::io::Cursor;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use rustmedia_core::{AudioParameters, Codec, MediaType, Packet, Track, TrackParameters};
use rustmedia_formats::{Demuxer, Mp4Demuxer, Mp4Muxer, Muxer, WavDemuxer};

/// Build a synthetic MP4 with `frames` PCM packets of `frame_bytes` each.
fn synth_mp4(frames: u32, frame_bytes: usize) -> Vec<u8> {
    let track = Track {
        id: 1,
        codec: Codec::PcmS16Le,
        media_type: MediaType::Audio,
        timescale: 48_000,
        duration: None,
        language: None,
        name: None,
        bitrate: None,
        codec_private: None,
        parameters: TrackParameters::Audio(AudioParameters {
            sample_rate: 48_000,
            channels: 2,
            bits_per_sample: Some(16),
        }),
    };
    let mut buf = Vec::new();
    let mut muxer = Mp4Muxer::new(&mut buf);
    muxer.start(std::slice::from_ref(&track)).unwrap();
    for i in 0..frames {
        let pkt = Packet::new(1, vec![(i & 0xFF) as u8; frame_bytes])
            .with_dts(i64::from(i) * 1024)
            .with_pts(i64::from(i) * 1024)
            .with_duration(1024)
            .keyframe(true);
        muxer.write_packet(&pkt).unwrap();
    }
    muxer.finish().unwrap();
    buf
}

/// Build a minimal PCM WAV of `data_bytes` payload.
fn synth_wav(data_bytes: usize) -> Vec<u8> {
    let sample_rate = 48_000u32;
    let channels = 2u16;
    let bits = 16u16;
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * u32::from(block_align);

    let mut v = Vec::with_capacity(data_bytes + 44);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&((data_bytes as u32) + 36).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    v.resize(v.len() + data_bytes, 0);
    v
}

/// Open a demuxer and drain every packet, returning the byte count read.
fn drain(mut demuxer: Box<dyn Demuxer>) -> usize {
    let mut total = 0;
    while let Some(pkt) = demuxer.read_packet().unwrap() {
        total += pkt.data.len();
    }
    total
}

fn bench_mp4(c: &mut Criterion) {
    let data = synth_mp4(4000, 512); // ~2 MB of samples across 4000 packets
    let mut group = c.benchmark_group("mp4");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("open_and_read_all", |b| {
        b.iter(|| {
            let demuxer = Mp4Demuxer::new(Cursor::new(black_box(data.clone()))).unwrap();
            black_box(drain(Box::new(demuxer)))
        });
    });
    group.finish();
}

fn bench_wav(c: &mut Criterion) {
    let data = synth_wav(4 << 20); // 4 MB
    let mut group = c.benchmark_group("wav");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("open_and_read_all", |b| {
        b.iter(|| {
            let demuxer = WavDemuxer::new(Cursor::new(black_box(data.clone()))).unwrap();
            black_box(drain(Box::new(demuxer)))
        });
    });
    group.finish();
}

criterion_group!(benches, bench_mp4, bench_wav);
criterion_main!(benches);

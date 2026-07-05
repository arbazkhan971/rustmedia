//! Robustness tests: RustMedia's parsers must reject malformed, truncated, and
//! random input with an error — never a panic. A media parser is a hostile
//! surface (it reads untrusted files), so "does not crash" is a hard property.

use std::io::Cursor;

use rustmedia_core::{AudioParameters, Codec, MediaType, Packet, Track, TrackParameters};
use rustmedia_formats::{detect_bytes, open, Demuxer, Mp4Demuxer, Mp4Muxer, Muxer};

/// A tiny deterministic PRNG (xorshift64) so the fuzz inputs are reproducible
/// without pulling in the `rand` crate.
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| (self.next_u64() & 0xFF) as u8).collect()
    }
}

/// Build a small valid MP4 in memory so we can truncate and mutate it.
fn small_mp4() -> Vec<u8> {
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
    for i in 0..20u32 {
        muxer
            .write_packet(&Packet::new(1, vec![i as u8; 256]).with_pts(i64::from(i) * 512))
            .unwrap();
    }
    muxer.finish().unwrap();
    buf
}

#[test]
fn empty_and_tiny_inputs_do_not_panic() {
    for len in 0..64usize {
        let bytes = vec![0u8; len];
        let _ = open(Cursor::new(bytes.clone()));
        let _ = detect_bytes(&bytes);
    }
}

#[test]
fn truncating_a_valid_mp4_never_panics() {
    let full = small_mp4();
    // Every prefix must parse or error cleanly — never crash.
    for len in 0..=full.len() {
        let prefix = full[..len].to_vec();
        let _ = Mp4Demuxer::new(Cursor::new(prefix));
    }
}

#[test]
fn corrupting_bytes_of_a_valid_mp4_never_panics() {
    let base = small_mp4();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..500 {
        let mut bytes = base.clone();
        // Flip a handful of random bytes.
        for _ in 0..8 {
            let idx = (rng.next_u64() as usize) % bytes.len();
            bytes[idx] ^= (rng.next_u64() & 0xFF) as u8;
        }
        let result = std::panic::catch_unwind(|| {
            let Ok(mut d) = Mp4Demuxer::new(Cursor::new(bytes)) else {
                return;
            };
            // Draining should also never panic on a corrupt-but-openable file.
            while let Ok(Some(_)) = d.read_packet() {}
        });
        assert!(result.is_ok(), "parser panicked on corrupted MP4");
    }
}

#[test]
fn random_garbage_is_rejected_cleanly() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_1234);
    for _ in 0..1000 {
        let len = (rng.next_u64() as usize) % 4096;
        let bytes = rng.bytes(len);
        // Detection and open must return without panicking.
        let _ = detect_bytes(&bytes);
        let result = std::panic::catch_unwind(|| {
            let _ = open(Cursor::new(bytes));
        });
        assert!(result.is_ok(), "open() panicked on random input");
    }
}

#[test]
fn garbage_with_valid_magic_is_rejected() {
    // Looks like an MP4 (ftyp) but is otherwise nonsense.
    let mut bytes = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom".to_vec();
    bytes.extend_from_slice(&[0xFF; 200]);
    assert!(open(Cursor::new(bytes)).is_err());

    // RIFF/WAVE header with no fmt/data chunks.
    let wav = b"RIFF\x04\x00\x00\x00WAVE".to_vec();
    assert!(open(Cursor::new(wav)).is_err());
}

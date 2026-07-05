<div align="center">

# 🦀 RustMedia

### Fast, safe, FFmpeg‑free media parsing and processing for Rust.

Inspect, parse, remux, trim, and extract **MP4 · MOV · WAV · MP3** — with a native, zero‑`unsafe` engine and **no C dependencies**.

[![CI](https://github.com/rustmedia/rustmedia/actions/workflows/ci.yml/badge.svg)](https://github.com/rustmedia/rustmedia/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rustmedia.svg)](https://crates.io/crates/rustmedia)
[![Docs.rs](https://img.shields.io/docsrs/rustmedia)](https://docs.rs/rustmedia)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/MSRV-1.75-orange.svg)](#installation)

</div>

---

```console
$ rustmedia inspect trailer.mp4
trailer.mp4
  format    mp4 (video/mp4)
  duration  00:02.000  (2.000 s)
  size      32.0 KiB

  streams (2)
    #1  video   h264      320×240   30 fps   46 kb/s
    #2  audio   aac       44.1 kHz   mono     70 kb/s

  metadata
    title    RustMedia Test
```

RustMedia is the media toolkit the Rust ecosystem was missing: a **single native
engine** that powers a library and a polished CLI, with WebAssembly and language
bindings on the roadmap. It reads and writes real media containers in **safe
Rust** — no `libav`, no `ffmpeg` subprocess, no bundled C.

## Why RustMedia?

- **🚫 No FFmpeg.** Native parsers for every supported format. Nothing to install, nothing to link, no `unsafe`.
- **⚡ Streaming‑first & zero‑copy‑friendly.** Packets are read on demand straight from disk; remux and trim never buffer the whole file just to move it.
- **🔒 Lossless by default.** `remux`, `trim`, and `extract` copy coded packets untouched — no quality loss, no re‑encode, milliseconds not minutes.
- **🧩 Small, composable crates.** Depend on just the type vocabulary, just the readers, or the whole toolkit. `rustmedia-core` has **zero** required dependencies.
- **🦀 Idiomatic API.** `Result`‑based errors with byte offsets, builder‑style packets, timescale‑aware timestamps. It feels like Rust, not a C wrapper.
- **✅ Verified against FFmpeg.** Every parser is tested against `ffprobe` ground truth; every muxer's output is decode‑checked by `ffmpeg`.

## Installation

**CLI** (via Cargo):

```bash
cargo install rustmedia-cli
```

**Library** (add to `Cargo.toml`):

```bash
cargo add rustmedia
```

RustMedia builds on stable Rust **1.75+** and has no system dependencies.

## CLI

```bash
# Inspect a file — tracks, codecs, duration, metadata, chapters
rustmedia inspect movie.mp4
rustmedia inspect movie.mp4 --json          # machine-readable

# Remux to another container without re-encoding (MOV → MP4)
rustmedia remux input.mov --to mp4

# Keyframe-aware, lossless trim
rustmedia trim input.mp4 --from 10s --to 30s

# Pull a single track out into its own file
rustmedia extract input.mp4 --track audio -o sound.m4a
```

Times accept `10s`, `1.5s`, `500ms`, `1:30`, or `00:01:30.250`. Output is
colorized on a TTY and honors `NO_COLOR`.

## Library

```rust
use rustmedia::Media;

fn main() -> rustmedia::Result<()> {
    let mut media = Media::open("movie.mp4")?;

    println!("format:   {}", media.format());
    println!("duration: {:?}", media.duration());

    for track in media.tracks() {
        println!("#{} {} · {}", track.id, track.media_type, track.codec);
        if let Some(v) = track.video() {
            println!("   {}x{} @ {:?} fps", v.width, v.height, v.fps());
        }
    }

    // Stream coded packets — never decoded, ready to remux or analyze.
    while let Some(packet) = media.read_packet()? {
        println!("track {} · {} bytes · keyframe={}",
            packet.track_id, packet.data.len(), packet.is_keyframe);
    }
    Ok(())
}
```

Lossless operations are one call each:

```rust
use rustmedia::{ops, TrimOptions};
use std::time::Duration;

// Remux (copy every track into a new container)
ops::remux("clip.mov", "clip.mp4")?;

// Trim 10s..30s, keyframe-aware
ops::trim("clip.mp4", "cut.mp4", &TrimOptions {
    start: Some(Duration::from_secs(10)),
    end:   Some(Duration::from_secs(30)),
})?;
# Ok::<(), rustmedia::Error>(())
```

## Format support

| Format        | Inspect | Demux | Mux | Notes                                             |
|---------------|:-------:|:-----:|:---:|---------------------------------------------------|
| MP4 / M4A     |   ✅    |  ✅   | ✅  | ISO‑BMFF, non‑fragmented, `co64`, `ctts`, iTunes tags, chapters |
| MOV           |   ✅    |  ✅   | ✅  | QuickTime, shares the ISO‑BMFF engine             |
| WAV           |   ✅    |  ✅   | —   | RIFF PCM/float, `LIST`/`INFO` tags                |
| MP3           |   ✅    |  ✅   | via MP4 | Frame sync, Xing/Info VBR, ID3v2 + ID3v1     |
| Matroska/WebM |   🚧    |  🚧   | 🚧  | On the roadmap                                    |

**Codecs recognized:** H.264, H.265/HEVC, AV1, VP8/VP9, AAC, MP3, Opus, FLAC,
ALAC, AC‑3/E‑AC‑3, PCM variants, and timed text — with codec‑init data (`avcC`,
`esds`, `dOps`, …) captured so remuxing stays lossless.

## Architecture

RustMedia is a Cargo workspace of small, focused crates:

```
rustmedia            ← ergonomic facade: Media::open, ops::{remux,trim,extract}
├── rustmedia-core   ← type vocabulary (Track, Packet, Codec, Timestamp) · zero deps
├── rustmedia-io     ← endian-aware readers, seekable sources · zero third-party deps
└── rustmedia-formats← native parsers + muxers, the Demuxer/Muxer traits
rustmedia-cli        ← the `rustmedia` binary (clap)
```

One engine, many front‑ends. Every parser implements one `Demuxer` trait and
every writer one `Muxer` trait, so the CLI, the facade, and future WASM/Node/
Python bindings all speak the same vocabulary. See [`docs/architecture.md`](docs/architecture.md).

## Design principles

- **Performance is a feature** — but correctness first, then profile, then optimize.
- **Minimal dependencies** — `core` and `io` pull in nothing; the CLI adds only `clap`, `anyhow`, and `serde_json`.
- **Safety** — `#![warn(unsafe_code)]` across the workspace; there is currently **no `unsafe`** in the codebase.
- **Streaming** — the API is built around pulling packets, not loading files.

## Roadmap

- [ ] Matroska / WebM demux + mux (EBML)
- [ ] Fragmented MP4 (`moof`/`traf`) and HLS/DASH segmenting
- [ ] FLAC and Ogg/Opus native containers
- [ ] WebAssembly build (`rustmedia-wasm`) for in‑browser processing
- [ ] Node.js and Python bindings
- [ ] Thumbnails, waveforms, and scene/silence detection
- [ ] Metadata & cover‑art editing in place

See the full [roadmap](docs/roadmap.md).

## Building from source

```bash
git clone https://github.com/rustmedia/rustmedia
cd rustmedia
cargo build --release

# Generate the test corpus (needs ffmpeg) and run everything
cargo xtask gen-fixtures
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Contributing

Contributions are very welcome — new formats, fuzz findings, docs, benchmarks.
Start with [`CONTRIBUTING.md`](CONTRIBUTING.md) and the
[architecture guide](docs/architecture.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution you submit shall be dual‑licensed as above, without
any additional terms.

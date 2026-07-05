# Roadmap

This is where RustMedia is headed. It expands the checklist in the
[README](../README.md) into a phased plan with the motivation behind each item.

Everything here is **aspirational** — a direction, not a promise or a schedule.
Priorities shift with real‑world use and with what contributors are excited to
build. If something below matters to you, that's the best possible reason to
open an issue or a PR; see [`CONTRIBUTING.md`](../CONTRIBUTING.md).

## Where we are

RustMedia today is a native, zero‑`unsafe` engine that can inspect, remux, trim,
and extract **MP4/MOV, WAV, and MP3** — all losslessly, with no FFmpeg and no C
dependencies. The trait‑based core (`Demuxer`/`Muxer`) and the shared type
vocabulary in `rustmedia-core` are designed so that new formats and new
front‑ends slot in without reshaping the engine. That foundation is what the
roadmap builds on.

## Near‑term

The next containers and edits, building directly on the existing engine.

- **Matroska / WebM demux + mux (EBML).** The single most requested format and
  the top priority. Matroska is the lingua franca of modern video, and WebM is
  its web‑native subset. Detection already recognizes the EBML magic and the
  `DocType` split; what's left is the EBML parser and the muxer. Landing this
  also unlocks the Vorbis, SubRip, and ASS codecs already named in the
  vocabulary.
- **Fragmented MP4 (`moof`/`traf`).** Fragmented ISO‑BMFF is how streaming and
  live capture ship video (DASH, CMAF, MSE). Supporting it makes RustMedia a
  first‑class citizen for adaptive‑streaming pipelines and for reading files
  that never had a top‑level `moov`.
- **Metadata & cover‑art editing in place.** Reading tags is done; the natural
  next step is writing them back — retitling, tagging, and swapping cover art
  without a full remux. A common, high‑value operation that needs no decoding.

## Mid‑term

Reach beyond the CLI, and beyond containers we can only copy.

- **FLAC and Ogg/Opus native containers.** RustMedia already recognizes FLAC and
  Opus *inside* MP4; native `.flac` and `.ogg`/`.opus` files are the obvious
  companion. Detection already sniffs `fLaC` and `OggS` — the parsers are what's
  missing.
- **WebAssembly build (`rustmedia-wasm`).** A safe, dependency‑free parser is a
  perfect fit for the browser. A WASM build (the crate is already carved out of
  the default workspace for a wasm target) would let web apps inspect and remux
  media entirely client‑side — no server round‑trip, no FFmpeg.wasm bloat.
- **Thumbnails, waveforms, and scene/silence detection.** Lightweight analysis
  that mostly needs container‑level access, not full decoding: pull keyframes
  for thumbnails, summarize PCM into waveforms, and flag scene/silence
  boundaries for editors and search.

## Long‑term

The ambitious surface — bindings and, carefully, encoding.

- **Node.js and Python bindings.** The same engine, exposed to the ecosystems
  that do the most media scripting. Because everything speaks the
  `Demuxer`/`Muxer` vocabulary, bindings are a front‑end problem, not a rewrite.
- **HLS/DASH segmenting.** Once fragmented MP4 lands, packaging media into HLS
  and DASH segments turns RustMedia into a lossless origin‑packager — segment
  and manifest without ever re‑encoding.
- **Transcoding via optional codec plugins.** RustMedia is lossless by design and
  will stay that way at its core. Actual encode/decode, if it ever arrives, will
  be **opt‑in** behind clearly‑scoped, feature‑gated codec plugins — so the
  default build stays small, safe, and FFmpeg‑free.

## Guiding constraints

Whatever gets built, these hold:

- **Safe Rust, no `unsafe`** — the workspace warns on `unsafe_code` and the tree
  has none.
- **Minimal dependencies** — `core` and `io` stay dependency‑free.
- **Lossless by default** — copy first; any re‑encoding is always opt‑in.
- **Verified against FFmpeg** — new parsers and muxers ship with ffprobe/ffmpeg
  ground‑truth tests, and fuzzing is on the way (see [`SECURITY.md`](../SECURITY.md)).

Want to move an item up this list? Contributions are genuinely welcome — pick a
line above, open an issue to say you're on it, and read
[`CONTRIBUTING.md`](../CONTRIBUTING.md) to get started.

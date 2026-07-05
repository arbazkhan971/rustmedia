# Changelog

All notable changes to RustMedia are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
adhere to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Workspace foundation** — `rustmedia-core` (zero-dependency type vocabulary:
  `Error`, `Rational`, `Timestamp`, `Codec`, `Track`, `Packet`, `Metadata`,
  `ContainerFormat`) and `rustmedia-io` (endian-aware `ReadBytes`, seekable
  `Source`).
- **MP4 / MOV demuxer** — native ISO-BMFF parser: `moov` tree, full `stbl`
  sample-table flattening (`stts`/`ctts`/`stsc`/`stsz`/`stz2`/`stco`/`co64`/
  `stss`), codec detection with `avcC`/`esds`/`dOps`/… extradata, iTunes `ilst`
  metadata, Nero chapters, keyframe-aware seeking.
- **Matroska / WebM demuxer** — EBML parser with `DocType` detection, `Info`/
  `Tracks` parsing, and cluster streaming with Xiph/EBML/fixed lacing.
- **WAV demuxer** — RIFF PCM/float with `LIST`/`INFO` metadata.
- **MP3 demuxer** — MPEG-1/2/2.5 Layer III frame sync, Xing/Info VBR duration,
  ID3v2 and ID3v1 metadata.
- **MP4 muxer** — faststart (`moov`-first) writer with real sample tables and
  codec sample entries; `OpusHead` → `dOps` conversion for cross-container Opus.
- **Lossless operations** — `remux`, keyframe-aware `trim`, and `extract`, all
  copy-only (no re-encode).
- **CLI** — `rustmedia inspect | remux | trim | extract`, with colorized human
  output (`NO_COLOR`-aware) and `--json`.
- **Library facade** — `Media::open`, packet streaming, and `ops::*`.
- **Quality** — Criterion benchmarks, a robustness/fuzz suite (never panics on
  hostile input), GitHub Actions CI (fmt, clippy `-D warnings`, cross-platform
  tests, MSRV, rustdoc, cargo-deny) and release automation.

[Unreleased]: https://github.com/rustmedia/rustmedia/commits/main

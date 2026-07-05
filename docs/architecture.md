# Architecture

How RustMedia is put together: one native engine, a shared type vocabulary, and
a pair of traits (`Demuxer` / `Muxer`) that every format and every front-end
speaks through. No FFmpeg, no C, no `unsafe`.

## One engine, many front-ends

RustMedia is a single parsing/writing engine wearing several hats. The library
facade, the CLI, and the future WASM/Node/Python bindings on the roadmap are all
thin shells around the same core. They never re-implement format logic — they
call into it.

Two traits make that possible:

- **`Demuxer`** — every container parser (MP4, WAV, MP3, …) implements it, so a
  reader is a `Box<dyn Demuxer>` regardless of the format underneath.
- **`Muxer`** — every container writer implements it, so an output sink is a
  `Box<dyn Muxer>` regardless of the target format.

Because both sides trade in the same neutral vocabulary from `rustmedia-core`
(`Track`, `Packet`, `Timestamp`, `Codec`, …), a packet read from one container
can be handed straight to a muxer for another. That is exactly what `remux`,
`trim`, and `extract` do — and they never decode a single sample.

## The crate graph

RustMedia is a Cargo workspace of small, focused crates:

```
rustmedia            ← ergonomic facade: Media::open, ops::{remux,trim,extract}
├── rustmedia-core   ← type vocabulary (Track, Packet, Codec, Timestamp) · zero deps
├── rustmedia-io     ← endian-aware readers, seekable sources · zero third-party deps
└── rustmedia-formats← native parsers + muxers, the Demuxer/Muxer traits
rustmedia-cli        ← the `rustmedia` binary (clap)
```

Dependencies point strictly downward:

| Crate               | Depends on                        | Third-party deps                 |
|---------------------|-----------------------------------|----------------------------------|
| `rustmedia-core`    | *(nothing)*                       | **zero** (optional `serde`)      |
| `rustmedia-io`      | `rustmedia-core`                  | **zero**                         |
| `rustmedia-formats` | `rustmedia-core`, `rustmedia-io`  | native parsers only              |
| `rustmedia`         | `core`, `io`, `formats`           | re-exports the above             |
| `rustmedia-cli`     | `rustmedia`                       | `clap`, `anyhow`, `serde_json`   |

- **`rustmedia-core`** is the shared type vocabulary — errors, timestamps,
  codecs, tracks, packets, metadata, format identifiers. It has **zero mandatory
  dependencies**, so it is cheap to depend on; the optional `serde` feature
  derives `Serialize`/`Deserialize` on the public types.
- **`rustmedia-io`** is the byte-level layer: the `ReadBytes` extension trait for
  endian-aware reads and the `Source` trait for seekable inputs. It depends only
  on `core` (for the error type) and adds no third-party crates — rather than
  pull in `byteorder`, it ships its own small `read_u32_be`/`read_fourcc`/… set.
- **`rustmedia-formats`** holds the native container parsers and muxers plus the
  `Demuxer`/`Muxer` traits and the `detect`/`open` entry points.
- **`rustmedia`** is the front door: it re-exports the whole `core` vocabulary
  and adds the `Media` facade and the `ops` module.
- **`rustmedia-cli`** is the `rustmedia` binary, and the only crate that reaches
  for `clap`, `anyhow`, and `serde_json`.

## The `Demuxer` contract

A demuxer reads a container and yields its tracks, metadata, and packets. The
trait is object-safe so any parser can be held as `Box<dyn Demuxer>`:

```rust
pub trait Demuxer {
    fn format(&self) -> ContainerFormat;
    fn tracks(&self) -> &[Track];
    fn metadata(&self) -> &Metadata;
    fn duration(&self) -> Option<Duration>;
    fn read_packet(&mut self) -> Result<Option<Packet>>;
    fn seek(&mut self, target: Duration) -> Result<()>;   // default: Unsupported
}
```

- `duration()` reports the overall media duration — the longest track's.
- `read_packet()` yields packets **in file order** (interleaved across tracks),
  returning `Ok(None)` at end of stream. File order is both the cheapest order to
  read and the order a muxer wants them back.
- `seek(target)` positions the demuxer so subsequent reads begin at or before
  `target`, aligned to a keyframe for video. The default implementation returns
  `Error::Unsupported`; seekable formats (like MP4) override it.

## The `Muxer` contract

A muxer writes tracks and packets into a container. Its lifecycle is strict:
`start` once, then `write_packet` per packet, then `finish`:

```rust
pub trait Muxer {
    fn start(&mut self, tracks: &[Track]) -> Result<()>;
    fn set_metadata(&mut self, metadata: &Metadata) -> Result<()>;  // default: no-op
    fn write_packet(&mut self, packet: &Packet) -> Result<()>;
    fn finish(&mut self) -> Result<()>;
}
```

Packets are copied through **untouched** — a muxer never re-encodes — so a
track's `codec_private` init data must describe the same coded bitstream the
packets carry. `write_packet` matches a packet to a track by `Packet::track_id`.

## The data model

Everything flows through a handful of plain types in `rustmedia-core`.

### `Track` — a stream description, no samples

A `Track` describes one elementary stream; it holds no data. Pull `Packet`s for a
given `Track::id` from a demuxer to get the bytes.

```rust
pub struct Track {
    pub id: u32,
    pub codec: Codec,
    pub media_type: MediaType,
    pub timescale: u32,
    pub duration: Option<Timestamp>,
    pub language: Option<String>,
    pub name: Option<String>,
    pub bitrate: Option<u64>,
    pub codec_private: Option<Vec<u8>>,   // avcC / esds / dOps / … — needed to remux losslessly
    pub parameters: TrackParameters,
}
```

`TrackParameters` is an enum — `Video(VideoParameters)`, `Audio(AudioParameters)`,
`Subtitle(SubtitleParameters)`, or `None` — carrying category-specific fields
(video: `width`, `height`, `frame_rate`, `display_aspect_ratio`, `bit_depth`;
audio: `sample_rate`, `channels`, `bits_per_sample`).

### `Packet` — one coded, undecoded unit

```rust
pub struct Packet {
    pub track_id: u32,
    pub dts: Option<i64>,
    pub pts: Option<i64>,
    pub duration: Option<u64>,
    pub is_keyframe: bool,
    pub data: Vec<u8>,
}
```

Timestamps are in the owning track's timescale. A `Packet` is deliberately just
data plus timing — RustMedia moves packets between containers without ever
decoding them. Builder-style setters (`with_pts`, `with_dts`, `with_duration`,
`keyframe`) make hand-constructing packets ergonomic.

### `Timestamp` — integer ticks, not floats

This is the design decision that keeps A/V sync exact. Media containers count
time in **ticks** of a per-track **timescale** (ticks per second):

```rust
pub struct Timestamp {
    pub ticks: i64,
    pub timescale: u32,   // ticks per second
}
```

A 30 fps frame at the two-second mark in a 15360-tick timescale is
`Timestamp { ticks: 30720, timescale: 15360 }`.

Why integers? Because eagerly converting every timestamp to floating-point
seconds accumulates **rounding drift** — a fraction of a millisecond per sample,
thousands of samples, and suddenly audio and video have walked apart. RustMedia
preserves the container's original integers and only converts to `Duration` or
`f64` at the edges, when a human needs to read them. `Timestamp::rescale` moves a
value to a new timescale using `i128` intermediate math with round-to-nearest, so
copying samples between time bases never overflows and never silently loses a
tick. `Rational` (used for frame rates and aspect ratios) is the same principle
applied to ratios: exact `num/den`, reduced by GCD, never flattened to a float
until asked.

### `Metadata`, `Codec`, `ContainerFormat`

`Metadata` is a `BTreeMap` of normalised, lowercase string tags (iTunes atoms,
ID3 frames, and RIFF `INFO` keys are all mapped onto the same `keys::*`
vocabulary) plus a `Vec<Chapter>`. The `BTreeMap` makes iteration deterministic
(alphabetical), which keeps CLI and `--json` output stable. `Codec` and
`MediaType` name what a track carries; recognising a codec by name does **not**
imply RustMedia can decode it — the toolkit inspects and remuxes coded data, it
does not transcode. `ContainerFormat` names the wrapper (MP4, MOV, WAV, MP3, …).

## Reading a file, end to end

```rust
let file = std::fs::File::open("movie.mp4")?;
let demuxer = rustmedia_formats::open(file)?;   // detect + construct the right parser
for track in demuxer.tracks() { /* … */ }
```

`open` calls `detect` to sniff the magic bytes, then constructs the matching
demuxer boxed behind the trait:

```rust
match detect(&mut reader)? {
    Some(Mp4 | Mov) => Ok(Box::new(Mp4Demuxer::new(reader)?)),
    Some(Wav)       => Ok(Box::new(WavDemuxer::new(reader)?)),
    Some(Mp3)       => Ok(Box::new(Mp3Demuxer::new(reader)?)),
    Some(other)     => Err(Error::unsupported(/* not yet implemented */)),
    None            => Err(Error::UnknownFormat),
}
```

The `Media` facade wraps this and adds conveniences (`video_tracks`,
`best_audio`, a `packets()` iterator, `size_bytes`, …), but the mechanics are the
same.

## Inside the MP4 engine: flattening the sample table

MP4/MOV is where most of the parsing complexity lives. `Mp4Demuxer::new` walks the
top-level boxes, loads the `moov` payload, and parses the movie tree:

```
moov
├── mvhd                     movie timescale + duration
├── trak
│   ├── tkhd                 track id, dimensions
│   └── mdia
│       ├── mdhd             track timescale, duration, language
│       ├── hdlr             handler → media type (vide/soun/text…)
│       └── minf/stbl        the sample table
└── udta / meta              iTunes (ilst) tags, Nero (chpl) chapters
```

The sample table (`stbl`) is not a list of samples — it is a set of **parallel
tables** that must be joined:

| Box            | Carries                                              |
|----------------|------------------------------------------------------|
| `stsz` / `stz2`| per-sample byte sizes (or one uniform size)          |
| `stco` / `co64`| chunk byte offsets (32- or 64-bit)                   |
| `stsc`         | sample-to-chunk map (`first_chunk`, `samples_per_chunk`) |
| `stts`         | decode durations, run-length `(count, delta)`        |
| `ctts`         | composition offsets (pts − dts), run-length          |
| `stss`         | sync-sample (keyframe) numbers                       |

`parse_sample_table` reads all of them and **flattens** them into one `Sample`
per media sample — `{ offset, size, dts, pts, duration, is_sync }` — in four
passes:

1. **Placement.** Walk chunks; `stsc` says how many samples each chunk holds (the
   final run extends to the last chunk). The first sample sits at the chunk
   offset; each subsequent offset is the previous plus its size.
2. **Timing.** Expand the `stts` runs, accumulating `dts` and recording each
   sample's `duration`.
3. **Composition.** Apply `ctts` offsets to get `pts = dts + offset`; with no
   `ctts`, `pts == dts`.
4. **Sync.** With no `stss`, **every** sample is a keyframe (typical for audio);
   otherwise the 1-based numbers in `stss` mark the sync samples.

The demuxer then builds a global `order` list of `(track, sample)` pairs sorted by
file offset. `read_packet` walks that list, seeks to `sample.offset`, and reads
`sample.size` bytes straight from `mdat` — packets on demand, nothing buffered.
`seek` picks a reference track (first video, else track 0), finds the last sync
sample whose `dts <= target`, and resumes reading from the first packet at or
after that keyframe's offset.

## Inside the MP4 muxer: the two-pass offset strategy

`Mp4Muxer` writes a **faststart** file — `moov` before `mdat` — so the result is
streaming-friendly. That ordering creates a chicken-and-egg problem: the chunk
offsets recorded in `stco`/`co64` are **absolute file positions**, but the sample
bytes live *after* `moov`, so their offsets depend on how big `moov` is — which
depends on the tables that contain those very offsets.

RustMedia breaks the cycle by exploiting one fact: the *size* of `moov` does not
depend on the *values* of the offsets (they are fixed-width `u32`/`u64` fields).
So `finish` builds `moov` twice:

```rust
let measure = build_moov(&tracks, /* data_start */ 0, use_co64);   // pass 1: learn the size
let data_start = (ftyp.len() + measure.len() + mdat_header_len) as u64;
let moov = build_moov(&tracks, data_start, use_co64);              // pass 2: real offsets
```

Then it writes `ftyp`, `moov`, the `mdat` header, and the buffered `mdat` payload.
Supporting details:

- **`co64` fallback.** If the media payload would overflow 32-bit offsets
  (`> u32::MAX − 64 MiB`), the muxer switches to 64-bit `co64` chunk offsets and a
  16-byte large-size `mdat` header.
- **Codec config.** Video sample entries get `avcC`/`hvcC`/`av1C`/`vpcC` from the
  track's `codec_private`; audio gets an `esds` for AAC (object type `0x40`) and
  MP3 (`0x6B`), `dOps` for Opus, `dfLa` for FLAC.
- **Table shape.** `stts`/`ctts` are run-length encoded, `stsc` maps one sample
  per chunk (preserving interleaving), `stss` is omitted when every sample is a
  sync sample, and `ctts` is written only when some `pts != dts`.
- **Guardrail.** `start` validates up front that every track's codec has a known
  sample-entry fourcc, so an unmuxable codec fails fast rather than mid-write.

## Format detection

`detect` reads the first 16 bytes, restores the stream position (so the same
source can be handed straight to the parser), and matches magic numbers:

| Magic                         | Format                                   |
|-------------------------------|------------------------------------------|
| `1A 45 DF A3`                 | Matroska / WebM (same magic; refined later) |
| `RIFF … WAVE`                 | WAV                                      |
| `fLaC`                        | FLAC                                    |
| `OggS`                        | Ogg                                     |
| box `ftyp`/`moov`/`mdat`/… @4 | MP4 — or MOV if the `ftyp` brand is `qt  ` |
| `ID3` or `FF Ex` frame sync   | MP3                                     |

`detect_bytes` is the pure, allocation-free core, handy for tests and for callers
that already hold the leading bytes. Detection names the *container*; it does not
guarantee a parser exists — `open` turns a recognised-but-unimplemented format
into `Error::Unsupported`.

## Extension points

Adding a new container is a matter of implementing the traits and registering
them — no changes to the type vocabulary:

1. **Implement `Demuxer`** for the new parser (and `Muxer` if you want to write
   the format too).
2. **Teach `detect_bytes`** the format's magic bytes so it returns the right
   `ContainerFormat`.
3. **Add a match arm in `open`** mapping that `ContainerFormat` to your demuxer.

To make a format a valid `remux`/`extract` *target*, register its extension in
`ops::muxer_for` and add its codec fourccs to the muxer. Matroska/WebM (EBML) is
the next format on the roadmap and slots into exactly these three seams.

## Safety and verification

- `#![warn(unsafe_code)]` across the workspace; there is currently **no `unsafe`**
  anywhere in the codebase. Every byte is parsed in safe Rust.
- Errors carry the two things that help when a file refuses to open — *which
  parser* rejected it and *at what byte offset* (`Error::Malformed { format,
  offset, message }`) — and a truncated read surfaces as `Error::UnexpectedEof`
  labelled with what was being read.
- Every parser is tested against `ffprobe` ground truth, and every muxer's output
  is decode-checked by `ffmpeg`.

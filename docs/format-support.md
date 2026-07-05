# Format support

This is the honest, detailed capability matrix for RustMedia — what each
container parser reads and writes today, the exact codecs each is recognized in,
and the limitations you should know about before relying on it.

RustMedia **inspects and remuxes coded data; it does not decode or transcode.**
Recognizing a codec means RustMedia can name it, carry its init data, and copy
its packets losslessly — not that it can turn it into pixels or samples.

Legend: ✅ supported · — not applicable · 🚧 planned (see the
[roadmap](roadmap.md)).

## Containers at a glance

| Container       | Inspect | Demux | Mux | Seek | Detected by |
|-----------------|:-------:|:-----:|:---:|:----:|-------------|
| MP4 / M4A / M4V |   ✅    |  ✅   | ✅  | ✅   | `ftyp`/`moov`/`mdat`/… box |
| MOV (QuickTime) |   ✅    |  ✅   | ✅  | ✅   | `ftyp` major brand `qt  ` |
| WAV             |   ✅    |  ✅   | —   | ✅   | `RIFF`…`WAVE` |
| MP3             |   ✅    |  ✅   | via MP4 | ~ | `ID3` tag or MPEG frame sync |
| Matroska / MKV  |   🚧    |  🚧   | 🚧  | 🚧  | EBML `1A 45 DF A3` |
| WebM            |   🚧    |  🚧   | 🚧  | 🚧  | EBML + `DocType webm` |
| FLAC (native)   |   🚧    |  🚧   | 🚧  | 🚧  | `fLaC` |
| Ogg             |   🚧    |  🚧   | 🚧  | 🚧  | `OggS` |

**Detection vs. parsing.** RustMedia's byte sniffer already recognizes the magic
of Matroska/WebM, FLAC, and Ogg, but no demuxer is wired up for them yet, so
`open()` returns an `Unsupported` error (not `UnknownFormat`) for those files.
That's deliberate: the vocabulary is in place ahead of the parsers.

---

## MP4 / MOV (ISO‑BMFF)

MP4 and MOV share one engine — MOV is distinguished only by its QuickTime major
brand (`qt  `). This is the most complete parser in the toolkit.

**Parsed on demux**

- **Tracks:** video, audio, subtitle, and opaque data tracks, with per‑track
  codec, timescale, and duration.
- **Codec parameters:** width/height for video; sample rate, channel count, and
  bit depth for audio; from the `stsd` sample entries.
- **Codec init data (extradata):** `avcC`, `hvcC`, `av1C`, `vpcC`, `esds`
  (AAC/MP3), `dOps` (Opus), `dfLa` (FLAC), and `alac` — captured into
  `codec_private` so remuxing stays byte‑exact.
- **Sample tables:** `stco`/`co64` (32‑ and 64‑bit chunk offsets), `stsc`,
  `stsz`, `stts`, `stss` (keyframes), and `ctts` (composition offsets / B‑frames).
- **Metadata:** iTunes‑style `ilst`/`meta` tags.
- **Chapters.**
- **Seeking:** keyframe‑aware, using the sync‑sample table.

**Written on mux**

The MP4 muxer writes non‑fragmented `moov`/`mdat` files with sample tables,
composition offsets, and the matching codec config boxes.

**Known limitations**

- **Fragmented MP4 is not supported.** `moof`/`traf` movie‑fragment streams
  (DASH/CMAF, live captures) are not yet parsed — top of the roadmap.
- The muxer writes a *subset* of codecs (see the codec table below); tracks with
  a codec the MP4 muxer can't describe are rejected rather than written wrong.
- Only the first sample entry per track is currently interpreted.
- Edit lists (`elst`) are read conservatively; unusual timelines may not be
  reproduced exactly.

## WAV (RIFF)

**Parsed on demux**

- **Tracks:** a single PCM/float audio track.
- **Format:** derived from the `fmt ` chunk — bit depth maps to `pcm_u8` (8),
  `pcm_s16le` (16), `pcm_s24le` (24); IEEE‑float format maps to `pcm_f32le`.
- **Metadata:** `LIST`/`INFO` tags.
- **Seeking:** exact (constant‑rate PCM).

**Recognized but not decoded:** WAV format tags for A‑law (`0x0006`), µ‑law
(`0x0007`), MP3 (`0x0055`), and AC‑3 (`0x2000`) are named; A‑law/µ‑law surface
as `Codec::Other`.

**Known limitations**

- **No WAV muxer** — WAV is read‑only today.
- Extensible (`WAVE_FORMAT_EXTENSIBLE`) channel masks are not surfaced.
- Files larger than 4 GiB (RF64/BW64) are not supported.

## MP3 (elementary stream)

**Parsed on demux**

- **Frame sync** across MPEG‑1/2 Audio Layer III frames.
- **VBR headers:** Xing/Info (and VBRI) are read for accurate duration.
- **Metadata:** ID3v2 (front) and ID3v1 (trailing) tags.

**Muxing:** MP3 has no native muxer, but an MP3 track can be **muxed into MP4**
(written as an `mp4a` entry with an MP3 object type).

**Known limitations**

- **Seeking in VBR files is approximate** unless a seek table is present —
  without a Xing TOC, position is estimated from the average bitrate.
- Free‑format and MPEG‑2.5 streams are best‑effort.

---

## Codec recognition matrix

Which codecs are recognized in which containers **today**. "Demux" = read and
carried losslessly; "Mux" = can be written into that container.

| Codec | Type | MP4/MOV | WAV | MP3 |
|-------|------|:-------:|:---:|:---:|
| H.264 / AVC        | video | demux + mux | — | — |
| H.265 / HEVC       | video | demux + mux | — | — |
| AV1                | video | demux + mux | — | — |
| VP8                | video | demux + mux | — | — |
| VP9                | video | demux + mux | — | — |
| MPEG‑4 Part 2      | video | demux | — | — |
| ProRes             | video | demux | — | — |
| AAC                | audio | demux + mux | — | — |
| MP3                | audio | demux + mux | recognized¹ | demux |
| Opus               | audio | demux + mux | — | — |
| FLAC               | audio | demux + mux | — | — |
| ALAC               | audio | demux | — | — |
| AC‑3               | audio | demux | recognized¹ | — |
| E‑AC‑3             | audio | demux | — | — |
| PCM s16le          | audio | demux + mux | demux | — |
| PCM s16be          | audio | demux + mux | — | — |
| PCM s24le          | audio | demux | demux | — |
| PCM f32le          | audio | demux | demux | — |
| PCM u8             | audio | demux | demux | — |
| `mov_text` (tx3g)  | text  | demux + mux | — | — |
| WebVTT             | text  | demux | — | — |

¹ Named from the WAV `fmt ` format tag; RustMedia reads the WAV container's
metadata but does not extract these embedded streams as separate outputs.

**Codecs in the vocabulary, not yet produced by any parser.** The `Codec` enum
also models **Vorbis**, **SubRip (SRT)**, and **ASS/SSA**. These are reserved
names with no current recognition path — they'll start appearing once the
Ogg and Matroska demuxers land, since those are the containers that carry them.
The enum is `#[non_exhaustive]`, so more codecs can be added without a breaking
change.

**Anything unrecognized** surfaces as `Codec::Other("<id>")` (carrying the raw
fourcc or container codec ID) or `Codec::Unknown`, and the packets are still
carried through — RustMedia never silently drops a track it doesn't recognize.

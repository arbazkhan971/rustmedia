# CLI reference

The `rustmedia` binary is a fast, safe, FFmpeg-free media toolkit. It inspects a
file's structure and moves media between containers — all without decoding a
single sample, so `remux`, `trim`, and `extract` are lossless and finish in
milliseconds.

```
rustmedia <COMMAND> [OPTIONS]

Commands:
  inspect   Inspect a media file: format, tracks, duration, metadata, chapters
  remux     Remux to another container without re-encoding (e.g. MOV → MP4)
  trim      Trim to a time range, keyframe-aware and lossless
  extract   Extract selected tracks into a new file without re-encoding
```

Install it with `cargo install rustmedia-cli`. Run `rustmedia --help` for the
command list or `rustmedia <command> --help` for a command's options;
`rustmedia --version` prints the version.

## `inspect`

Show a file's format, tracks, duration, metadata, and chapters.

**Synopsis**

```
rustmedia inspect <FILE> [--json]
```

**Options**

| Option   | Description                                                    |
|----------|----------------------------------------------------------------|
| `<FILE>` | Path to the media file to inspect (required).                  |
| `--json` | Emit machine-readable JSON instead of the human-readable summary. |

**Example**

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

Each stream line is `#id  type  codec  details`, where the details vary by kind:
video shows `WIDTH×HEIGHT`, frame rate, and (when not 8) bit depth; audio shows
sample rate and a channel name (`mono`, `stereo`, `5.1`, `7.1`, or `N ch`). A
track's language and bitrate are appended when known. Chapters, when present,
print under a `chapters (N)` heading with their start time and title.

## `remux`

Copy every track into a different container without re-encoding.

**Synopsis**

```
rustmedia remux <INPUT> [--to <EXT>] [-o <OUTPUT>]
```

**Options**

| Option              | Description                                                       |
|---------------------|-------------------------------------------------------------------|
| `<INPUT>`           | Input media file (required).                                      |
| `--to <EXT>`        | Target container / extension. Default: `mp4`.                     |
| `-o`, `--output`    | Output file. Defaults to the input name with the new extension.   |

Output must be an **MP4-family** container — `.mp4`, `.m4a`, `.m4v`, `.mov`, or
`.m4b`. Any other target extension is rejected.

**Example**

```console
$ rustmedia remux clip.mov --to mp4
remuxed clip.mov → clip.mp4 (2 tracks, 312 packets, 4.1 MiB)
```

## `trim`

Cut a file down to a time range, keyframe-aware and lossless. The demuxer seeks
to the keyframe at or before `--from` so the result stays decodable, and
timestamps are rebased so the output begins at zero.

**Synopsis**

```
rustmedia trim <INPUT> [--from <TIME>] [--to <TIME>] [-o <OUTPUT>]
```

**Options**

| Option              | Description                                                    |
|---------------------|----------------------------------------------------------------|
| `<INPUT>`           | Input media file (required).                                   |
| `--from <TIME>`     | Start of the kept range. Omit to start at the beginning.       |
| `--to <TIME>`       | End of the kept range. Omit to run to the end of the file.     |
| `--copy`            | Copy without re-encoding. On by default and the only mode; accepted for clarity. |
| `-o`, `--output`    | Output file. Defaults to `<name>_trimmed.<ext>`.               |

`--to` must be after `--from`, or the command errors. Times use the syntax below.
The output is an MP4-family container (the trim copies coded packets through a
native MP4 muxer).

**Example**

```console
$ rustmedia trim input.mp4 --from 10s --to 30s
trimmed input.mp4 → input_trimmed.mp4 (2 tracks, 601 packets, 1.2 MiB)
```

## `extract`

Pull selected tracks out into their own file, without re-encoding.

**Synopsis**

```
rustmedia extract <INPUT> --track <SELECTOR> [-o <OUTPUT>]
```

**Options**

| Option              | Description                                                        |
|---------------------|--------------------------------------------------------------------|
| `<INPUT>`           | Input media file (required).                                       |
| `--track <SELECTOR>`| Which track(s) to keep (required). See below.                      |
| `-o`, `--output`    | Output file. Defaults to `<name>_<selector>.m4a`.                  |

The selector is one of:

- `video` (or `v`) — all video tracks
- `audio` (or `a`) — all audio tracks
- `subtitle` (or `sub`, `s`) — all subtitle tracks
- a numeric **track id** — the single track with that id

If the selector matches no track, the command errors.

**Example**

```console
$ rustmedia extract movie.mp4 --track audio -o sound.m4a
extracted movie.mp4 → sound.m4a (1 tracks, 431 packets, 703.2 KiB)
```

## Time-format syntax

`--from` and `--to` (and library `parse_duration`) accept any of these forms:

| Form            | Example         | Meaning                        |
|-----------------|-----------------|--------------------------------|
| plain seconds   | `90`, `1.5`     | seconds (integer or fractional)|
| unit-suffixed   | `90s`, `1.5s`   | seconds                        |
|                 | `500ms`         | milliseconds                   |
|                 | `2m`            | minutes                        |
|                 | `1h`            | hours                          |
| clock `mm:ss`   | `1:30`          | 90 seconds                     |
| clock `hh:mm:ss`| `01:02:03`      | 3723 seconds                   |
| with fraction   | `00:01:30.250`  | 90.25 seconds                  |

Values must be non-negative and finite; empty or unparseable strings are
rejected with an `invalid argument` error.

## `--json` output

`inspect --json` prints a stable, pretty-printed JSON document — ideal for
scripting. The shape is:

```json
{
  "path": "trailer.mp4",
  "format": "mp4",
  "mime_type": "video/mp4",
  "duration_secs": 2.0,
  "size_bytes": 32768,
  "streams": [
    {
      "id": 1,
      "type": "video",
      "codec": "h264",
      "timescale": 15360,
      "duration_secs": 2.0,
      "language": null,
      "bitrate": 46000,
      "width": 320,
      "height": 240,
      "fps": 30.0
    },
    {
      "id": 2,
      "type": "audio",
      "codec": "aac",
      "timescale": 44100,
      "duration_secs": 2.0,
      "language": null,
      "bitrate": 70000,
      "sample_rate": 44100,
      "channels": 1
    }
  ],
  "metadata": { "title": "RustMedia Test" },
  "chapters": []
}
```

Video streams add `width`/`height`/`fps`; audio streams add
`sample_rate`/`channels`. Metadata keys are normalised and emitted in
alphabetical order, so diffs and downstream parsing stay deterministic.

## Colour, `NO_COLOR`, and exit codes

- **Colour** is opt-in and automatic: `inspect` colourises its output only when
  stdout is a real terminal. Piping or redirecting turns colour off, so captured
  output is always clean.
- **`NO_COLOR`** — setting the `NO_COLOR` environment variable (to any value)
  disables colour even on a TTY, per the informal convention.
- **Exit codes** — commands exit `0` on success. On failure they print
  `error: <message>` (the full cause chain) to **stderr** and exit `1`.

## Recipes

**Extract the audio track to an `.m4a`**

```bash
rustmedia extract podcast.mp4 --track audio -o podcast.m4a
```

**Trim a clip without re-encoding**

```bash
rustmedia trim lecture.mp4 --from 1:30 --to 4:15 -o highlight.mp4
```

**Remux QuickTime to MP4**

```bash
rustmedia remux screen-recording.mov --to mp4
```

**Pull a single track out by id**

```bash
# Find the id with `inspect`, then extract it.
rustmedia extract movie.mp4 --track 1 -o track1.m4v
```

**Get JSON for scripting with `jq`**

```bash
# Duration in seconds
rustmedia inspect movie.mp4 --json | jq '.duration_secs'

# Codec + resolution of the video stream
rustmedia inspect movie.mp4 --json \
  | jq '.streams[] | select(.type == "video") | {codec, width, height, fps}'

# Every codec in the file
rustmedia inspect movie.mp4 --json | jq -r '.streams[].codec'
```

**Force plain, colour-free output in a pipeline**

```bash
NO_COLOR=1 rustmedia inspect movie.mp4
```

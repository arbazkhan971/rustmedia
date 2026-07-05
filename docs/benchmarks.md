# Benchmarks

RustMedia ships a [Criterion](https://github.com/bheisler/criterion.rs)
benchmark suite that measures raw demux throughput — opening a container and
draining every packet.

```bash
cargo bench -p rustmedia-formats --bench demux
```

The benchmarks build their inputs **in memory** (no fixtures, no ffmpeg), so
they are deterministic and run identically in CI and on your laptop.

## Results

Measured on a cloud x86-64 Linux box (single thread, `--release`, `lto = "thin"`).
Your numbers will differ; treat these as ballpark, not gospel.

| Benchmark                | Input                         | Throughput   |
|--------------------------|-------------------------------|--------------|
| `mp4/open_and_read_all`  | ~2 MB MP4, 4000 PCM packets   | **~2.6 GiB/s** |
| `wav/open_and_read_all`  | 4 MB PCM WAV                  | **~5.4 GiB/s** |

"Throughput" is total input bytes ÷ wall-clock time to parse the container and
hand back every packet. It includes flattening the MP4 sample table
(`stts`/`stsc`/`stsz`/`stco` → per-sample records) and copying packet payloads.

## What this does and doesn't tell you

- ✅ It shows the parser is not the bottleneck: multi-GiB/s means a typical file
  is parsed in well under a millisecond of CPU, so real workloads are dominated
  by I/O, not RustMedia.
- ✅ It is reproducible and regression-guarding — run it before and after a change
  to catch accidental slowdowns.
- ⚠️ It is **not** a head-to-head with FFmpeg. A fair comparison (same file, same
  operation, cold cache) is on the [roadmap](roadmap.md); until it exists, we
  won't publish "N× faster than FFmpeg" claims we haven't earned.
- ⚠️ It uses PCM packets so the measurement is parsing overhead, not codec work
  (RustMedia never decodes, so there is no codec work to measure).

## Methodology notes

- One synthetic MP4 is produced by RustMedia's own muxer, then re-parsed — so the
  benchmark also exercises the write path each iteration's setup.
- `Throughput::Bytes` is set to the input size so Criterion reports GiB/s
  directly.
- Inputs are cloned inside the timed closure via `black_box` to prevent the
  optimizer from hoisting the parse out of the loop.

Contributions of more realistic corpora (real H.264/AAC files, large VBR MP3s,
pathological box layouts) are very welcome — see [CONTRIBUTING.md](../CONTRIBUTING.md).

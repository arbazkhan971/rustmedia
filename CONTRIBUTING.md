# Contributing to RustMedia

Thanks for taking the time to contribute. RustMedia is a native, FFmpeg‑free
media toolkit written in safe Rust, and it grows fastest when people bring new
formats, fuzz findings, docs, and benchmarks. This guide gets you from a clean
clone to a green PR.

Every contribution — a one‑line typo fix or a whole new container parser — is
welcome. If anything here is unclear, that's a bug in this document; please open
an issue.

## Getting set up

You need:

- **Rust 1.75 or newer** (the project MSRV). Install via [rustup](https://rustup.rs/).
  A `rust-toolchain.toml` pins the `stable` channel with `rustfmt` and `clippy`,
  so those components are installed automatically.
- **FFmpeg** — only for generating the test fixtures. The parsers themselves
  have **no** system dependencies; FFmpeg is used as ground truth for tests and
  to synthesize sample files. Install it from your package manager
  (`apt install ffmpeg`, `brew install ffmpeg`, …).

```bash
git clone https://github.com/rustmedia/rustmedia
cd rustmedia
cargo build

# Generate the test corpus (needs ffmpeg on PATH)
cargo xtask gen-fixtures            # shorthand
# equivalently:
cargo run -p xtask -- gen-fixtures

cargo test --workspace
```

If FFmpeg is not installed, the fixture‑based integration tests skip gracefully;
unit tests and the in‑memory mux round‑trip still run.

## The dev loop

These are the same checks CI runs, in the order you'll reach for them:

```bash
cargo build                                                  # compile
cargo test --workspace                                       # run tests
cargo fmt --all                                              # format
cargo clippy --workspace --all-targets -- -D warnings        # lint
```

Before pushing, run the full gauntlet so CI has nothing to complain about:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features               # rustdoc must be warning-free
```

CI additionally builds on the 1.75 MSRV and runs the test suite on macOS and
Windows, so keep changes portable and free of newer‑than‑1.75 language features.

## Code standards

The workspace lints are strict on purpose — they keep the codebase idiomatic and
safe. A PR is expected to satisfy all of them:

- **`cargo fmt` is law.** Formatting is enforced with `--check` in CI.
- **Clippy is clean at `-D warnings`.** The workspace turns on `clippy::all` and
  `clippy::pedantic`. A handful of pedantic lints are globally allowed (see the
  `[workspace.lints]` table in `Cargo.toml`); any *additional* `#[allow(...)]`
  must be local and justified with a comment at the use site.
- **Every public item is documented.** `missing_docs` is a warning across the
  workspace and rustdoc is built with `-D warnings`. New public types, traits,
  and functions need doc comments; anything that returns `Result` should say
  when it errors.
- **No `unsafe`.** The workspace sets `#![warn(unsafe_code)]` and there is
  currently **zero** `unsafe` in the tree. Keep it that way. If you believe a
  change genuinely needs `unsafe`, open an issue to discuss it *first* — it will
  need a written safety argument and a very good reason.
- **Tests are required.** New parsing logic needs unit tests; new format support
  needs round‑trip and/or ffprobe‑validated integration tests. Bug fixes should
  come with a regression test that fails before the fix.
- **Minimal dependencies.** `rustmedia-core` and `rustmedia-io` pull in nothing;
  don't add third‑party crates to them. Elsewhere, prefer the standard library
  and justify any new dependency in your PR description.

## How the codebase is organized

RustMedia is a Cargo workspace of small, focused crates:

```
rustmedia            ← ergonomic facade: Media::open, ops::{remux,trim,extract}
├── rustmedia-core   ← type vocabulary (Track, Packet, Codec, Timestamp) · zero deps
├── rustmedia-io     ← endian-aware readers, seekable sources · zero third-party deps
└── rustmedia-formats← native parsers + muxers, the Demuxer/Muxer traits
rustmedia-cli        ← the `rustmedia` binary (clap)
xtask                ← fixture generation (`gen-fixtures`), dev tooling only
```

One engine, many front‑ends: every parser implements the `Demuxer` trait and
every writer the `Muxer` trait, so the CLI, the facade, and future
WASM/Node/Python bindings all speak the same vocabulary. For the full tour, read
the [architecture guide](docs/architecture.md), and see
[`docs/format-support.md`](docs/format-support.md) for the capability matrix.

## Adding a new format

New container support is the most valuable kind of contribution. The shape of
the work is the same for every format:

1. **Teach detection.** Add the container's magic bytes to `detect_bytes` in
   [`crates/rustmedia-formats/src/detect.rs`](crates/rustmedia-formats/src/detect.rs),
   returning the matching `ContainerFormat`. (The `ContainerFormat` enum in
   `rustmedia-core` already has variants for the planned formats — Matroska,
   WebM, FLAC, Ogg — so most of the time you won't need to add one.)
2. **Write the demuxer.** Add a module under
   `crates/rustmedia-formats/src/` (e.g. `matroska.rs`) with a type that
   implements the [`Demuxer`](crates/rustmedia-formats/src/demux.rs) trait:
   `format`, `tracks`, `metadata`, `duration`, `read_packet`, and — for
   seekable containers — `seek`. Map the container's codec identifiers onto the
   `Codec` enum, and capture codec‑init/extradata into `Track::codec_private`
   so downstream remuxing stays lossless.
3. **Register it.** Wire the new demuxer into the `open()` match in
   [`crates/rustmedia-formats/src/lib.rs`](crates/rustmedia-formats/src/lib.rs)
   so a detected file is routed to your parser, and re‑export the type.
4. **(Optional) Write the muxer.** To support writing the format, implement the
   [`Muxer`](crates/rustmedia-formats/src/mux.rs) trait (`start`,
   `write_packet`, `finish`, optional `set_metadata`).
5. **Prove it.** Add a fixture to the `xtask gen-fixtures` corpus and tests that
   validate your output against `ffprobe`/`ffmpeg`.

Use the existing WAV parser as a compact reference and the MP4 parser as the
full‑featured one.

## Commits and pull requests

- Keep commits focused and their messages in the imperative mood
  ("Add Matroska demuxer", not "added matroska").
- Reference the issue you're closing (`Fixes #123`) where relevant.
- In the PR description, say what changed, why, and how you tested it. Mention
  any new dependency and any deliberately allowed lint.
- Make sure the full gauntlet above passes locally before you push — a green PR
  is a fast review.
- By submitting a contribution you agree it is dual‑licensed under MIT and
  Apache‑2.0, matching the project.

## Where to ask questions

- **Bugs and feature ideas:** open a [GitHub issue](https://github.com/rustmedia/rustmedia/issues).
- **Design discussion / "is this a good approach?":** open a draft PR or a
  discussion thread before writing a lot of code — we're happy to help you shape
  it early.
- **Security issues:** do **not** file a public issue; follow
  [`SECURITY.md`](SECURITY.md).

Please be kind and constructive; all participation is governed by our
[Code of Conduct](CODE_OF_CONDUCT.md).

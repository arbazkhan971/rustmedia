# Security Policy

RustMedia parses **untrusted media files** — the whole point of the library is to
read bytes you did not create. That makes parser robustness a security property,
not just a quality one: a malformed or hostile file must never lead to memory
unsafety, and should fail with a clean error rather than a panic or a hang.

Some things that work in our favor:

- **No `unsafe`.** The workspace sets `#![warn(unsafe_code)]` and there is
  currently **zero** `unsafe` in the codebase, so classic memory‑corruption bugs
  (out‑of‑bounds reads, use‑after‑free) are ruled out by the language.
- **No C dependencies.** There is no `libav`, no FFmpeg subprocess, and no
  bundled C — so RustMedia does not inherit the CVE stream of those libraries.
- **Fuzz testing is planned.** Coverage‑guided fuzzing of the demuxers against
  malformed inputs is on the roadmap; findings that turn up crashes, panics, or
  unbounded resource use are exactly the kind of report we want.

Even with those guarantees, a parser can still be tricked into panicking,
allocating too much, or looping — please report anything in that space.

## Supported versions

RustMedia is pre‑1.0. Security fixes are made against the latest `0.1.x` release.

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

## Reporting a vulnerability

**Please do not report security issues through public GitHub issues.**

Email **security@rustmedia.dev** with:

- a description of the issue and its impact,
- the affected version(s),
- and, ideally, a minimal media file or test case that reproduces it.

We'll acknowledge your report, work with you on a fix and a coordinated
disclosure timeline, and credit you in the release notes unless you'd prefer to
remain anonymous. Please give us reasonable time to ship a fix before any public
disclosure.

Thank you for helping keep RustMedia and its users safe.

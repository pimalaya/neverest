---
cairn: log
change: the-cpu-token-is-installed
date: 2026-11-17
---

# The CPU token is installed

Findings COH-12 (neverest half) and PKG-9 of the CLI family analysis of 2026-11-15.

## What landed

- `main` installs the detected SIMD CPU token first (simd-cpu 0.1, as himalaya and cardamum do), so the base64 and quoted-printable codecs neverest reaches through mime-rs and io-pimdir leave their portable baseline.
- The unused `percent-encoding` dependency is dropped (`crossterm` already left with 305cdf0).
- Not measured: no timing was taken.

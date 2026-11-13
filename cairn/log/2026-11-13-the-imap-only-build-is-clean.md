---
cairn: log
change: the-imap-only-build-is-clean
date: 2026-11-13
---

# The IMAP-only build is clean

A trivial fix, landed without a proposal. `cargo clippy --no-default-features --features rustls-ring,imap --all-targets -- -D warnings` failed on `Client::find_message` (src/client.rs): with `imap` the only backend, `Client` has one variant, which the `Client::Imap` arm matches whole, so the wildcard arm was unreachable.

## What landed

- The wildcard arm of `Client::find_message` is compiled only where another variant exists: `#[cfg(any(not(feature = "imap"), feature = "msgraph", feature = "dav", feature = "gpeople", feature = "gcal", feature = "gmail"))]`. No behaviour moves.

The other wildcard arms of `Client` carry `#[allow(unreachable_patterns)]`. This one is gated instead, as the owner asked: the allow would have done as well, and one spelling for all of them is the owner's to pick.

## Evidence

The command above is clean, with the other feature sets of the log of a-message-is-keyed-by-the-formats-reading.

## Capabilities moved

None.

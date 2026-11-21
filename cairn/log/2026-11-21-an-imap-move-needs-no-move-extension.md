---
cairn: log
change: an-imap-move-needs-no-move-extension
date: 2026-11-21
---

# An IMAP move needs no MOVE extension

OVH advertises IMAP4rev1 and UIDPLUS without MOVE. Its refusal of the unconditional UID MOVE led to repeated BAD responses, a closed connection and a partially synchronized account. Report: https://github.com/pimalaya/neverest/issues/35.

## What landed

The IMAP adapter chooses native MOVE from the authenticated capabilities, including IMAP4rev2. Without it, a single-message move searches its destination, copies only when absent, marks the source UID Deleted and expunges that UID alone, using existing io-imap operations (RFC 6851 section 3.3 and RFC 4315 section 2.1).

The push passes the searchable identity already derived by the kind seam, excluding minted and fallback keys. Missing identities and servers without UIDPLUS are refused before mutation. A new connection searches again after an uncertain copy result or a failed store/expunge, finding the prior copy rather than filing it again. This uses the existing identity-based retry convention; it does not add a durable COPYUID journal or claim transactional behavior under concurrent external mailbox changes.

The shared fake now advertises the native MOVE it already accepted and can withhold that capability. Scripted loopback tests exercise both native paths, fallback order, unsupported cases, failed search/copy, refused native moves, and retry after a lost copy answer or failed store/expunge. A driver test checks that ordinary identities reach the fallback while missing, derived and minted identities do not.

## Spec

[sync](../spec/sync.md) gains *An IMAP move respects the server's capabilities*. CHANGELOG records the fallback and its identity requirement.

## Validation

The all-feature offline suite passed, as did the added driver test, strict Clippy, formatting, the documented five-build feature matrix and cargo-deny (existing duplicate-dependency and unmatched-organization warnings). The global missing-docs check stops at the unchanged build.rs, which has no crate documentation. Strict rustdoc stops at the existing unresolved sync_verb link in src/sync/driver/reconcile.rs. No live OVH credentials were available; the reporter must validate deployment against the real account.

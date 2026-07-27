---
cairn: log
change: mail-meta-schema
landed: 2026-08-01
---

# Versioned mail meta schema (`v: 1`)

Widened the `message/rfc822` `meta` blob from the ad-hoc `{subject, from, date}`
to a stable, versioned `v: 1` JSON summary (`v`, `message_id?`, `subject`,
`from?`, `to?`, `date?`, `size?`; absent optionals omitted), so a reader (the
Himalaya pimdir backend, action plan M4) can render an envelope list without
fetching a body. `meta` is opaque at the store level (`ReplicaMeta(String)`,
`items.meta TEXT`, `serde_json`-encoded) — the contract is just a documented JSON
shape, no shared crate, io-pimdir stays kind-agnostic.

`remote.rs`: widened `MetaSummary` and emitted it from both writer paths —
`envelope_meta` (enumerate/`Meta`, from the `Envelope`, which already carries
`to` + `size`) and `parse_headers` (streamed/`Full`, adding recipient extraction
and threading the stream's known octet length in as `size`). Documented the
schema in `pimdir/SPEC.md` §13 ("Application meta conventions"), the cross-tool
reference.

Verified: three new `remote` unit tests — both writer paths produce parseable
`v: 1` JSON with the expected fields, and absent optionals are omitted (not
`null`) so a reader's `Option` fields default to `None`. `cargo test --bins`
green (11), fmt clean (one pre-existing unrelated clippy warning in
`wizard/autoconfig.rs`).

Spec updated: `sync` (ADDED: the mail summary is a versioned schema).

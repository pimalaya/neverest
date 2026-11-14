---
cairn: log
change: an-unknown-key-is-refused
date: 2026-11-14
---

# An unknown key is refused

neverest follows the owner's family-wide decision of 2026-11-14: a configuration key nothing reads fails loading wherever it sits, and a key an earlier neverest read is refused by name with its replacement and a link to MIGRATION.md.

## What landed

- src/config.rs: `Config` denies unknown fields, so the root holds `accounts` alone.
- src/config.rs: `SourceConfig` reads through `SourceTable` (`deny_unknown_fields`, one optional field per backend table plus `smtp`) and `TryFrom`, which refuses a source naming no backend or two, naming both. It replaces the `#[serde(flatten)]` read, which let a key beside the backend table by and kept the first of two backend tables. The TOML is unchanged, and so is serialization; `SourceBackendConfig` loses its unused `Deserialize`.
- src/config.rs: `AccountConfig` gains the removed `folder` and `envelope`, `SmtpConfig` the removed `login` and `password`, refused by `AccountConfig::validate` (`SmtpConfig::reject_removed_keys` on the account `smtp` and on each source's and target's). Every removed-key message ends `See https://github.com/pimalaya/neverest/blob/master/MIGRATION.md` (`MIGRATION_NOTES`). `collection.namespace` is looked for through `AccountConfig::sources`, the sugar included, where a backend written directly under the account used to pass with it; the names it lists are sorted.
- MIGRATION.md: the refusal among what changes from v0.3.0, and the flat SMTP keys' replacement under "From a git master build". CHANGELOG.md: a **BREAKING** Changed entry and an upgrade note.

## Evidence

New tests in src/config.rs, run against the old code first: `an_unknown_key_is_refused_at_every_level` failed on the root, source-table and target-table typos (they loaded); `a_source_declaring_two_backends_is_refused` failed (an `imap` and a `msgraph` table loaded); `a_removed_key_points_at_the_migration_notes` failed on every case (no message named MIGRATION.md, `folder`, `envelope` and the flat SMTP keys fell to the generic error, `imap.collection.namespace` validated). The backend, `smtp`, `tls.rustls`, `collection` and `store` typo cases already failed loading, those structs denying unknown fields. `every_key_the_sample_documents_is_known` loads each `key = value` line of config.sample.toml, commented or not, alone under an account and checks none is refused as unknown (it passes before and after). `a_source_pairs_one_backend_with_its_send_channel` asserted the first-of-two behaviour and serde's flatten message; it now asserts the unknown field and a source with its `smtp`. `the_flat_login_and_password_spelling_is_refused` goes through `validate`, where the refusal now is.

`cargo fmt`; `cargo clippy --all-targets -- -D warnings` over the default features, `--all-features` and `--no-default-features`: clean. `cargo test` and `cargo test --all-features`: 531 unit tests and every local integration test pass; the server and live suites (`#[ignore]`, Stalwart, Radicale, Graph, Google) were not run, their TOML checked by hand to hold only keys the configuration reads.

## Capabilities moved

- sync (cairn/spec/sync.md): added "The configuration refuses every key it does not read"; modified "The send channel authenticates like the sync side".

## Left

- A removed key in an account `-a` leaves out is not refused, `validate` running on the accounts a command opens, where an unknown key anywhere fails the load.
- Keys removed between unreleased git builds (`conflict.notify`, the Graph device-flow tables under `msgraph.auth`) stay generic unknown fields.
- neverest's config.sample.toml has no alternative sections for pimalaya-config's `sample::variants`, so the sample is checked key by key instead.

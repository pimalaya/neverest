---
cairn: change
id: the-alignment-housekeeping
status: landed
created: 2026-11-15
---

# The alignment housekeeping

## Why

Ledger lines N29, X2, X6 and W8 (held) of the CLI alignment round of 2026-11-15 (.github/cairn/changes/cli-alignment-2026-11-15/tasks.md), none of which moves a requirement of the spec.

- **N29**: CI was red at b05a2a3. The live test `a_graph_add_lands_in_drafts_only` (tests/msgraph.rs) expected an add outside Drafts to be queued and then rejected by the sync, where `drafts_only` (src/offline/capability.rs, since efc2cb3) declares `mail.message.add` `none` source-wide and `full` on Drafts alone, so the store refuses the add at enqueue and the test's helper panicked. The one `live-tests` job ran `--test google --test msgraph` in one command, so Google's quota failure stopped it before the Graph binary ran, which hid the stale test since efc2cb3.
- **X2**: the family's housekeeping: MIGRATION.md headings as "From vX to vY", landed changes archived, a verify script in every cairn, the code table in one spec file.
- **X6**: pimalaya-config 0.5.2 (an unknown key's error lists no removed key).
- **W8**: the move to io-webdav 0.10, held (see Decisions).

## What

- tests/common/mod.rs: `Replica::try_add` and `try_enqueue_body` answer the store's refusal (`PimdirError`) where `add` and `enqueue_body` panic on it.
- tests/msgraph.rs: the add outside Drafts expects `PimdirError::Unsupported` with the refusal the gate builds (`mail.message.add`, the source `live/msgraph`, the detail "Drafts only: Graph creates every MIME message as a draft"), no item queued, and a sync after it exiting 0 with nothing filed in the folder.
- .github/workflows/tests.yml: the live job is `live`, a matrix over `google` and `microsoft` with `fail-fast: false`, as himalaya's; each runs its own test binary with its own secrets.
- MIGRATION.md: "Highlights" and "Suggested steps" are the sections of "From v0.1 to v0.4" (they take a v0.1 or v1.0.0-beta setup to the coming release: `account check`, `init`, exit 3 and 4); "From v0.3.0, or any earlier build, to the next release" is "From v0.3 to v0.4"; "From a git master build" is its last subsection, as himalaya keeps "From a development build of v3"; "From v1.0.0-beta to v0.2" and "From v0.1 to v0.2". The three sentences naming a section link to it.
- cairn/changes: the 113 landed changes still beside the active ones move to cairn/changes/archive/ (`mv`), beside the 59 already there. The five active ones and the jmap-sources draft stay. None of the moved changes holds a relative link, and no file of neverest but the jmap-sources draft names one by path; log entries keep their text.
- cairn/verify.sh, calendula's, unchanged. It reports the cairn conformant but for the five files of cairn/spec/testing/, which carry no frontmatter and belong to the live-testing session.
- Cargo.toml requires pimalaya-config "0.5.2"; Cargo.lock moves it alone.

## Decisions

- **neverest stays on io-webdav 0.9.1 until io-webdav exposes the served href of a listed entry, so a key can be the segment served.** io-webdav 0.10 hands a segment back decoded where its encoding gives the segment back (`My%20Cal` as `My Cal`); encoding it again restores the served segment for a server that encodes its hrefs, but a server sending a raw space or a raw non-ASCII byte gets it kept raw by io-webdav and encoded by neverest, a store key changing, read as a collection or a member gone and another created. neverest gains nothing from 0.10 (it writes no calendar colour, and its rename sets the display name alone), so it does not take that risk. The re-encoding and its regression tests were written and set aside for that move.
- **No code table built.** Every code src/error.rs defines is already specified in cairn/spec/sync.md, the one spec neverest has, under the requirement that raises it: `backend-not-built` (Every remote backend is a cargo feature), `usage-invalid` (A refused command line exits 2), the store codes (A store error carries its code), `account-missing` (`-a` filters the accounts a command runs over) and the server refusals in their `| Code | When |` table (A server's refusal carries its code). A family-format table would repeat the store list and the refusal table in the same file, two statements of one vocabulary. Gathering them belongs with splitting a commands.md out of sync.md, the CLI requirements (exit statuses, `-a`, the account group, the JSON output, the codes) moving with it, which is a spec reorganisation for its own change.
- **The live job is renamed `live`**, its checks reading `live (google)` and `live (microsoft)` as himalaya's do. A branch protection naming `live-tests` would need the new names.
- **No CHANGELOG line for the test, the CI or the headings**: no user sees them. X6 has one.

---
cairn: log
change: a-server-without-uidplus-runs-in-ci
date: 2026-11-20
---

# A server without UIDPLUS runs in CI

pimdir audit line Y2 (pimdir cairn/changes/audit-2026-11-12, line 46, and the-freeze-plan section 1): the two rules neverest keeps for an IMAP server without UIDPLUS (RFC 4315), a remove whose target already holds the identity held rather than relocated, an append answered with no `APPENDUID` bound by its `Message-ID`, had only fake-server tests (2026-11-19-a-dry-run-replica-is-a-consistent-copy). They now run against a real one, maddy 0.9.6 from nixpkgs, in the job every push runs.

## What landed

- tests/without_uidplus.rs: each test starts its own maddy on a free loopback port, plain IMAP, its state in a temporary directory, the user made by `maddy creds create` and `maddy imap-acct create`; no TLS is needed. It passes untested when `NEVEREST_MADDY` (the binary) is unset, and checks first that the logged-in `CAPABILITY` holds neither UIDPLUS nor IMAP4rev2. Another client is a bare IMAP session over the socket.
- `a_move_into_a_mailbox_holding_its_identity_is_held`: a message of `Held` queued to move into `Filed` after another client filed a copy there. Three runs, the last after the other client flagged the member: each exits 3 with one rejected `delete` in `Held`, the member stays without `\Deleted` (its `\Flagged` kept), `Filed` holds one copy on the server and in the store. The first run is narrowed to `Held`, so the push asks the target (`find_message`); the next ones list `Filed` first, whose copy lands the move's create, and the engine derives a plain delete.
- `an_append_without_appenduid_is_bound_by_its_message_id`: a queued create appended once, bound after the first run to the UID the server holds, a second run changing nothing (no item hunk, no download).
- shell.nix sets `NEVEREST_MADDY`, so `nix develop -c cargo test --all-features`, the pimalaya/nix tests workflow tests.yml calls, runs both.

## Verified

- Each test fails on a mutation of the rule it pins: the remove relocating although the target holds the identity (src/sync/remote.rs, the first run then exits 0 having moved it), and the append failing after it landed, the 2026-10-30 behaviour (src/imap/backend.rs, the run exits 3). The code was restored after each.
- `cargo test --locked --all-features` in the dev shell: all pass, the two maddy tests included; with `NEVEREST_MADDY` unset they pass untested. `cargo clippy --locked --all-targets -D warnings` clean over all features, the default set, rustls-ring+imap and native-tls+imap+smtp. `nix flake check` passes.

## Left

- IMAP binds no revision (src/imap/backend.rs, `revision: None`), so a remove reaches the server with no `if_match` and a stale one cannot be expressed against IMAP; the third run (the member changed by another client) stands for it.
- Whether the append was bound by the search or landed by the same run's listing cannot be told from outside: a mutation answering no UID still binds it after the first run. The search itself is checked against maddy by the test's own `UID SEARCH HEADER Message-ID`.

No capability moved.

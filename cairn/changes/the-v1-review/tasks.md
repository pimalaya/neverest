---
cairn: tasks
change: the-v1-review
---

# Tasks

## Rules for the implementer

- Phase 0 moves this plan to neverest/cairn/changes/the-v1-review/ (proposal.md = Decisions and Scope, tasks.md = the phase checklists, delta.md filled as phases land); each phase ends folding its delta into cairn/spec/sync.md and a log entry cairn/log/2026-MM-DD-the-v1-review-<phase>.md.
- One conventional commit per phase (owner granted commits and the final push on 2026-11-16), no AI trailer; pushed once at the end. No publish, no version bump (Cargo.toml stays 0.3.0, D1).
- No live run: live suites (tests.yml `--ignored`) and any sync against real accounts are the owner's; give the command and stop.
- No `//` comment in any line you add (F4, hook of phase 0); `// SAFETY:` only.
- Each phase passes the gate below before its commit. A behaviour fix needs a test that fails on the old code first.

Gate (from neverest, all through `nix develop -c`): `cargo fmt --check`; `cargo clippy --locked --all-targets -- -D warnings` for the CI sets (default, `--all-features`, `--no-default-features --features rustls-ring`, `--no-default-features --features native-tls,imap,smtp`, and `--no-default-features --features rustls-ring,<b>` for b in imap smtp carddav caldav msgraph gpeople gcal gmail search); `cargo test --locked --all-features`; `cargo test --locked --no-default-features --features rustls-ring,imap,smtp`; `cargo deny check`; `cairn/verify.sh`.

## Phase 0: setup

- [x] Move this plan into cairn/changes/the-v1-review/ (proposal.md, tasks.md, delta.md with the literal headings); run `cairn/verify.sh`.
- [x] Install the hook with the `update-config` skill: a PreToolUse hook in ~/.claude/settings.json, matcher `Edit|Write|MultiEdit`, command `perl ~/.claude/hooks/no-inline-comments.pl`, the script:

```perl
#!/usr/bin/env perl
use strict; use warnings; use JSON::PP;
my $in = decode_json(do { local $/; <STDIN> });
my $t = $in->{tool_input} // {};
my $path = $t->{file_path} // '';
exit 0 unless $path =~ /\.rs\z/;
my @pairs;
if (defined $t->{content}) {
  my $old = -e $path ? do { local (@ARGV, $/) = ($path); <> } : '';
  @pairs = ([$old, $t->{content}]);
} elsif (defined $t->{edits}) {
  @pairs = map { [$_->{old_string} // '', $_->{new_string} // ''] } @{$t->{edits}};
} else {
  @pairs = ([$t->{old_string} // '', $t->{new_string} // '']);
}
my @bad;
for my $p (@pairs) {
  my %old = map { $_ => 1 } split /\n/, $p->[0];
  for my $l (split /\n/, $p->[1]) {
    next if $old{$l} || $l =~ m{//\s*SAFETY:};
    push @bad, $l if $l =~ m{^\s*//(?![/!])} || $l =~ m{[;{},)]\s*//\s};
  }
}
exit 0 unless @bad;
print STDERR "AI-written inline // comments are forbidden (only ///, //! and // SAFETY:):\n";
print STDERR "  $_\n" for @bad[0 .. ($#bad < 4 ? $#bad : 4)];
exit 2;
```

- [x] Baseline at f84c242: `cargo test --all-features` 646 passed, 70 ignored (live); `--no-default-features --features rustls-ring,imap,smtp` 378 passed, 49 ignored.

## Phase 1: deletions and small fixes

- [x] Owner's Cargo.toml and deny.toml as staged.
- [x] D3: delete `MIGRATION_NOTES`, the removed-key checks and their tests in src/config.rs (`AccountConfig::validate`, `SmtpConfig::reject_removed_keys`, the `removed_key!` uses); every type keeps `deny_unknown_fields`; spec sync.md:147 and its scenarios (:162, :167, :172) go.
- [x] A8: drop the `default` field and its warning; a config naming it fails as unknown.
- [x] D2: delete MIGRATION.md; reword the `store-version` refusal (src/offline/state.rs:276) to keep its order (sync with the neverest that wrote it until nothing is queued, move the store directory away, `neverest init`, sync; never delete) without citing MIGRATION.md; spec sync.md:343 likewise; remove MIGRATION.md from README and from CHANGELOG lines.
- [x] A3: delete src/offline/prof.rs and its 8 call sites.
- [x] Remove the dead direct `crossterm` dependency (Cargo.toml).
- [x] src/cli/account.rs:20: "List and check the configured accounts".
- Done when: `rg -n 'MIGRATION|removed_key|NEVEREST_PROFILE|crossterm' src Cargo.toml cairn/spec` finds nothing; gate green.

## Phase 2: plain output (D4)

- [x] Depend on pimalaya-cli `features = ["terminal", "signal"]` (not `spinner`); keep `signal::Shutdown` (src/sync/mod.rs:17).
- [x] Remove every `Spinner`: src/account.rs:109, src/cli/check.rs:218 and :239, src/cli/init.rs:145 and :152, src/cli/index.rs:101, src/offline/progress.rs (`Step`), src/offline/driver.rs:426.
- [x] Text mode renders the same `SyncEvent`s (src/sync/event.rs) `--json` emits: one plain line each on stderr, at step and collection granularity, never per item, no `\r`, no colour, no cursor move; stdout keeps only the result. progress.rs becomes one event stream with two renderers.
- [x] `check`, `init` and the credential resolution print a plain line per step on stderr in text mode, nothing under `--json`.
- Done when: `cargo tree -i crossterm --all-features` finds nothing; a test asserts a text-mode run's stderr holds no ESC byte and one line per collection; gate green.

## Phase 3: backend duplication and the suspected bug (A6)

- [x] 1. Lift msgraph's `fake::Server` (src/msgraph/client/wire.rs:200-375) into a crate-level `#[cfg(test)]` src/scripted.rs with per-connection `Step` scripts, gmail's optional responder, `requests()`, `lines()`, `json()`, `answer()`; replace gcal `Recorded`/`Answering`/`request_lines`, gpeople `Canned`/`Scripted`, dav `Replay`/`scripted::Wire`, gmail `scripted::Wire`. Test counts unchanged.
- [x] 2. Suspected bug: gcal and gpeople `send` return on `run(&mut self.inner)?` (src/gcal/client.rs:191, gpeople equivalent) before updating `alive`, so a stream that failed mid-exchange is never reopened; gmail sets `alive = false` on `Io` (src/gmail/client.rs:320-325). Write the test first: a scripted stream failing one exchange with an I/O error, the next request must reconnect. It must fail on today's gcal and gpeople. If it passes, drop the fix and record why.
- [x] 3. Factor the Google request loop (gcal :143-211, gpeople :95-184, gmail :233-345 core) into one generic function in src/throttle.rs carrying gmail's `Io` rule; the bug test passes.
- [x] 4. Factor TLS stream opening (gcal `open_stream` :1026, gpeople `reconnect` :98, gmail `open_stream` :1500, msgraph `connect` src/msgraph/client.rs:193) into one `open_tls(url, tls, proxy)` in src/http.rs, gated on the HTTP backend features.

## Phase 4: `sync` flags (D6)

- [x] Add to `sync`: `--init`, `--queue-only`, `--index-only`; clap `conflicts_with_all` so the two `-only` flags exclude each other and `--init`.
- [x] `--init`: initializes each covered account not yet initialized (the `init` path, src/cli/init.rs), then syncs; the report lists the accounts it initialized. Without it, an uninitialized account fails as today, the message (src/offline/state.rs:171) naming `neverest init` and `neverest sync --init`.
- [x] `--queue-only`: the old `drain` (src/cli/drain.rs): applies the queue, refreshes the index under `store.index-timeout`, no credential, no endpoint, `busy` exit 4 while a sync holds the store.
- [x] `--index-only`: the old `index` (src/cli/index.rs): builds until current, no deadline, no queue, no endpoint.
- [x] Remove the `drain` and `index` commands; their reports fold into the sync report (camelCase, every key printed); json-schema, completion and man output follow.
- [x] Spec: sync.md:267 and :2213 command lists, the drain and index requirements moved under `sync`. README and help name the flags.
- Done when: tests cover each flag, each conflict, `--init` on initialized and uninitialized accounts, and `--queue-only` busy; gate green.

## Phase 5: restructure

- [x] Config (item 11), himalaya's recipe (himalaya cairn/log/2026-11-14-the-config-splits-by-block.md): plain model, no feature gate in src/config/; conversions out as inherent impls in src/tls.rs, src/proxy.rs, src/sasl.rs, src/server.rs gated once in src/main.rs; one root dead-code allow; block files under src/config/ named as himalaya's where shared (tls.rs, proxy.rs, sasl.rs, smtp.rs, imap.rs, msgraph.rs, gmail.rs) plus neverest's own (account, store, mode, source, permissions, filter, dav, jmap, gcal, gpeople, conflict), each with its tests.
- [x] Modules (A5): src/collection.rs, src/flag.rs (header: mail keywords), kind/ = mod.rs + mail.rs, src/conflict/merge.rs, src/calendar.rs.
- [x] Engine (A1, A2): offline/ into sync/ (stale "hand-rolled 3-way diff" header removed); driver.rs into sync/driver/ by phase (run and pair loop, local run, spine phases, queue drain, relay), each file with its tests; functions over the A7 limit cut into named steps; pipe.rs replaced by `std::io::pipe`.
- Done when: no file over the A7 limits but those listed as exceptions in tasks.md with a reason; test count equal to baseline plus new tests; gate green. Pure moves first, one commit each (config, modules, engine), behaviour unchanged.

## Phase 6: comments and doc ceilings (D5, item 7)

- [x] Remove every `//` comment line but `// SAFETY:` (886 at review time, 292 `NOTE:`). Per comment: narration goes; a real why (RFC quirk, provider trap, invariant) moves to the enclosing item's `///` within inline-002's ceiling, to a test named after the behaviour, or to the spec. Never bulk-move into `///`.
- [x] Trim `//!` paragraphs to four lines (inline-001) and `///` to one line plus at most three (inline-002), clap items in src/cli/ excepted (inline-006); main.rs keeps every section (header-004). src/error.rs:3-32's list of unmet io-pimdir codes moves to the spec.
- [x] Add tests/docs.rs (doc ceilings only, since D5 lets humans write `//`): it checks (a `//!` paragraph over four lines, a `///` block over 1 + blank + 3 outside src/cli/) and the README rules of phase 7.
- Done when: `rg -n '^\s*//[^/!]' src tests | rg -v 'SAFETY:'` finds nothing; tests/docs.rs passes; gate green.

## Phase 7: user-facing docs

- [x] config.sample.toml on himalaya's shape (himalaya cairn/log 2026-11-11-the-sample-is-tested-and-shipped.md and 2026-11-14-the-sample-lists-values-in-brackets.md): opens on `[accounts.example]`, no header, every line commented, one aligned trailing comment per key, enum values in brackets, a short `# <Section>` per backend, TLS/proxy/ALPN written once; no `default`. Port himalaya's `the_sample_configuration_parses_with_each_variant` (himalaya src/config.rs:497 and its helpers :606-660). Ship it as share/doc/neverest/config.sample.toml in default.nix and install.sh, as himalaya does.
- [x] Account-model prose leaves the sample: the arity table stays as the one block of README Configuration; the rest goes to `sync`'s and `account`'s `--help` (inline-006 allows the length).
- [x] README (D7): draft the tightened readme rules in ~/code/pimalaya/.github GUIDELINES.md (per-section budget, Usage as a few command lines with no subsections, behaviour explained in `--help`, not the README); owner reviews the rules before the rewrite. Then rewrite neverest's README: Features at most 8 bullets of at most 120 characters (readme-008), sections in readme-007 order, no Conflicts/Retention/Duplicated identities/Coming from Maildir subsections, no cargo-feature callouts. tests/docs.rs checks Features count, bullet length and section order.
- [ ] CHANGELOG [Unreleased] last, on io-imap's template (~/code/pimalaya/io-imap/CHANGELOG.md): one item per bullet, past-tense headline, body of one or two sentences at most, net diff against 0.3.0, no "Upgrading" block, one line saying v1 reads no 0.x configuration or store. Released sections untouched.


### A7 exceptions after phase 5 (2026-11-16)

Kept above the limits with a reason; each is a candidate for its own split, not done here because cutting engine and backend functions is a behaviour risk that wants its own review.

- Files over 1000 non-test lines: src/gmail/client.rs (1871), src/sync/remote.rs (1796), src/msgraph/client.rs (1729), src/imap/backend.rs (1530), src/sync/report.rs (1527, mostly `Display`), src/client.rs (1419), src/dav/client.rs (1219), src/msgraph/client/mail.rs (1186), src/gcal/client.rs (1006). src/sync/driver/tests.rs (6636) is test code only, its helpers shared by every phase.
- Functions over 80 lines in src/sync/: `run_pair` 289, `sync_collection_into` 86 (driver/pair.rs); `drain_submits` 239 (driver/send.rs); `SyncOutput::fmt` 242 (report.rs); `run` 206 (driver.rs); `run_local` 193 (driver/local.rs); `phase1_spine` 171, `collection_spine` 102 (driver/spine.rs); `Landing::land` 107 (driver/hydrate.rs); `itemize_noted` 106, `placement_hunks` 95 (driver/itemize.rs); `merge_conflicts` 102, `keep_store_bodies` 88 (driver/reconcile.rs); `enumerate` 104, `push` 104, `hydrate_batch` 97 (remote.rs); `run_verb_paged` 97 (mod.rs). The backends were not measured.

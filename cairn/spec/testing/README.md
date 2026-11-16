---
cairn: spec
capability: testing
status: current
---

# Testing reports

Live test reports for neverest, one per account, each a run of the steps of the live testing campaign (.github/cairn/changes/live-testing-2026-11-15, part 1, N1 to N16) against a real provider, into the one store the apps then read (~/.local/state/neverest).

The automated counterparts are the integration suites under tests/ (Stalwart, Radicale, fake servers) and the credentialed suites tests/google.rs and tests/msgraph.rs, which make their own throwaway collections on the Google and Microsoft test accounts.

| Account | Sources | Mode | Report |
| --- | --- | --- | --- |
| fastmail | IMAP, CardDAV, CalDAV | one-way, pull-only | [fastmail.md](fastmail.md) (2026-11-15) |
| icloud | IMAP + SMTP, CardDAV, CalDAV | two-way | [icloud.md](icloud.md) (2026-11-15) |
| microsoft | Graph mail, contacts, calendars | two-way | [microsoft.md](microsoft.md) (2026-11-15) |
| google | Gmail + SMTP, Google People, Google Calendar | two-way | [google.md](google.md) (2026-11-15) |

N9 (deletes) has not run on any account: enabling `item.delete` is the owner's step.

The corpus the Google and Microsoft accounts hold before any sync, which every run on them is checked against: [corpus.md](corpus.md).

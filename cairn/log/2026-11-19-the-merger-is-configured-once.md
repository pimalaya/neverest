---
cairn: log
change: the-merger-is-configured-once
date: 2026-11-19
---

# The merger is configured once

## What landed

- src/config.rs: `Config` takes document-level `contact` and `calendar` tables (`ContactConfig`, `CalendarConfig`); `Config::select`, which every command loads its accounts through, fills an account's unset merger from the document's, so `conflict resolve --interactive` still reads one `Option<CommandConfig>` a kind. Test `a_document_merger_serves_the_accounts_naming_none`, failing to build on the code before (no document field, and the document key refused as unknown).
- src/config/contact.rs, src/config/calendar.rs: the docs say an unset merger takes the document's.
- config.sample.toml: both mergers above `[accounts.example]`, the account override shown as the exception. README.md names the fallback beside `--interactive`. CHANGELOG under Changed.

## Spec

- [sync](../spec/sync.md): new *The merger is named once for the document*; *Deciding is a command, never a run* gains the placeholder paragraph of the delta, which the code already followed.

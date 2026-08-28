---
cairn: tasks
change: calendar-over-caldav
---

# Tasks

- [x] Fold `src/carddav/` into `src/dav/` behind a `DavKind`, and rename the
      `carddav` cargo feature to `dav`.
- [x] Add the `text/calendar` kind, delegating to
      `io_pimdir::conventions::calendar`.
- [x] Add `CaldavConfig` and its direct-backend sugar, sharing `DavAuthConfig`
      with CardDAV.
- [x] Open a CalDAV source from the client seam, reporting `text/calendar`.
- [x] Discover and configure a CalDAV endpoint in the wizard.
- [x] Cover the run end to end against Radicale, as `tests/carddav.rs` does.
- [x] Repair the CardDAV end-to-end test, whose assertions predate both the
      bare link ids and the source-namespaced collection key.
- [x] README.md, CHANGELOG.md, MIGRATION.md and config.sample.toml.
- [x] Fold the delta into cairn/spec/sync.md and log it.

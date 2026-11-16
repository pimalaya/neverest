---
cairn: delta
change: a-feature-is-named-after-its-table
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature named after the configuration table it builds: `carddav` for the CardDAV backend and `caldav` for the CalDAV backend, where one `dav` built both. Code serving both DAV kinds SHALL build under either feature, and opening a source of one kind only under that kind's. The rule sharing one feature between backends of one adapter keeps `msgraph` for every Graph domain, all read from one `msgraph` table, and no longer covers DAV. Both DAV features SHALL ship in the default set, and no `dav` alias SHALL be kept. The `backend-not-built` refusal of a `carddav` or `caldav` source SHALL name `--features carddav` or `--features caldav`.

#### Scenario: A CalDAV source in a CardDAV-only build
- **GIVEN** a build with `carddav` and without `caldav`, and an account with a `caldav` table
- **WHEN** the source is opened
- **THEN** it fails coded `backend-not-built`, naming the `caldav` backend and `--features caldav`

## REMOVED Requirements

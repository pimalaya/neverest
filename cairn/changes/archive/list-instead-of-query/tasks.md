---
cairn: tasks
change: list-instead-of-query
---

# Tasks

- [x] Replace `query` with a `PROPFIND` listing through the io-webdav fallback
      flag.
- [x] Report a truncated listing as a delta rather than as a complete snapshot.
- [x] Drop the local refusal classifier for the library predicate.
- [x] Choose the enumeration from the advertised report set when it is known,
      and carry that cache over a reconnect.
- [x] Cover the capability read: advertised without the report, advertised with
      it, and never listed.
- [x] CHANGELOG.md and the CardDAV module header.
- [x] Fold the delta into cairn/spec/sync.md and log it.
- [ ] Bump io-webdav to the release carrying the enumeration fallback and drop
      its `[patch.crates-io]` entry (the same tail `retention-sweep`,
      `submit-intent` and `duplicate-link-id-freeze` still carry).

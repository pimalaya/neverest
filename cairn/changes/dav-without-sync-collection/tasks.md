---
cairn: tasks
change: dav-without-sync-collection
---

# Tasks

- [x] Fall back to `addressbook-query` on the `DAV:supported-report` precondition.
- [x] Treat an empty checkpoint as no cursor, the fallback storing none.
- [x] Read the status wherever the failed send nested it, the REPORT wrapping
      one level deeper than a plain request.
- [x] Cover the classifier with the body a real server answers, in both error
      shapes, apart from a
      permission refusal, a credential failure, a server fault and a stale token.
- [x] CHANGELOG.md and the CardDAV module header.
- [x] Fold the delta into cairn/spec/sync.md and log it.

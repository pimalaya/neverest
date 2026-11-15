---
cairn: tasks
change: qresync-is-what-the-server-enabled
---

# Tasks

- [x] `ImapClient::qresync` from the `ENABLED` list; `supports_qresync` reads it
- [x] Fake IMAP `enable_nothing`; test `a_delta_lists_an_arrival_where_qresync_is_not_enabled`, which listed nothing (`[]`) on the advertised-capability gate
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG

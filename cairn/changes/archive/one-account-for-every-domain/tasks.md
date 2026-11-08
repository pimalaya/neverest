---
cairn: tasks
change: one-account-for-every-domain
---

# Tasks

- [x] Move io-pim-discovery to 0.11 and patch pimalaya-cli to the local checkout.
- [x] src/wizard/search.rs: ask for every service neverest has a backend for, answer the raw configs.
- [x] src/wizard/plan.rs (pure): routes per domain, ranking, Auto, sending, review lines, change lists, sign-in groups.
- [x] src/wizard/server.rs (pure): a typed server URL per domain, the first-prompt reading of a scheme.
- [x] src/wizard/imap.rs, src/wizard/smtp.rs: the mechanism from the CAPABILITY probe, PLAIN or XOAUTH2 for SMTP, the channel test.
- [x] src/wizard/signin.rs: grouped sign-ins, `check`'s per-endpoint test, `prompt_store`, Ortie or the broker list, the refusal menu.
- [x] src/wizard/settings.rs: the collections multi-select and the exclusion.
- [x] src/wizard/discover.rs: the review loop, the changes, `Server URL:`, the account assembled from every domain.
- [x] src/cli/configure.rs: the account name settled before the sign-ins; src/config.rs: render order, collection permissions omitted, one direct backend at a time.
- [x] Drop src/wizard/imap_smtp.rs, dav.rs, msgraph.rs.
- [x] Tests: ranking per domain, a provider plan under one grant, password grouping across the four domains, server URLs per domain, the rendered account with several sources, the exclusion.
- [x] README.md, config.sample.toml, CHANGELOG.md, the welcome text.
- [x] Fold the delta into cairn/spec/sync.md, log, archive.

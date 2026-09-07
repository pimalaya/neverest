---
cairn: log
change: msgraph-waits-for-its-domains
landed: 2026-09-07
---

# Graph does not ship syncing one domain of three

`msgraph` left the `default` feature set, one release before it gains the domain axis that makes a Graph account mean more than mail.

Nothing else moved. The feature compiles, the tests and clippy runs cover it, every `msgraph` source config still parses in every build, and a source declaring one is refused when the sync opens it, the way any uncompiled backend is. `wizard::search` already gated the offer on `cfg!(feature = "msgraph")`, so Graph stopped being proposed with no change of its own.

README.md, config.sample.toml and CHANGELOG.md now say the backend is not in a released binary and what turns it on, rather than advertising Graph among the shipped domains.

Capability moved: **sync**, the cargo-feature requirement.

It returns to the default set under `a-graph-source-declares-its-domain`, which is what gives it contacts.

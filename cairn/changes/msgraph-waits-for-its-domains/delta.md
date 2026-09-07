---
cairn: delta
change: msgraph-waits-for-its-domains
---

# Delta

## MODIFIED Requirements

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature: `imap` for the IMAP backend, `msgraph` for the Microsoft Graph backend, `dav` for the CardDAV and CalDAV backends, `smtp` for the SMTP submission channel.

CardDAV and CalDAV SHALL share one feature rather than take one each: they are one dependency, one adapter and one discovery mechanism, so separate features would gate nothing that is separately compiled. A feature that merely aliases another is not introduced for the older spelling.

All of them SHALL ship in the default feature set except `msgraph`. Graph carries mail, contacts and calendar behind one protocol, and the backend syncs mail alone with no way to ask it for anything else, which reads as a misconfiguration and raises no error because nothing went wrong. A backend that answers for one domain of three SHALL NOT be in a released binary: it stays compilable, tested and configurable, and returns to the default set when it declares which domain it syncs.

A missing backend SHALL surface at runtime, never at build time: every feature combination compiles, the configuration surface stays whole (every source config still parses), and an unavailable backend fails when the source is *opened*, as the JMAP and Gmail sources already do. A build with neither `smtp` nor `msgraph` has no send channel and SHALL warn rather than perform a submit intent. Each optional backend crate SHALL take its TLS provider from neverest's own `native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than pinning one.

#### Scenario: A Graph source in a released build is refused when it opens
- GIVEN a released binary and an account declaring an `msgraph` source
- WHEN the account is checked or synced
- THEN the configuration parses and the source is refused as unavailable in this build, naming the feature that provides it

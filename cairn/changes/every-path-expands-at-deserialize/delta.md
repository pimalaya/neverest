---
cairn: delta
change: every-path-expands-at-deserialize
---

## ADDED Requirements

### Requirement: A path key expands at deserialize, never at a call site
Every path-valued configuration key SHALL be shell-expanded by its own
deserializer, so a value reaching any call site is already resolved. A key
SHALL NOT be expanded at the point it is read: one reader forgetting is a
lookup for a literal `./~/…` path, which fails naming a file the user never
wrote.

An absent optional key SHALL stay absent rather than expanding an empty path,
which is what `#[serde(default)]` beside the deserializer buys.

#### Scenario: A certificate under the home directory is found
- GIVEN `imap.tls.cert = "~/ca.pem"`
- WHEN the configuration is loaded
- THEN the certificate path is the one under the user's home directory

## MODIFIED Requirements

None.

## REMOVED Requirements

None.

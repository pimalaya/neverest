---
cairn: delta
change: json-keys-are-camel-case
---

Folds into cairn/spec/sync.md when the rename lands, not before.

## ADDED Requirements

### Requirement: JSON keys are camelCase
Every output type neverest prints SHALL serialize its keys as camelCase, matching the wire formats the endpoints speak (JMAP per RFC 8620, Microsoft Graph, the Google APIs) and keeping every key reachable by dot access in jq and JavaScript, which is how the README's notifier reads a report. A field carrying a provider or format spelling verbatim SHALL keep its explicit `#[serde(rename)]`, `@odata.nextLink`, `nextPageToken` and the vCard `fn` being the examples: those are the wire's names rather than derivable ones. Configuration types SHALL stay kebab-case, since what the loader reads from TOML is not what the printer emits.

## MODIFIED Requirements

## REMOVED Requirements

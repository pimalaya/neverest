---
cairn: change
change: discovery-wizard
---

# Delta

## ADDED Requirements

### Requirement: The wizard discovers in parallel and proposes what it found
The configuration wizard SHALL open with a welcome banner on stderr (what
neverest is, what the wizard does, where the documented sample lives), then ask
for **one** input: an email address (a bare domain is accepted and synthesized
as `@domain`; server URLs and folder paths are not wizard inputs). The account
name SHALL be derived from the email domain's first label, never prompted.

Discovery SHALL run io-pim-discovery's parallel compose fan-out (fixed provider
rules, PACC, Mozilla autoconfig, RFC 6186 SRV) under a fan-out deadline, so one
unreachable endpoint cannot stall the wizard, and every reachable service SHALL
become one selectable entry carrying the authentication capabilities it
advertised; the concrete mechanism is picked after the entry is chosen. The
resolver SHALL be `NEVEREST_DNS_RESOLVER` when set, else the system resolver,
else a public default. Only backends compiled into the running build SHALL be
proposed, so the wizard never writes a side that `client::open` refuses; when
discovery finds nothing, the wizard SHALL stop and point at the documented
sample rather than prompting for a hand-entered config.

IMAP credential prompts SHALL be narrowed by an unauthenticated CAPABILITY
probe, falling back to the discovery-advertised list when the probe fails, and
every secret SHALL be collected through the shared keyring / OAuth-broker
pickers. When discovery found a submission endpoint, the SMTP channel SHALL be
configured too, reusing the IMAP login and password on confirmation and
prompting a dedicated pair otherwise (neverest's SMTP channel authenticates
with LOGIN only, so reuse is offered only for a login + password mechanism; a
blank login means an unauthenticated relay). Every connection the wizard
configures SHALL be tested before the configuration is written.

## MODIFIED Requirements

### Requirement: Sides are remote backends only
A sync side SHALL be a remote backend (IMAP today; JMAP/Gmail/Graph as their
backends land). Local file backends (m2dir, maildir) SHALL NOT be sync sides — the
pimdir store is the local replica, so a local file store is redundant as a side and
belongs on the import/export path (io-pimdir conversion), which neverest documents
rather than syncing directly. The wizard SHALL produce a one-side (local-sync)
remote account only: it writes `left` plus the implicit store and never a
`right`, so a remote-to-remote mirror is always configured by hand.

## REMOVED Requirements

---
cairn: delta
change: sources-declare-their-capabilities
---

# Delta

## ADDED Requirements

### Requirement: Every source declares its capabilities before the drain

At every run, before it drains an action, neverest declares each source it runs in the store (pimdir STORAGE §15.6): every Annex B capability of the source's kind, from its backend and its configured rights alone, `none` with the reason. A right the configuration withholds is `none`; a one-way source supports no write. A JMAP source stays undeclared.

#### Scenario: A source is declared with no network

- **GIVEN** a Graph source whose token command fails
- **WHEN** neverest runs
- **THEN** the source's declaration is in the store before the credential is read

### Requirement: An intent is performed by the source it names

A `submit` is sent by the source its payload names, and left pending for another run when it names a source this run does not perform.

### Requirement: Google Calendar notifies as the resource asks

An update notifies the attendees only when the resource is scheduled (pimdir Annex B.1); a delete notifies them when the event has attendees.

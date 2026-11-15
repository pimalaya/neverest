---
cairn: delta
change: a-send-is-handed-over-once
---

# Delta

## ADDED Requirements

### Requirement: A run may perform the queued intents alone
`neverest sync --intents-only` SHALL drain the queue, then perform the queued `collection-create`, `submit`, `calendar-reply` and `calendar-cancel` intents as a sync does before it lists anything, and stop: no collection listed, no other write pushed, no item fetched, no retention sweep. It SHALL open an endpoint only for an intent that needs its session: one it performs on its server, or a send through its native channel; a send through an `smtp` table SHALL open that channel alone. An endpoint that does not open SHALL be reported unreached and the other endpoints SHALL still perform theirs. It SHALL refuse `--dry-run`, `--reset`, `--declare-only`, `--since` and the collection filters, and SHALL exit as a sync does.

A sent copy neverest files (pimdir STORAGE Annex B.2) is in the store when the run ends and on the server after the next sync.

#### Scenario: A send while the mail server is down
- **GIVEN** an IMAP account with an `smtp` table, whose IMAP server cannot be reached, and a queued `submit`
- **WHEN** it runs `sync --intents-only`
- **THEN** the message leaves through the SMTP channel, the run exits 0, and the report names nothing unreached and no collection

### Requirement: A send that may have been delivered is never sent again
A `submit` that fails after the whole message was handed over and before a verdict came back SHALL be uncertain: over SMTP once the data's final `.` was written and flushed and no reply read; over Graph `sendMail` once the request was written whole and flushed and no answer read whole (none, one cut short, one that does not parse), or when 500, 502, 503 or 504 answered. A `sendMail` SHALL never be sent again on a dead connection; only a 429, which Graph answers before acting, SHALL send it again. An uncertain `submit` SHALL be parked with an error beginning `uncertain: `, SHALL be reported in `submitted` with `uncertain: true`, SHALL make the run that parked it exit 2, and SHALL never be sent again by any run.

Before the first byte of a message leaves, the run SHALL count the attempt in the store and SHALL write the intent's id to `submitting` beside `pimdir.db`, durably, clearing it once the outcome is recorded; a run finding the mark with that intent still pending SHALL park it as uncertain before sending anything. A store created or reset SHALL start with no mark. A run that cannot record an outcome SHALL keep the mark and send nothing more.

#### Scenario: A server hanging up after the data
- **GIVEN** a queued `submit` and an SMTP server that reads the data and closes the connection unanswered
- **WHEN** the account syncs
- **THEN** the run exits 2, the row is parked under `uncertain: `, and the next run sends nothing

#### Scenario: A run killed while the server holds its verdict
- **GIVEN** a run handing over a queued `submit`
- **WHEN** it is killed before the server answers
- **THEN** the row reads pending with one attempt, and the next run parks it under `uncertain: ` without sending it

## MODIFIED Requirements

### Requirement: A queued submission is a `submit` queue intent
Neverest SHALL NOT reserve a collection for queued sends. Submission is a **mail** capability: a `submit` intent belongs to a `message/rfc822` account, and an `smtp` channel declared on a source of any other kind SHALL be refused before any connection is made, rather than silently ignored. A submission SHALL be a **queue action** whose kind (`submit`) is defined by neverest, not by pimdir: the format carries an action kind and a versioned JSON payload, and which kinds an owner can perform is the owner's business. An owner that does not recognise a kind, or recognises it but lacks the capability, SHALL **skip** the row, leaving it pending, never parking it (parking means permanently unappliable) and never blocking later actions of that collection.

The intent's payload SHALL be `v: 1` JSON carrying `object` (the body hash, by the convention every action kind follows), `from` (empty means the null reverse path), `rcpts` and an optional `subject`. The body SHALL be written durably before the enqueue, so the queue row pins it (queued bodies are pinned, and no GC can sweep it between the enqueue and the send); it belongs to no collection. The intent SHALL anchor on whatever collection the producer chose: neverest scans every collection's pending actions, so there is no anchor rule.

Neverest SHALL perform each pending intent through the one source offering a send channel: its own `smtp` table, else its native send (the Graph `sendMail` action, which files the message in Sent itself), sources walked in the order the account declares them. At most one source may declare an `smtp` table, so the only pick that order decides is between a declared channel and a natively-sending source. On success the row SHALL be acknowledged with its receipt, releasing the body's pin. A **transient** failure, which certainly delivered nothing (an SMTP 4xx, a Graph 401, 408 or 429, a transport error before the message went whole), SHALL leave the row pending; a **permanent** one (an SMTP 5xx, a Graph 400, 403, 404, 413, 422 or other 4xx, an undecodable payload, a missing body) SHALL park it with its error; an **uncertain** one SHALL park it as "A send that may have been delivered is never sent again" says. A refusal SHALL reset the SMTP transaction before the next message; a failure with no reply or status SHALL leave the rest of the run's intents unattempted, pending. A build with no send channel (neither `smtp` nor `msgraph`) SHALL skip submit intents and warn, never park them. Message content is never logged.

The SMTP channel SHALL remove the body's `Bcc` field before `DATA` (RFC 5322 §3.6.3): a producer may stage the body with it, the payload's `rcpts` alone carrying those recipients to the server. The Graph channel hands the field to `sendMail`, which derives the recipients from it and does not deliver it.

Submission is **at-most-once** on its own: a message that may have left is parked, never sent again by a run, and only a person, through the producer, sends it again.

An SMTP channel that does not open SHALL be reported: a server that cannot be reached, or a session cut before its credentials were answered, under `unreached` as `<source>.smtp`, its intents pending and unattempted; credentials the server refused, by each queued `submit` failing in `submitted` with the reason, not parked, not attempted. Either makes the run incomplete.

### Requirement: An unreached endpoint is named in the report
A run SHALL list in its report, under `unreached`, every endpoint whose connections it could not open, with the error: a server it could not reach, or one refusing the connection or its credentials. The failure SHALL still read as a failed `*` scan in the collection patch and make the run incomplete (exit 3). A collection that failed to read on an endpoint that opened SHALL NOT be listed: that is a server answering, not an endpoint unreached.

A source's SMTP channel SHALL be listed as `<source>.smtp` when its server could not be reached, with no `*` scan, a send channel holding no collection; it SHALL still make the run incomplete. Credentials it refused SHALL NOT list it: the sends report the refusal.

An exit 3 says the run did not do everything, and the patch says which collection failed; neither said whether the server was reached, so a caller told "offline" from "one folder unreadable" by matching the scan of `*`. Reported by MOA, whose "provider unreachable" read the first scan error of an exit-3 report.

#### Scenario: A closed port
- **GIVEN** an account whose CalDAV source points at a port nothing listens on
- **WHEN** it is synced
- **THEN** the run exits 3 and `unreached` names `caldav` with an error beginning `Open connection`

#### Scenario: A closed submission port
- **GIVEN** an IMAP account whose `smtp` table points at a port nothing listens on, and a queued `submit`
- **WHEN** it runs `sync --intents-only`
- **THEN** the run exits 3, `unreached` names `imap.smtp`, and the row reads pending with no attempt

### Requirement: A conflicted run succeeds, with its own exit code
A run that reconciled its collections and left work behind SHALL exit with a code distinct from both success and failure, and SHALL report the outstanding conflict count read from the store rather than the count the run itself marked.

Four states are that code, and they are one class: a divergence waiting for a decision, a duplicate `UID` a side refuses, a write a side would not take, and a send this run parked as uncertain. Each leaves something a person has to settle, and a rerun on its own changes none of them. The first three are re-reported on every run until a person acts; the uncertain send is reported by the run that parked it, and after it among the parked rows, which never decide the exit code.

A conflict is one item wide, and so is a refusal. Failing the run would stop every other item over one divergence, and under a supervisor restarting on failure it would loop over a state no supervisor can resolve. The distinct code says the same thing without pretending the run broke.

The two conflict counts differ and the difference matters: the engine emits nothing for a placement already parked, which is what keeps notifications quiet across repeated runs, and which is also why the run's own tally is not the number of decisions waiting.

#### Scenario: A parked conflict does not fail the run
- GIVEN a collection holding one parked conflict beside ordinary items
- WHEN it is synced
- THEN the ordinary items reconcile, the run exits with the conflict code, and the outstanding count is reported

#### Scenario: A run that could not deliver a write says so
- GIVEN a source that refuses the only write a run had to make
- WHEN the run ends
- THEN it exits with the same code, rather than reporting success over a change that stayed in the store

### Requirement: An incomplete run has its own exit code
A run that could not do all its work SHALL exit with a code of its own, distinct from success, failure and the conflict code: a source it could not scan, an endpoint or a send channel it could not open, a hunk it could not apply, a send or an intent that failed without parking, a source that gave up throttled, or a round left open on a collection it synced. It SHALL win over the conflict code, the report still counting what waits for a person.

An unreachable server is the common case. The run stops nothing else over it, so it is no failure, yet a run with nothing to do and a run that could not reach its server both exited 0, and a caller told offline from idle by parsing error strings. A rerun clears this state on its own, which is what sets it apart from the conflict code: that one waits for a person, this one for the network.

A send or an intent that parked is not this: it waits for a person, and its row says why.

#### Scenario: An unreachable source is not an idle run
- GIVEN an account whose only source refuses the connection
- WHEN it is synced
- THEN the scan error is reported and the run exits with the incomplete code

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and enqueue mutations through io-pimdir's producer queue. A sync run SHALL drain the store's queue once, at its start, after declaring the sources and before reading any credential or opening any endpoint, so a run that cannot reach its servers still applies what was queued: exactly-once apply-and-delete per action, permanently bad actions parked, transient failures left queued in order. The store applies each action as the source syncing its collection (pimdir STORAGE §15.2), so the drain is the store's and not a source's. `--declare-only` SHALL NOT drain; `--intents-only` SHALL; a dry run drains its replica. The applied counts SHALL be logged (info when nonzero) and reported.

Every parked action SHALL surface in the run report until repaired, and SHALL surface **once per run**, read after every source has run, in a dry run as much as in a real one.

The subsequent sync of a drained collection pushes the resulting dirty state. An action kind the drain cannot apply itself (a capability-bound intent such as `submit`) SHALL be left pending for the phase that can, never parked.

#### Scenario: A sync that cannot connect
- **GIVEN** an event queued on a calendar whose server is unreachable
- **WHEN** the account syncs and exits 3
- **THEN** the event is in the store, with its `seq`

## REMOVED Requirements

None.

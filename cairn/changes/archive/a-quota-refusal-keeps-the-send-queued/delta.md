---
cairn: delta
change: a-quota-refusal-keeps-the-send-queued
---

## ADDED Requirements

### Requirement: A quota refusal keeps the send queued
An SMTP rejection SHALL be classified by its reply code and its reply text. A 5xx refusing for a sending rate or quota SHALL be transient: the row SHALL stay pending, reported under `submitted` with its error and `parked: false`, and the run's remaining sends SHALL stay pending, unattempted, each reported as not attempted. Such a refusal is a 5xx whose text opens with an enhanced status code (RFC 3463) meaning nothing else, `5.4.5` (congestion, which RFC 3463 calls a persistent transient error), `5.1.90`, `5.7.232`, `5.7.233` or `5.7.236` (codes outside the registry naming a daily sending limit), or one whose text says so under a code of other meanings, matched without case: `SubmissionQuotaExceeded`, `submission quota`, `sending limit exceeded`, `rate limited`, `daily limit for message recipients`. Any other 5xx SHALL stay permanent, a sender blocked until a person releases it (`5.1.8`, `5.7.501` to `5.7.503`, `5.7.70x`) and a sender's own full mailbox (`5.2.2` with `QuotaExceededException`) among them.

A provider refusing a send for its daily limit has not refused the message: the next day's run sends it, and parking it would leave it to a person for nothing. The set stays narrow because a misread refusal holds the whole queue, each run reporting it (exit 3) until a person acts.

#### Scenario: A daily limit holds the send
- **GIVEN** two queued `submit` intents and an SMTP server answering the first one's data with `550 5.4.5 Daily user sending limit exceeded`
- **WHEN** the account syncs
- **THEN** both rows stay queued, `submitted` names the first with its error and `parked: false` and the second as not attempted, and the second never reaches the server

#### Scenario: An unknown recipient still parks
- **GIVEN** a queued `submit` intent and an SMTP server answering its `RCPT TO` with `550 5.1.1 User unknown`
- **WHEN** the account syncs
- **THEN** the row is parked with its error

## MODIFIED Requirements

### Requirement: A queued submission is a `submit` queue intent
Neverest SHALL NOT reserve a collection for queued sends. Submission is a **mail** capability: a `submit` intent belongs to a `message/rfc822` account, and an `smtp` channel declared on a source of any other kind SHALL be refused before any connection is made, rather than silently ignored. A submission SHALL be a **queue action** whose kind (`submit`) pimdir defines (STORAGE Annex B.2) and io-pimdir types (`PimdirSubmit`), carrying a versioned JSON payload; which kinds an owner can perform is the owner's business. An owner that does not recognise a kind, or recognises it but lacks the capability, SHALL **skip** the row, leaving it pending, never parking it (parking means permanently unappliable) and never blocking later actions of that collection.

The intent's payload SHALL be `v: 1` JSON, read by io-pimdir's `PimdirSubmit` whatever the order of its keys, carrying `object` (the body hash, required, and the hash the row pins), `from` (empty means the null reverse path), `rcpts` (at least one), and an optional `subject`, `source` and `copy`. A payload `PimdirSubmit` refuses, or one naming another body than the row pins, SHALL park as undecodable. The body SHALL be written durably before the enqueue, so the queue row pins it (queued bodies are pinned, and no GC can sweep it between the enqueue and the send); it belongs to no collection. The intent SHALL anchor on whatever collection the producer chose: neverest scans every collection's pending actions, so there is no anchor rule.

Neverest SHALL perform each pending intent through the one source offering a send channel: its own `smtp` table, else its native send (the Graph `sendMail` action, which files the message in Sent itself), sources walked in the order the account declares them. At most one source may declare an `smtp` table, so the only pick that order decides is between a declared channel and a natively-sending source. On success the row SHALL be acknowledged with its receipt, releasing the body's pin. A **transient** failure, which certainly delivered nothing (an SMTP 4xx, an SMTP 5xx refusing for a sending rate or quota as "A quota refusal keeps the send queued" says, a Graph 401, 408 or 429, a transport error before the message went whole), SHALL leave the row pending; a **permanent** one (any other SMTP 5xx, a Graph 400, 403, 404, 413, 422 or other 4xx, an undecodable payload, a missing body) SHALL park it with its error; an **uncertain** one SHALL park it as "A send that may have been delivered is never sent again" says. A refusal SHALL reset the SMTP transaction before the next message; a refusal for a sending rate or quota, or a failure with no reply or status, SHALL leave the rest of the run's intents unattempted, pending. A build with no send channel (neither `smtp` nor `msgraph`) SHALL skip submit intents and warn, never park them. Message content is never logged.

The SMTP channel SHALL remove the body's `Bcc` field before `DATA` (RFC 5322 §3.6.3): a producer may stage the body with it, the payload's `rcpts` alone carrying those recipients to the server. The Graph channel hands the field to `sendMail`, which derives the recipients from it and does not deliver it.

Submission is **at-most-once** on its own: a message that may have left is parked, never sent again by a run, and only a person, through the producer, sends it again.

An SMTP channel that does not open SHALL be reported: a server that cannot be reached, or a session cut before its credentials were answered, under `unreached` as `<source>.smtp`, its intents pending and unattempted; credentials the server refused, by each queued `submit` failing in `submitted` with the reason, not parked, not attempted. Either makes the run incomplete.

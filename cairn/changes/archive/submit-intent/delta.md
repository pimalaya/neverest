---
cairn: change
change: submit-intent
---

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A queued submission is a `submit` queue intent
Neverest SHALL NOT reserve a collection for queued sends. A submission SHALL be
a **queue action** whose kind (`submit`) is defined by neverest, not by pimdir:
the format carries an action kind and a versioned JSON payload, and which kinds
an owner can perform is the owner's business. An owner that does not recognise a
kind, or recognises it but lacks the capability, SHALL **skip** the row, leaving
it pending, never parking it (parking means permanently unappliable) and never
blocking later actions of that collection.

The intent's body SHALL be written durably before the enqueue and named by the
payload, so the queue row pins it (queued bodies are pinned, so no GC can sweep
it between the enqueue and the send); it belongs to no collection. Its payload SHALL be `v: 1` JSON carrying `object` (the body hash, by the
convention every action kind follows), `from` (empty means the null reverse
path), `rcpts` and an optional `subject`. It SHALL anchor on
whatever collection the producer chose: neverest scans every collection's pending
actions, so there is no anchor rule.

Neverest SHALL perform each pending intent through the first side offering a send
channel: its own `<side>.smtp` table, else its native send (the Graph `sendMail`
action, which files the message in Sent itself), sides walked in configuration
order. On success the row SHALL be acknowledged, releasing the body's pin. A
**transient** failure (an SMTP 4xx, a transport error) SHALL leave the row
pending; a **permanent** one (an SMTP 5xx, an undecodable payload, a missing
body) SHALL park it with its error. A build with no send channel (neither `smtp`
nor `msgraph`) SHALL skip submit intents and warn, never park them. Message
content is never logged.

Submission is **at-least-once**: a crash between the server's acceptance and the
acknowledgement resends on the next run, so deduplication is the receiving
provider's job (`Message-ID`).

## REMOVED Requirements

---
cairn: change
id: a-refusal-carries-its-code
status: landed
created: 2026-11-14
---

# A refusal carries its code

## Why

Himalaya landed "a result carries its errors" on 2026-11-14 (its change of that name, on pimalaya-cli's of the same name): the error of one item prints as pimalaya-cli's item error `{id, code, error, sources}`, a shared noun of every binary (pimalaya-cli spec output, "Shared nouns keep one shape"), and `permission-denied`, `quota-exceeded`, `rate-limited` and `recipient-refused` joined the family's code vocabulary.

neverest's report already lists what each item cost a run: the writes a side refused, the sends and calendar intents that failed, the queue actions parked, the hunks that failed and the endpoints left unreached. Each row carried one free-text `error` (a `reason` for a refused write) holding the whole chain, and no code. A frontend telling a full mailbox from a read-only address book, or a refused recipient from a server down, parsed that text, as MOA did to tell "offline" from "one folder unreadable" before `unreached` existed.

## What

### The codes

Four codes join neverest's vocabulary, spelled as Himalaya's:

| Code | A row, or an account, when |
|---|---|
| `permission-denied` | an HTTP 403 that is no rate limit (Graph, Gmail, Google Calendar, Google People); a DAV 403 whose body names no precondition, or `need-privileges` (RFC 3744 §7.1.1); an IMAP completion naming `NOPERM` (RFC 5530 §3) |
| `quota-exceeded` | an HTTP 507; a DAV body naming `quota-not-exceeded` (RFC 4331 §6); Graph's `ErrorQuotaExceeded`; an IMAP completion naming `OVERQUOTA` (RFC 5530 §3); an SMTP 5yz reply naming a sending limit, as "A quota refusal keeps the send queued" reads it |
| `rate-limited` | an HTTP 429, or a Google 403 naming a rate limit or a quota, still answered after the waits neverest takes |
| `recipient-refused` | an SMTP `RCPT TO` refused (RFC 5321 §3.3), io-smtp offering none after the first |

A refusal none of these name carries none, its reason kept in `sources`.

### The rows

Every report row naming what a run could not do to one item or one endpoint gains pimalaya-cli's item error keys beside its own: `code`, `error` the outermost message, and `sources` the messages of its causes, outermost first. These are the collection and item patch entries, `rejected` (whose `reason` becomes `error`), `submitted`, `intents`, `parked` and `unreached` in `sync`, and `parked` in `drain`. A row that succeeded carries `code` and `error` null and `sources` empty. The text output prints the message and its causes as one chain, unchanged.

An account's row (`sync`, `drain`, `init`) and the index refresh keep the shape of a check row, `code` beside an `error` holding the whole chain, and gain the refusal codes: `init` already prints the endpoints it opened as `sources`. The index refresh gains `code` (the store's, `index-version` or `index-fts5-missing`).

### The readers

One reader per protocol, in its client module, codes the typed error it knows; src/error.rs walks a failure's chain through them once a row is built (`Failure::of`), after any coded error. The typed errors stay in the chain the clients return, so `is_duplicate_uid`, `Gone` and the others read them as before.

pimalaya-cli moves to 0be1b1f, the commit Himalaya pins.

## Decisions

- **Flattened, not nested.** The maintainer chose on 2026-11-14 that a row keeps its own keys and gains `code` and `sources` beside `error`, rather than nesting an item error under `error`: a row then holds the shared noun's keys plus its context (side, collection, action).
- **Account rows keep the check row's shape.** `init` already names its opened endpoints `sources`, and an account's failure is a unit's, as a backend check row is. Their `code` is read as a row's is, a server's refusal included.
- **Coded at report time.** Wrapping each client's error in a coded one would have hidden the typed errors the driver reads back out of the chain; the readers code the chain as it stands.
- **A DAV 403 naming another precondition is no matter of rights.** CalDAV and CardDAV answer most refused bodies 403 with a precondition (RFC 4791 §5.3.2, RFC 6352 §6.3.2); only `need-privileges` says the account may not.
- **Every Google window is `rate-limited`.** A burst, a per-minute and a daily refusal all ask to wait; a 5xx is coded none.
- **A sending limit wins over a refused recipient.** A 5yz reply naming a sending limit (`550 5.4.5`) is `quota-exceeded` wherever it falls, a `RCPT TO` included, the send kept queued. Any other refused `RCPT TO` is `recipient-refused`, a 4yz included, as Himalaya codes it.
- **A parked queue row is uncoded.** The store keeps its error as text alone (pimdir STORAGE §13).
- **No `message-unreadable`, `mailbox-unreadable` nor `message-not-found`.** neverest fails a listing whole on a line that does not decode ("An IMAP listing is read whole or fails") and reads a message gone from the server as gone, not as a failure.
- **No new exit status.** The rows already decide 3 and 4.

## Scope / non-goals

- The `check` rows keep `{backend, ok, code, error}`, the shared noun.
- A send not attempted (a channel stopped or held) carries no code: its reason is neverest's.
- A credential refusal (SMTP 535, a 401) carries none: the vocabulary has no code for it yet.
- io-imap's partial FETCH (Himalaya's pass 2) is not taken: neverest stays on the io-imap 0.10 release, which fails such a line whole.

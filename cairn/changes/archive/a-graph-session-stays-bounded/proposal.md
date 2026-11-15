---
cairn: change
id: a-graph-session-stays-bounded
status: landed
created: 2026-10-13
---

# A Graph session survives idle connections and stays bounded

## Why

The Android client met these on real Microsoft 365 boxes (`mail-sync-robustness`, `body-loss-races`, `write-once-and-oauth-dos`, `5a44a09`, `3906474`), and neverest shares the shape:

- **Dead keep-alive.** Graph closes an HTTP/1.1 connection it left idle after about two minutes. A Graph session only followed the server's keep-alive hint: a request on a connection closed while it idled failed the request (a listing, a body batch), and a read that timed out left the connection in use, so a late answer could be read as the next request's.
- **Writes on items gone.** A flag set, a move or a delete on a message already deleted or moved elsewhere answered 404, failed its hunk and came back every run, the store never converging. Graph contacts and events deletes the same.
- **Memory on hydration.** A body batch asked for twenty messages whatever their size: the whole answer, its parsed JSON with base64 strings, and every decoded body of every retry round sat in memory before the first was written, up to io-http's 256 MiB cap a connection.
- **Memory on listing.** A delta was collected whole (every row of every page, as `MsgraphMessage`, about a kilobyte each before its strings) before it was read, and the per-run row cache kept the full summary of every message a run listed, for the `Meta` tier and the pass, a whole-folder round of 100,000 mails keeping 100,000 summaries to the end of the run.
- **Throttling inside a batch.** The throttled gets of a batch all waited the longest `Retry-After` any of them stated, and a stated wait past two minutes was waited two minutes then tried again, where a single request gives the source up at once.

## What

As agreed for this wave, the design the Android bridge runs:

- **Connection** (`src/msgraph/client/wire.rs`): the stream is watched, recording what each exchange wrote and whether any octet of its answer came. A connection is reused only after a whole answer the server neither ended nor asked to close; one idle past 60 s is reopened before the next request; any other failure (a timeout, a reset, a body cut short) drops it. A request that only reads (a `GET`, a `$batch` of gets), met by a reused connection already dead before any octet of an answer (reset, aborted, broken pipe, end of stream), is sent again once on a new connection (RFC 9112 §9.3.1). A read that timed out is never sent again, nor a request that creates, sends or moves.
- **Items gone converge.** A flag set or a delete answered 404 counts as done (debug log). A move answered 404 is told from a destination Graph no longer knows by reading the message by id: gone, the move counts as done; still there, the 404 is the destination's and stays an error (the destination is resolved from the folder listing first, so this is a folder removed meanwhile). Graph contacts and events deletes: a 404 on the delete or on the read checking the revision counts as done. A copy of a message gone stays an error.
- **Bodies bounded in octets.** The listings ask Graph for the MAPI message size (`$expand=singleValueExtendedProperties($filter=id eq 'Integer 0x0E08')`, io-msgraph `fa73f04`): the band listing, the summaries read by id, and the whole-folder delta (Graph documents `$expand` on message delta; refused once with a 400, the session asks without it). The size reaches the summary and a map shared by the source's connections (`SizeHints`, 100,000 entries at most, each taken once). Bodies are planned in batches of at most 20 gets and 4 MiB stated; a message over 1 MiB, or of no stated size, is read alone by a plain get. When more than one message of a fetch has no stated size (bodies of a run interrupted before, a link made before this change), their sizes are read first, 20 a batch, with the id alone. Each body is written to the store as soon as it is decoded, its JSON dropped before the next is read; nothing accumulates across retry rounds. A debug line reports, per batch, the stated octets, the base64 octets the answer held, the decoded octets and the largest body.
- **Listings hold ids.** A delta and the pass read each page down as it comes: per message, its id and a few octets (removed, out of scope, its flags as three bits, or, on a summary link, the summary of a message the store does not bind, which the page needs anyway), the last row of each id kept, in order. A member the store binds is listed by its flags on every link. The per-run row cache is gone: a band round that goes on with a pass keeps the flags of what it listed, three bits a message, dropped by the pass, which lists again only the members whose flags moved since the band (by one summary read), the round having listed the others already (pimdir SYNC §4). The `Meta` tier reads each summary by id.
- **Throttling inside a batch.** Each throttled get waits its own `Retry-After` (neverest's parser, seconds or an HTTP date), the others going in the next batch at once; past four times, or on a stated wait over two minutes, it is left for the next run and a throttle (429, 503) gives the source up until its wait is over, which stops every connection of the source.

## Not done

- Not run against Microsoft 365 here; the live suite (`tests/msgraph.rs`) needs a tenant. The delta `$expand` follows Graph's documentation and falls back when refused.
- The answer of one batch is still held whole while io-msgraph parses it (its body and its JSON, about 2.3 times the stated octets, so about 10 MiB a connection at the 4 MiB budget); streaming it would take a streaming parser in io-msgraph.
- A single raw get is still buffered whole by io-http (bounded by its 256 MiB cap); streaming it into the blob store would take a streaming body in io-msgraph.
- A message whose MAPI size understates its MIME size is batched by the smaller figure; the MAPI size is close to the MIME size in practice.

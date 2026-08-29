---
cairn: log
change: a-settled-body-is-a-body-of-its-item
landed: 2026-08-29
---

# A settled body is a body of its item

Found by the end-to-end conflict campaign against a real CardDAV account. `conflict resolve --interactive` took whatever bytes the merger left: `Merger::run` reads a decision as "the output file differs from the empty bytes it was seeded with", and nothing after that looked at them. `Conflict::apply` wrote them into the blob tree and staged an `update`, asking the kind for a summary on the way past and ignoring what that parse could not find. A merger writing `this is not a card at all` and exiting zero settled the conflict and stored 26 bytes under a row the store still addressed as `nvt-delta`, with an empty `fn` and no `uid` in its summary. What stopped it spreading was the server, which answered the push with `403 Resource is not a vCard object`; a server that stores what it is given would have kept it, and the store's own copy, which is what a frontend reads, was already gone.

The body is now read before it reaches the blob tree, and refused unless it is a body of the collection's kind and of that item. Two questions, both cheap: does it open and close with the kind's component delimiters, and does it state the `UID` the item is bound by. Either answer being no leaves the divergence exactly as an aborted merger leaves it, with an error naming what is wrong. Mail is refused outright, its bodies being immutable.

The reading is the kind's own scanner rather than vcard-rs and ical-rs, deliberately. Those live behind the `merge` cargo feature, and a build without it is the build where an interactive resolution is the *only* way anything is ever settled, so it is the one that can least afford a weaker guard. The identity check alone would have caught the reported body and has a hole the delimiters close: a card carrying no `UID` is keyed by a digest of itself, so both sides state no identity and any bytes at all would pass.

One test fixture was wrong and was fixed rather than worked around: it seeded cards linked by `uid:a` whose bodies stated no `UID` at all, a state no sync can produce.

Capabilities moved: sync, one new requirement on the resolution.

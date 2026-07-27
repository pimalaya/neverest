---
cairn: change
id: side-owned-send-channel
status: landed
created: 2026-08-07
---

# The send channel belongs to the side, not to the account

## Why

`smtp` sat at the account root, beside `left` and `right`, which reads as if it
belonged to both sides or to neither:

```toml
[accounts.example]
left.imap.server = "…"
right.imap.server = "…"
smtp.server = "…"     # whose?
```

It is neither. A submission server is a property of one provider, the same
provider the side already names: Fastmail's SMTP completes Fastmail's IMAP, not
the mailbox on the other end of the mirror. The account root also makes the
capability look protocol-independent when it is exactly the opposite: whether a
side needs an SMTP table at all depends on its backend. Microsoft Graph already
sends by itself through `sendMail`, and JMAP will send by itself the day its
submission lands, at which point a root `smtp` would be actively misleading:
the JMAP side would send natively while a root table suggests it does not.

Putting the channel in the side makes the shape say the true thing: a side is a
mailbox provider, and a provider either submits by itself or needs a companion
server.

## What (design)

- `SideConfig` becomes a table pairing the backend with its channel:
  `<side>.<backend>.*` (the existing enum, now `SideBackendConfig`, flattened
  in) plus an optional `<side>.smtp` block. `AccountConfig.smtp` is removed.
- Channel resolution walks the sides in configuration order and takes the first
  that offers one: its own `smtp` table, else its native send (Graph today).
  `left` therefore wins on an account where both sides could send, which is a
  statable rule rather than the previous implicit "the root table beats every
  side".
- The wizard writes the discovered submission endpoint into the side it was
  discovered with, so the IMAP + SMTP pair that discovery returns as one
  service lands as one side. `neverest configure` carries over the channel the
  side already had when a re-run discovers none.
- Deserialization stays strict enough: a mistyped backend key ("imapp") is
  refused, since no variant matches the flattened data.

## Cost

This is a breaking config change, and it lands before 1.0.0 rather than after.
A config keeping `smtp` at the root now fails to parse with `unknown field
`smtp``, which is loud, and the fix is a one-line move under `left`.

## Out of scope

- JMAP submission itself: this only makes room for it. A JMAP side will send
  natively (`sends_natively`) once the backend lands, and will simply never
  carry an `smtp` table.
- Per-side outboxes. The `Outbox` collection stays account-level (one store,
  one queue); only the channel draining it moved.

---
cairn: log
change: a-resource-has-one-spelling
date: 2026-10-04
---

# A resource has one spelling

MOA saw a card whose `UID` holds a `:` (cardamum's default `urn:uuid:`) duplicated in the store after a sync. neverest created it as `urn:uuid:x.vcf`, Stalwart listed it as `urn%3Auuid%3Ax.vcf`, and the two names read as two resources. Decoding the listed name was not the answer: io-webdav reading `urn:uuid:x.vcf` back got a 404, a first segment with a colon reading as a scheme (RFC 3986 §4.2).

## What landed

- src/dav/client.rs: `canonical_segment` gives a member one spelling (unreserved octets decoded, `:` encoded as `%3A`, escapes upper-cased, any other escape kept); `href_id` reads hrefs through it, and `sanitize` names a new resource in it.
- Checked against two Stalwart servers, a card copied from one address book to the other: the run before the change re-copied it on every run; after it, the second and third runs copy nothing and the target holds one card.

## Capabilities moved

- sync: a resource has one spelling.

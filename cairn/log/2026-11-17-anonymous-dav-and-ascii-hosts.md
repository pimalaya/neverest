---
cairn: log
change: anonymous-dav-and-ascii-hosts
date: 2026-11-17
---

# Anonymous DAV and ASCII hosts

Finding COH-6, its two rulings, made by the owner on 2026-11-17 for the whole family.

## What landed

- A `carddav` or `caldav` table naming no `auth` loads as `auth = "none"`, where it failed with "missing field `auth`". A 401 met by such a session, discovering its home set or listing its collections, says the account configures no `auth`.
- `idna_adapter` is pinned to 1.0.0, as in himalaya, cardamum, calendula and ortie: no Unicode IDNA back end, and 21 crates (ICU4X and its support) left Cargo.lock.
- A configured host that is not ASCII, in any `server` or a DAV `home`, is refused at load naming its Punycode form, on every scheme: url keeps an `imaps` or `smtps` host opaque, which then failed at DNS, while an `https` one went through IDNA.

## Spec

sync.md: the DAV home set requirement.

---
cairn: log
change: pool-size-is-graphs-alone
date: 2026-11-17
---

# pool-size is Graph's alone

Finding NEV-7, its `pool-size` part, ruled by the owner on 2026-11-17: move the option where it is read.

## What landed

- `pool-size` left the fields every backend table shares (`source_config!`) and stays on the Graph domain tables (`msgraph.mail`, `msgraph.contact`, `msgraph.calendar`), the only ones that read it. `imap.pool-size`, `gmail.pool-size` and the others now fail to load as unknown keys, where they loaded and nothing read them.
- config.sample.toml dropped `#imap.pool-size = 8`, which told IMAP users to narrow a pool that `connections` bounds, and names `pool-size` among the keys a `msgraph.mail` table takes.

## Spec

sync.md, the connection budget paragraph.

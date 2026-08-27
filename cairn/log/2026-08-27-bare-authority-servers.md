---
cairn: log
change: bare-authority-servers
date: 2026-08-27
---

# One resolver for every `server`

`carddav.server = "posteo.de:8843"` failed with io-webdav's `WebDAV URL
"posteo.de:8843" has no host`. The error arriving from a backend rather than
from the configuration is the whole diagnosis: neverest had accepted the value
and passed on a URL with no host in it.

A bare authority is not a relative URL. `url` reads `posteo.de:8843` as the
scheme `posteo.de` with the path `8843`, which parses cleanly. The IMAP
resolution decided when to prepend a scheme by matching
`ParseError::RelativeUrlWithoutBase`, so it caught `imap.example.com` and missed
`imap.example.com:143`, a spelling config.sample.toml documents as supported.
CardDAV required a full URL outright. Three backends, three degrees of
flexibility, one wrong shared assumption.

`config::server_url(server, scheme)` now resolves all three, splitting on `://`
rather than on a parse error: a value carrying it is parsed verbatim, so an
explicit `http://` or a non-default port survives, and a value without it takes
the backend's default scheme (`imaps`, `smtps`, `https`). The IMAP path loses
its `RelativeUrlWithoutBase` arm and the CardDAV path its full-URL contract.

No default-port table came with it. `url` knows the `http` and `https` ports,
and the two protocols needing one for the OAUTHBEARER GS2 header ask their own
backend crate through `default_port`, which is where the scheme table lives.

Capabilities moved: **sync** (server resolution, shared across the backends).

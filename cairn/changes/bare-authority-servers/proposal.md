---
cairn: change
id: bare-authority-servers
status: landed
created: 2026-08-27
---

# One resolver for every `server`, so a bare authority works everywhere

## Why

`carddav.server = "posteo.de:8843"` failed with `WebDAV URL "posteo.de:8843" has no host`, and the error came from io-webdav rather than from neverest, which is the tell: the value reached a backend as a URL neverest had already accepted.

A bare authority is **not** a relative URL. `url` reads `posteo.de:8843` as the scheme `posteo.de` with the path `8843`: it parses cleanly and carries no host. So the IMAP resolution, which matched on `ParseError::RelativeUrlWithoutBase` to decide when to prepend a scheme, only ever caught the *portless* spelling. `imap.example.com:143` took the same path and was handed to io-imap hostless, and config.sample.toml documents that spelling as supported. The CardDAV side did not try at all, requiring a full URL by contract.

Three backends, three different degrees of flexibility, and the one shared assumption behind them was wrong. The user reaching for a non-default port is exactly the user who has no reason to know any of this.

## What

One `server_url(server, scheme)` in the config module, used by every backend that resolves one: IMAP (`imaps`), SMTP (`smtps`) and CardDAV (`https`).

The presence of `://` decides. A value carrying it is a full URL and is parsed verbatim, so an explicit `http://` or a non-default port survives; a value without it is an authority and takes the caller's default scheme, port and all. Nothing sniffs for a leading hostname or a trailing port, because there is nothing reliable to sniff: the two forms are distinguished by the separator, not by their shape.

## Not in scope

No default-port table. `url` already knows the ports for `http` and `https`, and the two protocols that need one for something other than connecting (the OAUTHBEARER GS2 header) ask their own backend crate through `default_port`, which is where the scheme table belongs.

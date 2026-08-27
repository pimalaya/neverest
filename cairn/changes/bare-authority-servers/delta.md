---
cairn: change
change: bare-authority-servers
---

# Delta

## ADDED Requirements

### Requirement: A `server` is an authority or a URL, resolved at one seam
Every backend's `server` SHALL accept either a bare authority, with or without a
port, or a full URL, and both SHALL resolve through one shared function rather
than per backend. The scheme a bare authority takes is the backend's own:
`imaps` for IMAP, `smtps` for SMTP, `https` for a DAV entry point.

The presence of `://` SHALL be what tells the two forms apart. A value carrying
it SHALL be parsed verbatim, so an explicit cleartext scheme or a non-default
port survives; a value without it SHALL take the default scheme. Resolution
SHALL NOT be decided by a parse error: a bare authority carrying a port parses
as a URL whose scheme is the hostname and whose path is the port, so it reports
no error and carries no host, and a backend handed one rejects it for a reason
that names neither the value the user wrote nor the field it came from.

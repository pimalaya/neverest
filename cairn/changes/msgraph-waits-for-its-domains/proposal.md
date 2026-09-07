---
cairn: change
id: msgraph-waits-for-its-domains
status: landed
created: 2026-09-07
---

# Graph does not ship syncing one domain of three

## Why

Microsoft Graph carries mail, contacts and calendar behind one protocol, one host and one credential. The `msgraph` backend syncs mail and nothing else, and there is no way to ask it for anything else: `Client::media_type` answers `message/rfc822` as a constant, the adapter drives `mail_folders_list` and `messages_delta`, and `MsgraphConfig` carries no field naming a domain.

That reads as a misconfiguration to whoever configures a Graph account and gets mail alone. It is not one, and no error says so, because from the sync's point of view nothing went wrong.

Releasing it that way sets the expectation that `msgraph` means mail, which the domain axis then has to break. Holding it out of the default set for one release costs the few people who want Graph mail a `--features msgraph`, and costs everyone else nothing.

## What

`msgraph` leaves the default feature set. Nothing else moves: the feature still exists, still compiles, is still covered by the tests and the clippy runs, and every `msgraph` source config still parses in every build. A source declaring one is refused when the sync opens it, which is what an uncompiled backend has always done.

The wizard needs no change, `wizard::search` already gating the offer on `cfg!(feature = "msgraph")`, so Graph simply stops being proposed.

It comes back with its domain axis, under `a-graph-source-declares-its-domain`.

## Not in scope

**The backend is not removed.** Deleting `src/msgraph/` would throw away the adapter the contacts arm is written against, and the code compiles clean today.

**No new error text.** A source whose backend was not compiled in already reports it at runtime, in the one message every such backend shares.

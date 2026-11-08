---
cairn: delta
change: a-scoped-copy-is-made-once
---

## ADDED Requirements

### Requirement: A copy is made once
A mail create carrying an origin (pimdir SYNC §4) and a `Message-ID` SHALL be delivered only once its target is known not to hold that message: an IMAP source SHALL search the target by `UID SEARCH HEADER Message-ID "<id>"` before uploading it, and a Graph mail source SHALL list the target folder filtered on `internetMessageId` before `message_copy`. A message found there and bound by no item of this side SHALL be bound to the create, nothing filed; any other find delivers the create as before. A create whose identity was minted for a second copy of one `Message-ID` SHALL NOT search, its twin being the message the search would find. A failed search SHALL reject the create, which stays pending.

A copy whose answer was lost stays staged, and a scope hides the server's copy from every listing (a message dated below the floor, a `Date` the server cannot read), so only a search tells the next run it was made. Outside a scope the listing lands the create by its `Message-ID` first (pimdir SYNC §6), and the search finds nothing more; one search a copy is cheap, copies being rare. A staged move whose answer was lost is made once already on IMAP, its source half moving the member server side; on Graph its target half may deliver it as a copy, which this search covers. A server that matches no `HEADER Message-ID` search files the copy again, as before.

#### Scenario: A copy made before its answer was lost
- **GIVEN** a message dated 2020 bound in `A`, a copy of it staged into `B`, and `B` already holding that message on the server, the answer to the copy having been lost
- **WHEN** the account syncs under `item.filter.since = "30d"`
- **THEN** `B` holds the message once and the store binds the staged copy to it

#### Scenario: A move made before its answer was lost
- **GIVEN** a message dated 2020 bound in `A`, a move of it staged into `B`, and the server having moved it already
- **WHEN** the account syncs under `item.filter.since = "30d"`
- **THEN** `B` holds the message once, `A` none, and the store binds it in `B`

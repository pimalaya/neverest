---
cairn: change
id: a-check-lists-collections-with-their-roles
status: landed
created: 2026-10-04
---

# A check lists collections with their roles

## Why

`neverest check` lists each source's collections but reports only how many. A caller adding a mail account needs the list itself, and what the server says each folder is for, to propose which folder is the sent one, the drafts, the trash, the archive.

MOA asks this of himalaya today (`himalaya imap list` for IMAP SPECIAL-USE, `himalaya msgraph mail-folder get` for Graph's well-known folders), which is the only reason its shipped himalaya keeps the `imap` and `msgraph` features. neverest already opens every source at check time: it can answer, and himalaya can ship with `pimdir` alone (MOA `docs/plan/onboarding.md`, step 1). Graph's Drafts is also the one folder Graph creates messages in (`drafts_id`), so a caller must be able to tell which listed folder it is.

## What (design)

- **`check` lists the collections.** Each source's entry carries `collections: [{ id, name, role? }]` instead of a count. Breaking for a JSON reader of the count; nothing is known to read it (MOA does not).
- **`role` is the server's own statement, never guessed from a name.** Kept out of `Collection`, which stays protocol-free: a check-only probe on the client seam (`Client::collection_roles`), answering collection id → role.
  - **IMAP:** the SPECIAL-USE attributes of the `LIST` rows (RFC 6154): `\Sent`, `\Drafts`, `\Trash`, `\Junk`, `\All`, `\Archive`; `INBOX` is always the inbox (RFC 3501 §5.1).
  - **Graph mail:** one `GET mailFolders/<well-known>` per role (`inbox`, `sentitems`, `drafts`, `deleteditems`, `junkemail`, `archive`), the answered id matched to the listed folder; a well-known folder Graph does not have (often `archive`) is skipped.
  - **Gmail API:** the system labels `INBOX`, `SENT`, `DRAFT`, `SPAM`, `TRASH`.
  - **DAV, Google Calendar and People, Graph contacts and calendars:** no roles.
- **Role names:** `inbox`, `sent`, `drafts`, `trash`, `junk`, `all`, `archive`.
- **Nothing written to the store.**

## Out of scope

- JMAP (`Mailbox.role`): JMAP sources parse but do not open yet; the probe answers for it once they do.
- Roles on synced collections (the store has none and should not: only mail needs them).

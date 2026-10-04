---
cairn: change
change: a-check-lists-collections-with-their-roles
---

# Delta

## ADDED Requirements

### Requirement: A check lists collections with their roles
`neverest check` SHALL list, for each endpoint, every collection it answered with: its id, its name and, when the server states one, its role among `inbox`, `sent`, `drafts`, `trash`, `junk`, `all` and `archive`. A role SHALL come from the server alone, never from a collection's name: on IMAP the SPECIAL-USE attributes of the `LIST` rows (RFC 6154) and `INBOX` (RFC 3501 §5.1); on Graph mail the well-known folders, one lookup per role, a well-known folder the mailbox does not have being skipped; on the Gmail API the system labels. Other backends state no role. The check SHALL write nothing to the store, and roles SHALL stay out of the shared collection shape.

Display names are localised (`[Gmail]/Bin`, "Éléments envoyés") and an IMAP server need not name its folders after their use: only the server's statement lets a caller propose the right folder for each use. On Graph, the Drafts folder is the one place a message can be created, so a caller has to know which listed folder it is.

#### Scenario: An IMAP check names the special-use folders
- GIVEN an IMAP account whose server advertises SPECIAL-USE
- WHEN `neverest check --json` runs
- THEN each source lists its collections, `INBOX` with the role `inbox` and the sent, drafts and trash folders with theirs

#### Scenario: A folder without a statement has no role
- GIVEN a folder the server marks with no SPECIAL-USE attribute
- WHEN `neverest check --json` runs
- THEN the folder is listed without a role, whatever its name

## MODIFIED Requirements

None.

## REMOVED Requirements

None.

---
cairn: change
id: an-imap-move-copies-without-move
status: landed
created: 2026-11-21
---

# An IMAP move copies without MOVE

## Why

Issue [#35](https://github.com/pimalaya/neverest/issues/35): OVH (Zimbra) advertises IMAP4rev1 and UIDPLUS but no MOVE. `ImapClient::move_messages` (src/imap/backend.rs:789 at 207eba0) sends `UID MOVE` regardless. OVH answers `BAD`, and after a few refusals it sends `BYE`, so the rest of the run fails on a dead connection (partial sync, 25 errors in the reporter's run).

PR [#36](https://github.com/pimalaya/neverest/pull/36) fixes it but is not taken, see "Closing #36".

## What

`move_messages` keeps its signature and picks its path from the authenticated capabilities:

- MOVE (RFC 6851) or IMAP4rev2 (RFC 9051) advertised: `UID MOVE`, as today.
- Neither, and no `UID EXPUNGE` (UIDPLUS or IMAP4rev2): refused before any command, as `delete_message` refuses a delete; the push rejects and holds the change.
- Otherwise: `UID COPY` into the target, then the `UID STORE +FLAGS (\Deleted)` and `UID EXPUNGE` of `delete_message` on the source UIDs alone, never a plain `EXPUNGE`.

The adapter searches nothing. Before the backend is called, `targets_holding` (src/sync/remote.rs:1647) searches the target for every move with a non-minted Message-ID, and deletes the source instead of moving it when the target holds it (`a_remove_into_a_target_holding_the_identity_deletes`). A fallback interrupted after its COPY is therefore finished by the next run, without a second copy.

## Decision: minted keys move too

A message with a minted key (`alt:` without a Message-ID, `dup:` for a second copy) is not searched by `targets_holding`. Recommendation: move it with the same fallback; an interrupted move can then leave a second copy in the target, never lose one, as an interrupted append already can. The alternative is to refuse it on servers without MOVE (what #36 does), which leaves it unmovable there and reported every run.

Ruling (owner, 2026-11-21): the recommendation is taken.

## Closing #36

Reasons:

- the identity plumbing (src/client.rs, src/sync/remote.rs) and the target search in the adapter repeat `targets_holding`;
- it refuses every move of a message without a searchable Message-ID on servers without MOVE;
- it repeats `delete_message` inline;
- it does not follow the guidelines: AGENTS.md carries the Cairn activation stanza only (GUIDELINES.md cairn-001), a Cairn change is landed in the PR with no proposal agreed first (AI_POLICY.md, Agents), the proposal records the author's own session, and the spec scenarios do not follow the shape of cairn/spec/sync.md.

Comment, once the fix is pushed:

> Thanks @Mickael-Roger for the report and the patch. I fixed it in <commit> instead and close this one: the retry it adds is already handled by `targets_holding` (src/sync/remote.rs), it refuses moves of messages without a Message-ID, and it does not follow the guidelines (the AGENTS.md edit, a Cairn change landed with no proposal agreed first, the spec shape). See the [contributing guide](https://github.com/pimalaya/neverest/blob/master/CONTRIBUTING.md), [GUIDELINES.md](https://github.com/pimalaya/.github/blob/master/GUIDELINES.md) and the [AI policy](https://github.com/pimalaya/.github/blob/master/AI_POLICY.md) for a next one.

Issue #35 stays open until the reporter confirms on OVH (no OVH account here):

> @Mickael-Roger fixed on master in <commit>. Could you try it against your OVH account with a move staged? v1 reads no 0.x store or config, so it needs a fresh one.

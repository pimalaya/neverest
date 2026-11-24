---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A sent copy outside Sent is filed

Task N1 of the-live-night-findings, under the owner's ruling R1 of 2026-11-24: Gmail adds the label, Graph declares `mail.submit.copy` Partial. On the live night of 2026-11-23, `himalaya message compose --send --save <folder>` through google-pimdir and microsoft-pimdir sent the message and never filed the copy, the row reading `applied`: src/sync/driver/send.rs dropped every copy when the provider files sent mail itself, not only a copy into Sent.

`capability::sent_filing` (src/sync/capability.rs) replaces `files_sent_copy` with what the provider files: `Nothing`, `Sent` (an IMAP filing pair, Gmail over SMTP), `SentOnly` (Graph) or `Labels` (Gmail's `messages.send`). `copy_step` (src/sync/driver/send.rs) leaves a copy to the provider only when its collection is the one the store states `sent`; elsewhere Gmail's `messages.send` labels the sent message (`GmailClient::label_message`, on the id `send_raw` now answers, through `SendChannel::label`), falling back to the `add` when the label fails; Graph parks the intent before the send; any other provider takes the `add` as before. `capability::sent_only` declares Graph's `mail.submit.copy` `none` on every folder but Sent Items, the source-wide row reading `partial` with "Sent Items only: ...", so the producer's gate refuses such a `submit` at enqueue. Capability moved: sync.md, "A sent copy is filed once sent".

## Checked

- src/sync/driver/collection_intent_tests.rs `a_copy_outside_sent_is_filed_where_the_provider_files_in_sent`: on the old code (`acknowledge_sent(.., files_copy = true, ..)`) it failed at "the copy outside Sent is filed".
- `a_copy_outside_sent_is_labelled_on_gmail_and_refused_on_graph`, and src/sync/capability.rs `gmail_labels_a_sent_copy_and_graph_files_it_in_sent_items_alone`.
- Live, under the live-suite lock: tests/google.rs `a_gmail_send_files_its_copy_in_another_label` (one message, in SENT and the run's label, no add queued; the message and the label deleted at cleanup) and tests/msgraph.rs `a_graph_send_copied_outside_sent_items_is_refused_at_enqueue` (refused `mail.submit.copy` at enqueue, nothing sent; the folder deleted at cleanup), both passing.

## Left

pimdir STORAGE Annex B.2 (line 843) still says a provider filing sent mail itself supports the copy "as that filing and enqueues nothing"; the wording handed back to the lead for lane F names Sent alone.

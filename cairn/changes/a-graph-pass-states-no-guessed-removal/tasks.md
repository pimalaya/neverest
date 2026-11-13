---
cairn: tasks
change: a-graph-pass-states-no-guessed-removal
---

# Tasks

- [x] Microsoft's documentation read for `$deltatoken=latest` on a mail folder's messages: not documented
- [x] `pass` states only the rows marked removed (src/msgraph/client/mail.rs)
- [x] `a_pass_failing_to_list_a_member_states_no_removal`, seen failing on the old statement
- [x] `an_interrupted_round_resumes_its_band_and_passes_again` changed where it asserted the forbidden statement, and nowhere else
- [x] `a_message_dropped_before_the_pass_goes_with_a_later_round`: the limit and what ends it
- [x] Spec (the requirement that mandated the statement, the limit), CHANGELOG, log
- [ ] The stale row of a message dropped between its band page and the pass: the owner's, three ways named in the log

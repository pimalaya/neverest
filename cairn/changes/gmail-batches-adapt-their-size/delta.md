---
cairn: change
change: gmail-batches-adapt-their-size
---

# Delta

## MODIFIED Requirements

### Requirement: Gmail reads in paced batches
A `gmail` source SHALL read message metadata, sizes and raw bodies through Gmail's HTTP batch endpoint, at most `gmail.batch-size` reads a batch (10 by default, 1 to 100) and at most `gmail.batches-in-flight` batches at once across the source's connections (1 by default), each batch paced by the units of all its reads. The size SHALL adapt: a batch with a read refused for concurrency or rate, failed transiently or left unanswered SHALL cut the size to half the reads it carried, never raising it and never below one read, and a full batch sent at the size in force and answered with no refusal SHALL double it, never above `gmail.batch-size`; a per-minute quota refusal SHALL leave it as it is. The source's connections SHALL share the size for the run, a later read starting where the last one left it. A read refused inside a batch for concurrency, rate or a missing answer SHALL be sent again at the head of the next batch after one to two seconds, one refused a per-minute quota likewise after two to five seconds and then the next minute; a read answered 404 SHALL be a message gone, never an error, and a body read so SHALL be answered gone to the driver; a daily quota, or a read refused six times, SHALL give the source up. A raw batch SHALL hold about 8 MiB of messages by their sizes, a message over 5 MiB being read alone, and each body SHALL be stored as its batch lands. The sizes SHALL be the run's own reads, else the ones the store records for the bodies it owes, a minimal read asking Gmail only for the sizes neither knows.

A run SHALL read a message once: a message carrying several synced labels SHALL be named, under the second label listing it, from the metadata the first read, its flags from that label's own flag listings. The run SHALL read the mailbox history once, with no label, and project it on each label.

#### Scenario: Ten reads a batch, the refused one put back
- **GIVEN** a Gmail label of 45 unbound messages, one refused for concurrency once
- **WHEN** its round page is read
- **THEN** six batches are sent one after another, 10, 5, 10, 10, 10 and 1 reads, the refused message first in the second, halved by the refusal and doubled back after it

#### Scenario: Refusals shrink the batches until one lands
- **GIVEN** a Gmail label of 20 unbound messages, the first refused for concurrency three times
- **WHEN** its round page is read
- **THEN** the batches carry 10, 5, 2, 1, 2 and 3 reads, and the size left for the run is 4

#### Scenario: A minute refusal leaves the size alone
- **GIVEN** a Gmail label of 25 unbound messages, the first refused once for concurrency and the eleventh once for the per-minute quota
- **WHEN** its round page is read
- **THEN** the batches carry 10, 5, 5 and 7 reads

#### Scenario: Batches sent before a refusal land after it
- **GIVEN** two connections that each built a batch of 10 before either was answered
- **WHEN** both are refused
- **THEN** the size is 5, not 2, and a batch of 10 answered later leaves it at 5

#### Scenario: A message under two labels
- **GIVEN** a message filed under `INBOX` and a user label
- **WHEN** both are listed in one run
- **THEN** its metadata is read once

#### Scenario: A resumed hydration
- **GIVEN** bodies an interrupted run left owed, their sizes in the store
- **WHEN** a run downloads them
- **THEN** raw batches read them with no minimal read first

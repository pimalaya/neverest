---
cairn: change
change: a-settled-body-is-a-body-of-its-item
---

# Delta

## ADDED Requirements

### Requirement: A settled body is a body of its item
A body a resolution settles on SHALL be read before it is staged, and refused
unless it is a body of the collection's kind and of that item. It SHALL open and
close with the kind's component delimiters (`VCARD`, `VCALENDAR`), and it SHALL
state the identity the item is bound by: a body stating another `UID`, or none
where the item states one, SHALL be refused. Mail SHALL be refused outright, its
bodies being immutable.

A refusal SHALL leave the divergence exactly as it was, which is what an aborted
merger already does.

The three bodies a run merges by itself are the store's own, and the merge
refuses a side no parser reads (`Merged::Unmergeable`). A settled body is the one
body reaching the store that nothing derived: a merger that crashed after a
partial write, a template saved half-finished and a tool writing its error
message to the output path all produce bytes that are not a contact, and the item
keeps its link id while losing every field that identity came from.

#### Scenario: A merger writes something that is not a card
- GIVEN an interactive resolution whose merger writes bytes no parser reads and exits zero
- WHEN the decision is applied
- THEN it is refused naming the delimiters, nothing is staged, and the conflict is still parked

#### Scenario: A resolution may not rename the item
- GIVEN a settled body that reads as a card but states another `UID`
- WHEN the decision is applied
- THEN it is refused naming both identities

---
cairn: delta
change: account-ls-and-the-family-abouts
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: The account group is the family's
`account list` SHALL take `ls` as its visible alias, as in every family binary.

#### Scenario: Listing with `ls`
- GIVEN a configuration holding an account
- WHEN `neverest account ls` runs
- THEN it prints what `account list` prints and exits 0, and `account -h` lists `list` with `[alias: ls]`

## REMOVED Requirements

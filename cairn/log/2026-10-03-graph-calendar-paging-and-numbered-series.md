---
cairn: log
change: graph-calendar-paging-and-numbered-series
date: 2026-10-03
---

# Graph calendar paging and numbered series

Two Graph calendar bugs reported on calendula (pimalaya/calendula#10, #12) sat in neverest's copy of the same code. Graph fills the `endDate` of a `numbered` range with `0001-01-01`, so a numbered series' instances window ended before its start and Graph refused the read. Graph also returns the last event of one `@odata.nextLink` page again as the first of the next, so the enumeration listed one id twice, the input the duplicate link id data loss feeds on.

## What landed

- io-msgraph 0.4.5: `MsgraphPatternedRecurrence::bounds`, the dates a series spans by its range type.
- src/msgraph/client/calendar.rs: the series window reads `bounds`; the enumeration and the instances listing drop an event repeated at the same `changeKey` and fail on one id at two.

## Capabilities moved

- sync: the `msgraph-calendar` enumeration and series window.

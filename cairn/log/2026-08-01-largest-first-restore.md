---
cairn: log
change: largest-first-restore
landed: 2026-08-01
---

# Restore largest-first hydration order (from store meta, no probe)

Batching the body fetch had switched hydration from largest-first to UID order.
That made the progress bar linear — which felt slower and, worse, froze near the
end (~97%) when a large message landed at a high UID: the tail stalled on one big
body. Largest-first front-loads the heavy messages, so the counter crawls at the
start and accelerates to a smooth finish, and the tail is all fast small bodies.

The original largest-first paid a size probe (`UID FETCH … RFC822.SIZE`), which
is why batching dropped it. But the sizes are already local — the `Meta` tier
wrote each envelope's `RFC822.SIZE` into the store meta before hydration. So the
driver now reads each not-yet-`Full` item's size from its store meta while
collecting the hydration handles (no round trip) and passes a `handle → size` map
into the `Full` fetch. `fetch_full` orders largest-first when sizes are present,
falling back to UID order when not (the two-source cross-copy path passes an empty
map). Batches are chunked from the ordered handles and work-stolen, so the biggest
batches start first.

Verified live (Stalwart): with a big message at UID 3 among small UIDs 1–5, the
batched FETCH command begins with UID 3 — largest-first confirmed — and bodies
download correctly. No size probe reintroduced.

Spec updated: `sync` (MODIFIED "Hydration may run concurrently, largest-first":
largest-first order restored, sized from the store meta rather than a probe).

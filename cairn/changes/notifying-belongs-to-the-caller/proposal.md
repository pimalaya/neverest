---
cairn: change
id: notifying-belongs-to-the-caller
status: landed
created: 2026-08-29
---

# Neverest carried a notification daemon it could not use well

## Why

A run could raise a desktop notification when an item entered conflict, opt-in through `conflict.notify`. It is the wrong job for this program, and the report already does it better.

The once-only rule is what looked like it needed building in: a five-minute schedule over one unresolved card must raise one notification, not three hundred a day. That rule rests on the engine saying nothing about a placement an earlier run already parked, and the report states that distinction outright. `conflicts` is what this run marked, item by item; `outstanding_conflicts` is what the store holds waiting. A caller reading `--json` notifies on entry by testing the first, once, with no state of its own to keep. The hard part was already data.

What was built in is worse than what a caller can write. The notification adapter deliberately carries no template, since which variables exist is the caller's business, so `conflict.notify` could only ever show a fixed summary and body: it could not name the card, its collection or the side it diverged on. Three lines of shell reading the report can.

The exit code cannot carry it either, and that is worth stating so nobody reaches for it: exit 2 means the run left something waiting, which is a parked conflict, a duplicate `UID` the other side refuses, or a write it would not take. A wrapper keying on the status alone would announce the wrong thing.

Against that, the cost was real: a C library on the link line, `dbus` and an `LD_LIBRARY_PATH` entry in the devshell, `dbus` plus an `NIX_LDFLAGS` rpath and an aarch64 `-mno-outline-atomics` override in the package, a cargo feature, a config key, a serde newtype over a foreign type, and a failure mode when the daemon is absent. Neverest is a headless sync tool, usually under cron or systemd, where a desktop bus is often not even reachable.

Comodoro keeps its notification, and the contrast is the argument: a timer is desktop-interactive and the notification is the product. A sync run is not.

## What

- The `notify` cargo feature goes, along with `pimalaya-config/notify`.
- `conflict.notify` and the `ConflictNotification` newtype go. `conflict.merger` stays: that one is neverest's job, invoked from a command rather than a run.
- `announce_conflicts` becomes `warn_conflicts`, keeping the log line per item and the once-only count, dropping only the daemon call.
- `dbus` leaves shell.nix and package.nix, taking the rpath workaround and the aarch64 atomics override with it.
- The README and the sample configuration carry the replacement recipe rather than implying one.

The behaviour a person sees is unchanged unless they had set `conflict.notify`, which no release offers: the key exists only under `[Unreleased]`.

---
cairn: log
change: notifying-belongs-to-the-caller
landed: 2026-08-29
---

# Notifying belongs to the caller

A run could raise a desktop notification when an item entered conflict, opt-in through `conflict.notify`. It is the wrong job for a headless sync tool, and the report already did it better.

## What landed

The `notify` cargo feature is gone, with `pimalaya-config/notify` behind it. `conflict.notify` and the `ConflictNotification` newtype are gone; `conflict.merger` stays, that one being invoked from a command rather than a run. `announce_conflicts` is now `warn_conflicts`, keeping the log line per item and the once-only count and dropping only the daemon call. `dbus` left shell.nix and package.nix, taking with it an `LD_LIBRARY_PATH` entry, an `NIX_LDFLAGS` rpath and an aarch64 `-mno-outline-atomics` override that existed because dbus calls libgcc outline atomics a static aarch64 link cannot resolve.

## Why the built-in one could not earn its keep

The once-only rule looked like the thing that needed building in, and it was already data. It rests on the engine saying nothing about a placement an earlier run parked, and the report states that distinction outright: `conflicts` is what this run marked, item by item, and `outstanding_conflicts` is what the store holds waiting. A caller reading the JSON report notifies on entry by testing the first, once, with no state of its own to keep.

What was built in was also worse than what a caller can write. The notification adapter deliberately carries no template, since which variables exist is the caller's business, so the key could only ever show a fixed summary and body. It could not name the card, its collection or the side it diverged on. Three lines of shell reading the report can, and the README and the sample configuration now carry that recipe rather than implying one.

The exit code is not the signal either, and the spec now says so: exit 2 means the run left something waiting, which a parked conflict, a refused duplicate `UID` and a rejected write all satisfy. A wrapper keying on the status alone would announce the wrong thing.

## Not changed

Nothing a person sees moves unless they had set `conflict.notify`, which no release offers: the key existed only under `[Unreleased]`. The once-only rule itself is untouched and still pinned by its tests, now through the report rather than through a daemon call.

Comodoro keeps its notification, and the contrast is the argument: a timer is desktop-interactive and the notification is the product. A sync run is not.

## Capabilities moved

- **sync**: "Entering a conflict notifies once" became "Entering a conflict is said once", a property of the report and the log rather than of a notification, and the exit code is explicitly not that signal.

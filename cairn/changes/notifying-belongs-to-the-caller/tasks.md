---
cairn: tasks
change: notifying-belongs-to-the-caller
---

- [x] Drop the `notify` cargo feature and the `pimalaya-config/notify` dependency
- [x] Remove `conflict.notify` and `ConflictNotification`, keeping `conflict.merger`
- [x] Reduce `announce_conflicts` to `warn_conflicts`, keeping the log and the count
- [x] Remove dbus, the rpath and the aarch64 override from shell.nix and package.nix
- [x] Keep `buildFeatures` reaching the derivation once its notify arm is gone
- [x] Document the `--json` recipe in the README and the sample configuration
- [x] Verify the once-only tests still pin the rule through the report

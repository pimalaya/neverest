---
cairn: log
change: the-installer-fails-on-http-errors
landed: 2026-11-18
---

# The installer fails on an HTTP error

install.sh downloads with `curl -fsSLo`: an HTTP error stops the script where it used to save the error page as the archive and let `tar` fail on it. Owner ruling of 2026-11-18 for every family installer, carillon ba9f13b the model; checksums wait on pimalaya/nix publishing them.

No capability moved: packaging only.

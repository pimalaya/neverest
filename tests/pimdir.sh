#!/usr/bin/env bash
# Install the `pimdir` operator CLI, built from the io-pimdir commit or
# version Cargo.lock locks, so it reads a store the way neverest writes it.
# The suites never spawn it: it is for inspecting a store by hand.
#
# The binary lands in target/pimdir/<commit or version>/bin/pimdir; a lock
# moving to another io-pimdir asks for this script again.

set -eu

cd "$(dirname "$0")/.."

source=$(awk '
    /^name = "io-pimdir"$/ { found = 1; next }
    found && /^source = / { gsub(/^source = "|"$/, ""); print; exit }
    found && /^\[\[package\]\]$/ { exit }
' Cargo.lock)

case "$source" in
git+*)
    url="${source#git+}"
    url="${url%%\?*}"
    url="${url%%#*}"
    rev="${source##*#}"
    root="target/pimdir/$rev"
    set -- --git "$url" --rev "$rev"
    ;;
registry+*)
    version=$(awk '
        /^name = "io-pimdir"$/ { found = 1; next }
        found && /^version = / { gsub(/^version = "|"$/, ""); print; exit }
    ' Cargo.lock)
    root="target/pimdir/$version"
    set -- io-pimdir --version "=$version"
    ;;
*)
    echo "io-pimdir has no locked source in Cargo.lock" >&2
    exit 1
    ;;
esac

cargo install --locked --features cli --root "$root" "$@"
echo "pimdir ready: $root/bin/pimdir"

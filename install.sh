#!/bin/sh

set -eu

die() {
    printf '%s\n' "$1" >&2
    exit "${2-1}"
}

DESTDIR="${DESTDIR:-}"
PREFIX="${PREFIX:-"$DESTDIR/usr/local"}"
RELEASES_URL="https://github.com/soywod/neverest/releases"

system=$(uname -s | tr [:upper:] [:lower:])
case $system in
  msys*|mingw*|cygwin*|win*) system=windows; binary=neverest.exe ;;
  linux|freebsd) system=linux; binary=neverest ;;
  darwin) system=macos; binary=neverest ;;
  *) die "Unsupported system: $system" ;;
esac

tmpdir=$(mktemp -d) || die "Failed to create tmpdir"
trap "rm -rf $tmpdir" EXIT

echo "Downloading latest $system release…"
curl -sLo "$tmpdir/neverest.tar.gz" \
     "$RELEASES_URL/latest/download/neverest-$system.tar.gz"

echo "Installing binary…"
tar -xzf "$tmpdir/neverest.tar.gz" -C "$tmpdir"

mkdir -p "$PREFIX/bin"
cp -f -- "$tmpdir/$binary" "$PREFIX/bin/$binary"

die "$("$PREFIX/bin/$binary" --version) installed!" 0

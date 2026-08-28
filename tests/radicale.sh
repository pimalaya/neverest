#!/usr/bin/env bash
# Bootstrap a local Radicale server for CalDAV / CardDAV tests.
#
# Radicale is a lightweight CalDAV / CardDAV server. This script runs it
# in a container with a single htpasswd user over plain HTTP, with
# collections auto-created on first write so the test flow can MKCOL its
# own calendars and addressbooks.
#
# Steps:
#   1. Write a plaintext htpasswd file with one user (`test` / `test`).
#   2. Write a minimal config enabling htpasswd auth and granting any
#      authenticated user full access to their collections.
#   3. Start the container, binding Radicale to host port 5232.
#
# Host port mapping:
#   5232 → HTTP (CalDAV + CardDAV)

set -eu

NAME="neverest-dav-tests"
USER="test"
PASS="test"
PORT=5232
IMAGE="tomsquest/docker-radicale:latest"

CONFDIR=$(mktemp -d)
trap 'rm -rf "$CONFDIR"' EXIT

# Plaintext htpasswd entry (`user:password`); fine for a throwaway test
# container and avoids depending on a bcrypt tool to mint the hash.
printf 'test:test\n' > "$CONFDIR/users"
chmod 644 "$CONFDIR/users"

cat > "$CONFDIR/config" <<'EOF'
[server]
hosts = 0.0.0.0:5232

[auth]
type = htpasswd
htpasswd_filename = /config/users
htpasswd_encryption = plain

[rights]
type = authenticated

[storage]
filesystem_folder = /data/collections
EOF
chmod 644 "$CONFDIR/config"

docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" --rm \
    -v "${CONFDIR}/config:/config/config:ro" \
    -v "${CONFDIR}/users:/config/users:ro" \
    -p "${PORT}:5232" \
    "$IMAGE" >/dev/null

# Wait for the HTTP listener to answer (401 without credentials is fine).
for _ in $(seq 1 30); do
    if curl -fsS -o /dev/null -u "${USER}:${PASS}" \
        "http://localhost:${PORT}/" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

echo "radicale ready: dav http://${USER}:${PASS}@127.0.0.1:${PORT}"

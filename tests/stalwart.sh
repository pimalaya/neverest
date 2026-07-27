#!/usr/bin/env bash
# Bootstrap a local Stalwart v0.16 IMAP server for integration tests.
#
# Stalwart v0.16 dropped the REST `/api/principal` endpoint and the
# auto-provisioning script that v0.15 supported. This script does the
# equivalent over the new JMAP management surface (`urn:stalwart:jmap`),
# which is not yet documented but is what the new webadmin uses.
#
# Steps:
#   1. Write a minimal `config.json` (rocksdb data store only).
#   2. Start the container with `STALWART_RECOVERY_ADMIN=admin:test` so
#      a permanent admin exists from first boot (no bootstrap wizard).
#   3. Wait for `/.well-known/jmap` to respond, then resolve the
#      admin's JMAP account id.
#   4. Provision over JMAP `POST /jmap/`:
#        - x:Domain/set     create "pimalaya.org"
#        - x:Account/set    create test user with strong password
#        - x:NetworkListener/set update the default IMAPS listener to
#          plain (useTls = false, tlsImplicit = false) so the test can
#          talk to it without TLS
#        - x:Imap/set       set allowPlainTextAuth = true
#        - x:Action/set     trigger ReloadSettings
#   5. Restart the container so the now-plain listener rebinds. The
#      ReloadSettings action persists the config but doesn't
#      re-open sockets.
#
# Host port mapping:
#   8080 → admin HTTP (JMAP + webadmin at /admin)
#   143  → plain IMAP (mapped to container 993, which we reconfigured
#          to be plain)
#
# The chosen password is `P!malaya-test-2026`. Stalwart's password
# strength check rejects shorter / weaker secrets like `test`.

set -eu

NAME="io-imap-tests"
ADMIN_PASS="test"
IMAP_PASS='P!malaya-test-2026'
ADMIN_PORT=8080
IMAP_HOST_PORT=143
IMAGE="stalwartlabs/stalwart:v0.16-alpine"

CONFIG=$(mktemp)
trap 'rm -f "$CONFIG"' EXIT
printf '{"@type":"RocksDb","path":"/var/lib/stalwart/data"}\n' > "$CONFIG"
# mktemp defaults to mode 600; the stalwart UID inside the container
# needs read access on the bind-mounted config.
chmod 644 "$CONFIG"

docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" --rm \
    -e "STALWART_RECOVERY_ADMIN=admin:${ADMIN_PASS}" \
    -v "${CONFIG}:/etc/stalwart/config.json:ro" \
    -p "${ADMIN_PORT}:8080" \
    -p "${IMAP_HOST_PORT}:993" \
    "$IMAGE" >/dev/null

# Wait for the admin HTTP listener.
for _ in $(seq 1 30); do
    if curl -fsS -u "admin:${ADMIN_PASS}" \
        "http://localhost:${ADMIN_PORT}/.well-known/jmap" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

# Resolve admin's JMAP account id from the session document.
acc=$(curl -fsSL -u "admin:${ADMIN_PASS}" \
    "http://localhost:${ADMIN_PORT}/.well-known/jmap" |
    jq -r '.accounts | keys[0]')

# Locate the default IMAPS listener (it's identified by name "imaps").
imaps_id=$(curl -fsS -u "admin:${ADMIN_PASS}" \
    -H 'Content-Type: application/json' \
    -d "{
      \"using\":[\"urn:ietf:params:jmap:core\",\"urn:stalwart:jmap\"],
      \"methodCalls\":[
        [\"x:NetworkListener/query\",
          {\"accountId\":\"$acc\",\"filter\":{\"name\":\"imaps\"}},\"0\"]
      ]
    }" \
    "http://localhost:${ADMIN_PORT}/jmap/" |
    jq -r '.methodResponses[0][1].ids[0]')

# Batch: create domain + user + flip IMAPS listener to plain + allow
# clear-text auth + reload.
curl -fsS -u "admin:${ADMIN_PASS}" \
    -H 'Content-Type: application/json' \
    -d "{
      \"using\":[\"urn:ietf:params:jmap:core\",\"urn:stalwart:jmap\"],
      \"methodCalls\":[
        [\"x:Domain/set\",
          {\"accountId\":\"$acc\",\"create\":{
            \"d1\":{\"name\":\"pimalaya.org\"}
          }},\"0\"],
        [\"x:Account/set\",
          {\"accountId\":\"$acc\",\"create\":{
            \"u1\":{
              \"@type\":\"User\",
              \"name\":\"test\",
              \"domainId\":\"#d1\",
              \"credentials\":{
                \"0\":{\"@type\":\"Password\",\"secret\":\"${IMAP_PASS}\"}
              }
            }
          }},\"1\"],
        [\"x:NetworkListener/set\",
          {\"accountId\":\"$acc\",\"update\":{
            \"$imaps_id\":{\"useTls\":false,\"tlsImplicit\":false}
          }},\"2\"],
        [\"x:Imap/set\",
          {\"accountId\":\"$acc\",\"update\":{
            \"singleton\":{\"allowPlainTextAuth\":true}
          }},\"3\"],
        [\"x:Action/set\",
          {\"accountId\":\"$acc\",\"create\":{
            \"r1\":{\"@type\":\"ReloadSettings\"}
          }},\"4\"]
      ]
    }" \
    "http://localhost:${ADMIN_PORT}/jmap/" |
    jq -e '.methodResponses[] | .[1] | (.created // .updated // {}) | length > 0' >/dev/null

# Listener config changes take effect on socket rebind, which happens
# at process start. Restart the container to pick up the plain listener.
docker restart "$NAME" >/dev/null

# Wait for the IMAP listener.
for _ in $(seq 1 30); do
    if (echo > /dev/tcp/127.0.0.1/${IMAP_HOST_PORT}) >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

echo "stalwart ready: imap://test@pimalaya.org:${IMAP_PASS}@127.0.0.1:${IMAP_HOST_PORT}"

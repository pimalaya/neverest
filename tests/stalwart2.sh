#!/usr/bin/env bash
# Bootstrap TWO local Stalwart v0.16 IMAP servers for the relay integration
# test: server A on :143 and server B on :144, each its own container and admin
# port. Same provisioning as tests/stalwart.sh, factored into a function.
#
# Cross-server relay (stream A→B, no hub retention) only exercises against two
# DISTINCT servers; a single server would make a copy a same-server COPY.
#
# Host port mapping per instance:
#   admin HTTP (JMAP)  → container 8080
#   plain IMAP         → container 993 (reconfigured to plain)
#   plain SMTP         → container 25, the channel a queued `submit` intent
#                        leaves through

set -eu

IMAP_PASS='P!malaya-test-2026'
ADMIN_PASS="test"
IMAGE="stalwartlabs/stalwart:v0.16-alpine"

# provision <container-name> <admin-host-port> <imap-host-port> <smtp-host-port>
provision() {
    local name="$1" admin_port="$2" imap_port="$3" smtp_port="$4"

    local config
    config=$(mktemp)
    printf '{"@type":"RocksDb","path":"/var/lib/stalwart/data"}\n' > "$config"
    chmod 644 "$config"

    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" --rm \
        -e "STALWART_RECOVERY_ADMIN=admin:${ADMIN_PASS}" \
        -v "${config}:/etc/stalwart/config.json:ro" \
        -p "${admin_port}:8080" \
        -p "${imap_port}:993" \
        -p "${smtp_port}:25" \
        "$IMAGE" >/dev/null

    for _ in $(seq 1 30); do
        if curl -fsS -u "admin:${ADMIN_PASS}" \
            "http://localhost:${admin_port}/.well-known/jmap" >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done

    local acc
    acc=$(curl -fsSL -u "admin:${ADMIN_PASS}" \
        "http://localhost:${admin_port}/.well-known/jmap" |
        jq -r '.accounts | keys[0]')

    local imaps_id
    imaps_id=$(curl -fsS -u "admin:${ADMIN_PASS}" \
        -H 'Content-Type: application/json' \
        -d "{
          \"using\":[\"urn:ietf:params:jmap:core\",\"urn:stalwart:jmap\"],
          \"methodCalls\":[
            [\"x:NetworkListener/query\",
              {\"accountId\":\"$acc\",\"filter\":{\"name\":\"imaps\"}},\"0\"]
          ]
        }" \
        "http://localhost:${admin_port}/jmap/" |
        jq -r '.methodResponses[0][1].ids[0]')

    curl -fsS -u "admin:${ADMIN_PASS}" \
        -H 'Content-Type: application/json' \
        -d "{
          \"using\":[\"urn:ietf:params:jmap:core\",\"urn:stalwart:jmap\"],
          \"methodCalls\":[
            [\"x:Domain/set\",
              {\"accountId\":\"$acc\",\"create\":{\"d1\":{\"name\":\"pimalaya.org\"}}},\"0\"],
            [\"x:Account/set\",
              {\"accountId\":\"$acc\",\"create\":{
                \"u1\":{\"@type\":\"User\",\"name\":\"test\",\"domainId\":\"#d1\",
                  \"credentials\":{\"0\":{\"@type\":\"Password\",\"secret\":\"${IMAP_PASS}\"}}}
              }},\"1\"],
            [\"x:NetworkListener/set\",
              {\"accountId\":\"$acc\",\"update\":{\"$imaps_id\":{\"useTls\":false,\"tlsImplicit\":false}}},\"2\"],
            [\"x:Imap/set\",
              {\"accountId\":\"$acc\",\"update\":{\"singleton\":{\"allowPlainTextAuth\":true}}},\"3\"],
            [\"x:Action/set\",
              {\"accountId\":\"$acc\",\"create\":{\"r1\":{\"@type\":\"ReloadSettings\"}}},\"4\"]
          ]
        }" \
        "http://localhost:${admin_port}/jmap/" |
        jq -e '.methodResponses[] | .[1] | (.created // .updated // {}) | length > 0' >/dev/null

    docker restart "$name" >/dev/null

    for _ in $(seq 1 30); do
        if (echo > "/dev/tcp/127.0.0.1/${imap_port}") >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done

    rm -f "$config"
    echo "stalwart ${name} ready: imap://127.0.0.1:${imap_port}, smtp://127.0.0.1:${smtp_port}"
}

provision "neverest-relay-a" 8080 143 2525
provision "neverest-relay-b" 8081 144 2526
echo "both servers ready (A :143 imap, :2525 smtp; B :144 imap, :2526 smtp)"

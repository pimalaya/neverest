#!/usr/bin/env bash
# Run the Docker suites under a rootless Docker, which binds no port under
# 1024. The servers start on shifted ports, and pasta (from passt) runs the
# command in a network namespace whose loopback forwards each port the
# suites name to its shifted twin, so the suites run as they are.
#
# ```sh
# ./tests/pasta.sh servers
# ./tests/pasta.sh cargo test --test deletes -- --ignored
# ```

set -eu

cd "$(dirname "$0")/.."

export STALWART_ADMIN_PORT=18080 STALWART_IMAP_PORT=1143
export STALWART_A_ADMIN_PORT=18080 STALWART_A_IMAP_PORT=1143 STALWART_A_SMTP_PORT=12525
export STALWART_B_ADMIN_PORT=18081 STALWART_B_IMAP_PORT=1144 STALWART_B_SMTP_PORT=12526
export RADICALE_PORT=15232

if [ "${1:-}" = servers ]; then
    ./tests/stalwart2.sh
    ./tests/radicale.sh
    exit
fi

exec pasta --config-net --quiet \
    -T 143:1143,144:1144,8080:18080,8081:18081,2525:12525,2526:12526,5232:15232 \
    -- "$@"

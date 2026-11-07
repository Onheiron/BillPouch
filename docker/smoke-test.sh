#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo -e "  ${GREEN}✓${NC} $1"; }
fail() { FAIL=$((FAIL + 1)); echo -e "  ${RED}✗${NC} $1"; }
info() { echo -e "${CYAN}▸${NC} $1"; }
header() { echo -e "\n${YELLOW}═══ $1 ═══${NC}"; }

CONTAINERS=("bp-pouch" "bp-bill" "bp-post")

header "1. Waiting for daemons to be ready"
for c in "${CONTAINERS[@]}"; do
    info "Checking ${c}..."
    READY=false
    for i in $(seq 1 60); do
        RESP=$(docker exec "${c}" bash -c \
            'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null' \
        ) || true
        if echo "${RESP}" | grep -q "pong"; then
            pass "${c} daemon is responding"
            READY=true
            break
        fi
        sleep 0.5
    done
    if [ "${READY}" = false ]; then
        fail "${c} daemon never became ready (timeout 30s)"
    fi
done

header "1b. Waiting for every node to join the network through an invite"
for c in "${CONTAINERS[@]}"; do
    if timeout 60 bash -c "until docker exec ${c} test -f /root/.joined; do sleep 1; done"; then
        pass "${c} joined via invite"
    else
        fail "${c} never joined (no invite token after 60s)"
    fi
done

header "2. Waiting for mDNS discovery + gossipsub mesh formation"
info "Sleeping 20s for peers to discover each other and form gossipsub mesh..."
sleep 20

header "3. Hatching services on each node"

hatch_service() {
    local container="$1"
    local svc_type="$2"
    info "Hatching '${svc_type}' on ${container}..."
    local OUT
    local extra=()
    [ "${svc_type}" = "pouch" ] && extra=(--tier T1)
    OUT=$(docker exec "${container}" bp hatch "${svc_type}" --network smoketest "${extra[@]}" 2>&1) || true
    if echo "${OUT}" | grep -qi "hatched\|service_id\|running"; then
        pass "${container} hatched '${svc_type}' successfully"
    else
        fail "${container} failed to hatch '${svc_type}': ${OUT}"
    fi
}

hatch_service "bp-pouch" "pouch"
hatch_service "bp-bill"  "bill"
hatch_service "bp-post"  "post"

header "4. Waiting for gossipsub to propagate NodeInfo"
info "Sleeping 15s for gossipsub announcements to reach all peers..."
sleep 15

header "5. Peer discovery verification"
for c in "${CONTAINERS[@]}"; do
    info "${c} flock output:"
    FLOCK_OUTPUT=$(docker exec "${c}" bp flock 2>&1) || true
    echo "${FLOCK_OUTPUT}"
    echo ""

    PEER_COUNT=$(echo "${FLOCK_OUTPUT}" | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "0")

    if [ "${PEER_COUNT}" -ge 2 ]; then
        pass "${c} sees ${PEER_COUNT} peers (expected >= 2)"
    else
        fail "${c} sees only ${PEER_COUNT} peers (expected >= 2)"
    fi
done

header "6. Service type cross-visibility"

check_service_visible() {
    local from_node="$1"
    local service_type="$2"

    local flock_out
    flock_out=$(docker exec "${from_node}" bp flock 2>&1) || true

    local peers_section
    peers_section=$(echo "${flock_out}" | sed -n '/Known Peers/,$ p' | tail -n +2) || true

    if echo "${peers_section}" | grep -qi "${service_type}"; then
        pass "${from_node} sees a '${service_type}' peer"
    else
        fail "${from_node} does NOT see a '${service_type}' peer"
    fi
}

check_service_visible "bp-pouch" "bill"
check_service_visible "bp-pouch" "post"
check_service_visible "bp-bill" "pouch"
check_service_visible "bp-bill" "post"
check_service_visible "bp-post" "pouch"
check_service_visible "bp-post" "bill"

header "7. Health check (ping/pong)"
for c in "${CONTAINERS[@]}"; do
    PING_OUT=$(docker exec "${c}" bash -c \
        'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null' \
    ) || true

    if echo "${PING_OUT}" | grep -qi "pong"; then
        pass "${c} ping → pong"
    else
        fail "${c} daemon is NOT responding to ping"
    fi
done

header "8. Network membership"
for c in "${CONTAINERS[@]}"; do
    FLOCK_OUT=$(docker exec "${c}" bp flock 2>&1) || true

    if echo "${FLOCK_OUT}" | grep -q "smoketest"; then
        pass "${c} is on network 'smoketest'"
    else
        fail "${c} is NOT on network 'smoketest'"
    fi
done

header "9. Node logs (last 20 lines each)"
for c in "${CONTAINERS[@]}"; do
    info "--- ${c} ---"
    docker logs "${c}" 2>&1 | tail -20
    echo ""
done

header "RESULTS"
TOTAL=$((PASS + FAIL))
echo -e "  ${GREEN}Passed: ${PASS}${NC} / ${TOTAL}"
if [ "${FAIL}" -gt 0 ]; then
    echo -e "  ${RED}Failed: ${FAIL}${NC} / ${TOTAL}"
    echo ""
    echo -e "${RED}SMOKE TEST FAILED${NC}"
    exit 1
else
    echo ""
    echo -e "${GREEN}ALL SMOKE TESTS PASSED${NC}"
    exit 0
fi

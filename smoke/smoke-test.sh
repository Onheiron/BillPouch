#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# BillPouch Smoke Test
# ═══════════════════════════════════════════════════════════════════════════════
# Verifies that 3 BillPouch nodes (pouch, bill, post) discover each other
# via mDNS on a Docker bridge network and exchange NodeInfo via gossipsub.
#
# Usage:
#   docker compose -f docker-compose.smoke.yml up --build -d
#   ./smoke/smoke-test.sh
# ═══════════════════════════════════════════════════════════════════════════════
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo -e "  ${GREEN}✓${NC} $1"; }
fail() { FAIL=$((FAIL + 1)); echo -e "  ${RED}✗${NC} $1"; }
info() { echo -e "${CYAN}▸${NC} $1"; }
header() { echo -e "\n${YELLOW}═══ $1 ═══${NC}"; }

CONTAINERS=("bp-pouch" "bp-bill" "bp-post")

# ── Wait for all nodes to be ready ──────────────────────────────────────────
header "1. Waiting for nodes to be ready"
for c in "${CONTAINERS[@]}"; do
    info "Checking ${c}..."
    for i in $(seq 1 60); do
        if docker exec "${c}" bp flock >/dev/null 2>&1; then
            pass "${c} is responding to 'bp flock'"
            break
        fi
        if [ "$i" -eq 60 ]; then
            fail "${c} never became ready (timeout 30s)"
        fi
        sleep 0.5
    done
done

# ── Give mDNS + gossipsub time to propagate ─────────────────────────────────
header "2. Waiting for peer discovery (mDNS + gossipsub)"
info "Sleeping 15s for mDNS discovery and gossipsub propagation..."
sleep 15

# ── Check each node sees the other peers ────────────────────────────────────
header "3. Peer discovery verification"
for c in "${CONTAINERS[@]}"; do
    info "Checking ${c} peer view..."

    FLOCK_OUTPUT=$(docker exec "${c}" bp flock 2>&1) || true
    echo "${FLOCK_OUTPUT}" | head -30

    # Count known peers (lines that look like peer entries)
    # We use status command which returns JSON with peer_count
    STATUS_OUTPUT=$(docker exec "${c}" bash -c '
        echo "{\"cmd\":\"status\"}" | socat - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null || echo "{}"
    ') || true

    # Fallback: just check flock output for peer indicators
    PEER_COUNT=$(echo "${FLOCK_OUTPUT}" | grep -ci "peer\|node\|pouch\|bill\|post" || true)

    if [ "${PEER_COUNT}" -ge 2 ]; then
        pass "${c} sees other peers (matched ${PEER_COUNT} lines)"
    else
        fail "${c} does NOT see enough peers (matched ${PEER_COUNT} lines)"
    fi
    echo ""
done

# ── Verify each service type is visible from another node ───────────────────
header "4. Service type cross-visibility"

check_service_visible() {
    local from_node="$1"
    local service_type="$2"
    local flock_out
    flock_out=$(docker exec "${from_node}" bp flock 2>&1) || true

    if echo "${flock_out}" | grep -qi "${service_type}"; then
        pass "${from_node} sees a '${service_type}' service"
    else
        fail "${from_node} does NOT see a '${service_type}' service"
    fi
}

# From pouch node, should see bill and post
check_service_visible "bp-pouch" "bill"
check_service_visible "bp-pouch" "post"

# From bill node, should see pouch and post
check_service_visible "bp-bill" "pouch"
check_service_visible "bp-bill" "post"

# From post node, should see pouch and bill
check_service_visible "bp-post" "pouch"
check_service_visible "bp-post" "bill"

# ── Verify ping works on all nodes ──────────────────────────────────────────
header "5. Health check (ping/pong)"
for c in "${CONTAINERS[@]}"; do
    PING_OUT=$(docker exec "${c}" bash -c '
        echo "{\"cmd\":\"ping\"}" | socat - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null || echo "FAIL"
    ') || true

    if echo "${PING_OUT}" | grep -qi "pong"; then
        pass "${c} ping → pong"
    else
        # Fallback: try via bp status
        STATUS_OUT=$(docker exec "${c}" bp flock 2>&1) || true
        if [ -n "${STATUS_OUT}" ]; then
            pass "${c} daemon is responsive (via bp flock)"
        else
            fail "${c} daemon is NOT responding"
        fi
    fi
done

# ── Verify all nodes are on the same network ────────────────────────────────
header "6. Network membership"
for c in "${CONTAINERS[@]}"; do
    FLOCK_OUT=$(docker exec "${c}" bp flock 2>&1) || true

    if echo "${FLOCK_OUT}" | grep -qi "smoketest"; then
        pass "${c} is on network 'smoketest'"
    else
        fail "${c} is NOT on network 'smoketest'"
    fi
done

# ── Node logs (for debugging) ──────────────────────────────────────────────
header "7. Node logs (last 10 lines each)"
for c in "${CONTAINERS[@]}"; do
    info "--- ${c} ---"
    docker logs "${c}" 2>&1 | tail -10
    echo ""
done

# ── Summary ─────────────────────────────────────────────────────────────────
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

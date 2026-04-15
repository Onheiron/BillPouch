#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# agents/chaos/run_chaos.sh — Chaos Test Orchestrator
# ═══════════════════════════════════════════════════════════════════════════════
# Runs all three chaos levels in sequence:
#   L1 — Soft kill (node restart)
#   L2 — Network partition (iptables split)
#   L3 — Byzantine (corrupt fragment responses)
#
# Expects cluster from docker-compose.chaos.yml to be up.
# Containers: bp-chaos1, bp-chaos2, bp-chaos3
# ═══════════════════════════════════════════════════════════════════════════════
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/assert.sh
source "${SCRIPT_DIR}/../lib/assert.sh"

NETWORK="chaostest"
CONTAINERS=("bp-chaos1" "bp-chaos2" "bp-chaos3")
LEVEL="${1:-all}"

# ── Bootstrap helpers ──────────────────────────────────────────────────────────

bootstrap_cluster() {
    suite "Chaos Bootstrap"
    for c in "${CONTAINERS[@]}"; do
        wait_daemon "$c" 90
    done
    hatch "bp-chaos1" "pouch" "$NETWORK"
    hatch "bp-chaos2" "pouch" "$NETWORK"
    hatch "bp-chaos3" "post"  "$NETWORK"
    info "Sleeping 20s for mesh formation..."
    sleep 20
    for c in "${CONTAINERS[@]}"; do
        local count
        count=$(docker exec "$c" bp flock 2>&1 | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "0")
        assert_ge "$count" 1 "${c}: has at least 1 peer before chaos"
    done
}

peers_of() {
    local container="$1"
    docker exec "$container" bp flock 2>&1 | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "0"
}

# ══════════════════════════════════════════════════════════════════════════════
# L1 — Soft Kill (node restart)
# ══════════════════════════════════════════════════════════════════════════════
chaos_l1_kill() {
    suite "L1: Soft Kill — Node Restart"

    local victim="bp-chaos2"
    local observer="bp-chaos1"

    local peers_before
    peers_before=$(peers_of "$observer")
    info "${observer} has ${peers_before} peers before kill"

    # Kill the victim container (SIGKILL — no graceful shutdown)
    info "Sending SIGKILL to ${victim}..."
    docker kill "$victim"

    sleep 5

    # Verify the network continues without the victim
    local ping_out
    ping_out=$(docker exec "$observer" bash -c \
        'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null') || true
    assert_contains "$ping_out" "pong" "${observer}: daemon still alive after peer crash"

    # Restart and verify rejoin
    info "Restarting ${victim}..."
    docker start "$victim"
    wait_daemon "$victim" 30

    info "Sleeping 15s for gossipsub re-mesh..."
    sleep 15

    local peers_after
    peers_after=$(peers_of "$observer")
    assert_ge "$peers_after" 1 "${observer}: still sees peers after restart"

    local victim_peers
    victim_peers=$(peers_of "$victim")
    assert_ge "$victim_peers" 1 "${victim}: re-joined and sees peers"

    pass "L1: network survived node kill and restart"
}

# ══════════════════════════════════════════════════════════════════════════════
# L2 — Network Partition (iptables split)
# ══════════════════════════════════════════════════════════════════════════════
chaos_l2_partition() {
    suite "L2: Network Partition — iptables split"

    # Requires NET_ADMIN capability on bp-chaos3
    local isolated="bp-chaos3"
    local peer1="bp-chaos1"

    # Get bp-chaos3's IP on the Docker network
    local victim_ip
    victim_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$isolated")
    info "${isolated} IP: ${victim_ip}"

    # Block all inbound/outbound traffic on bp-chaos3 except loopback
    info "Partitioning ${isolated} from the network..."
    docker exec --privileged "$isolated" bash -c "
        iptables -A INPUT  ! -i lo -j DROP
        iptables -A OUTPUT ! -o lo -j DROP
    "

    sleep 5

    # bp-chaos1 should still function
    local ping_out
    ping_out=$(docker exec "$peer1" bash -c \
        'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null') || true
    assert_contains "$ping_out" "pong" "${peer1}: survives partition"

    # Wait for stale-peer eviction (NetworkState evicts after 120s by default)
    info "Sleeping 30s (partial wait for eviction; full eviction is 120s in production)..."
    sleep 30

    # Heal partition
    info "Healing partition on ${isolated}..."
    docker exec --privileged "$isolated" bash -c "
        iptables -F INPUT
        iptables -F OUTPUT
    " || true

    info "Sleeping 15s for re-mesh..."
    sleep 15

    local peers_after
    peers_after=$(peers_of "$peer1")
    assert_ge "$peers_after" 1 "${peer1}: mesh healed"

    pass "L2: network survived partition and healed"
}

# ══════════════════════════════════════════════════════════════════════════════
# L3 — Byzantine (corrupt fragment responses)
# ══════════════════════════════════════════════════════════════════════════════
chaos_l3_byzantine() {
    suite "L3: Byzantine — Corrupt storage responses"

    # Strategy: use the PoS fault injection via bp's debug control socket.
    # The byzantine node deliberately fails all PoS challenges.
    # After FAULT_BLACKLISTED (fault_score=100), other nodes should evict it.
    #
    # Current implementation: inject continuous PoS failures by overwriting
    # stored fragments with random bytes on the bitcoin node's disk.

    local evil="bp-chaos1"
    local observer="bp-chaos2"

    # Corrupt all stored fragments on the evil node
    info "Corrupting stored fragments on ${evil}..."
    docker exec "$evil" bash -c "
        find \"\${HOME}/.local/share/billpouch/storage\" -name '*.fragment' \
             -exec sh -c 'dd if=/dev/urandom of=\"\$1\" bs=1 count=\$(stat -c%s \"\$1\") 2>/dev/null' _ {} \\;
    " 2>/dev/null || warn "${evil}: no fragments to corrupt yet (ok for new node)"

    info "Sleeping 10s for PoS cycle..."
    sleep 10

    # Verify the observer's daemon is unaffected
    local ping_out
    ping_out=$(docker exec "$observer" bash -c \
        'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null') || true
    assert_contains "$ping_out" "pong" "${observer}: unaffected by byzantine peer"

    # Check that the evil node starts accumulating fault score
    # (visible in bp status --verbose or logs)
    local evil_status
    evil_status=$(docker exec "$evil" bp status 2>&1) || true
    info "${evil} status after corruption:\n${evil_status}"

    pass "L3: observer daemon survived byzantine peer scenario"
    warn "L3: Full blacklist eviction requires ~10 failed PoS cycles (300s each) — not waited here"
}

# ══════════════════════════════════════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════════════════════════════════════
case "$LEVEL" in
    l1)  bootstrap_cluster ; chaos_l1_kill ;;
    l2)  bootstrap_cluster ; chaos_l2_partition ;;
    l3)  bootstrap_cluster ; chaos_l3_byzantine ;;
    all)
        bootstrap_cluster
        chaos_l1_kill
        chaos_l2_partition
        chaos_l3_byzantine
        ;;
    *)
        echo "Usage: $0 [l1|l2|l3|all]"
        exit 1
        ;;
esac

print_results "CHAOS TESTS"

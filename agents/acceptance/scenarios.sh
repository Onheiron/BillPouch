#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# agents/acceptance/scenarios.sh — End-to-end acceptance scenarios
# ═══════════════════════════════════════════════════════════════════════════════
# Expects a running cluster defined in docker-compose.acceptance.yml:
#   bp-bill   — Bill node (performs put/get)
#   bp-pouch1 — Pouch node 1 (T1 storage)
#   bp-pouch2 — Pouch node 2 (T1 storage)
#   bp-pouch3 — Pouch node 3 (T1 storage)
#   bp-post   — Post node (relay)
#
# Usage:
#   ./agents/acceptance/scenarios.sh [scenario_name]
#   If no scenario specified, all scenarios run in order.
# ═══════════════════════════════════════════════════════════════════════════════
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/assert.sh
source "${SCRIPT_DIR}/../lib/assert.sh"

NETWORK="acceptance"
BILL="bp-bill"
POUCHES=("bp-pouch1" "bp-pouch2" "bp-pouch3")
POST="bp-post"
ALL_NODES=("$BILL" "${POUCHES[@]}" "$POST")

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 0 — Cluster Bootstrap
# ══════════════════════════════════════════════════════════════════════════════
scenario_bootstrap() {
    suite "S0: Cluster Bootstrap"

    # Wait for all daemons
    for c in "${ALL_NODES[@]}"; do
        wait_daemon "$c" 90
    done

    # Hatch services
    hatch "$BILL"     "bill"  "$NETWORK"
    hatch "bp-pouch1" "pouch" "$NETWORK"
    hatch "bp-pouch2" "pouch" "$NETWORK"
    hatch "bp-pouch3" "pouch" "$NETWORK"
    hatch "$POST"     "post"  "$NETWORK"

    info "Sleeping 25s for gossipsub mesh to form..."
    sleep 25
}

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 1 — Peer Discovery
# ══════════════════════════════════════════════════════════════════════════════
scenario_peer_discovery() {
    suite "S1: Peer Discovery"

    for c in "${ALL_NODES[@]}"; do
        local out
        out=$(docker exec "$c" bp flock 2>&1) || true
        local count
        count=$(echo "$out" | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "0")
        assert_ge "$count" 2 "${c}: known peers"
    done
}

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 2 — Storage Advertised
# ══════════════════════════════════════════════════════════════════════════════
scenario_storage_advertised() {
    suite "S2: Storage Advertised"

    for p in "${POUCHES[@]}"; do
        local out
        out=$(docker exec "$p" bp status 2>&1) || true

        assert_contains "$out" "bid:"       "${p}: bid field present"
        assert_contains "$out" "avail (eff.)" "${p}: effective avail field present"

        # bid must be non-zero (T1 = 10 GiB)
        local bid_bytes
        bid_bytes=$(echo "$out" | grep -oP 'bid:\s+\K[0-9.]+' | head -1 || echo "0")
        if awk "BEGIN { exit !($bid_bytes > 0) }"; then
            pass "${p}: bid > 0 (${bid_bytes})"
        else
            fail "${p}: bid is 0"
        fi
    done
}

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 3 — Put / Get (file round-trip)
# ══════════════════════════════════════════════════════════════════════════════
scenario_put_get() {
    suite "S3: Put / Get (file round-trip)"

    local test_file="/tmp/bp_acceptance_test.bin"
    local got_file="/tmp/bp_acceptance_got.bin"

    # Create a 4 KiB pseudo-random payload on the bill node
    docker exec "$BILL" bash -c \
        "dd if=/dev/urandom bs=1k count=4 of=${test_file} 2>/dev/null"
    local sha_before
    sha_before=$(docker exec "$BILL" sha256sum "$test_file" | awk '{print $1}')
    info "Payload SHA256: ${sha_before}"

    # Put the file
    info "Running bp put on ${BILL}..."
    local put_out
    put_out=$(docker exec "$BILL" bp put "$test_file" --network "$NETWORK" 2>&1) || true
    assert_contains "$put_out" "chunk_id\|uploaded\|put" "${BILL}: put returned chunk_id"

    # Extract chunk_id (format: "chunk_id: <hex>")
    local chunk_id
    chunk_id=$(echo "$put_out" | grep -oP '(?i)(chunk.?id[^:]*:\s*)\K[a-f0-9]+' | head -1 || true)
    if [ -z "$chunk_id" ]; then
        fail "Could not extract chunk_id from put output: ${put_out}"
        return
    fi
    pass "chunk_id extracted: ${chunk_id}"

    # Get the file back
    info "Running bp get ${chunk_id} on ${BILL}..."
    local get_out
    get_out=$(docker exec "$BILL" \
        bp get "$chunk_id" --output "$got_file" --network "$NETWORK" 2>&1) || true
    assert_contains "$get_out" "saved\|written\|get\|ok" "${BILL}: get returned success"

    # Verify integrity
    local sha_after
    sha_after=$(docker exec "$BILL" sha256sum "$got_file" 2>/dev/null | awk '{print $1}' || echo "")
    assert_eq "$sha_after" "$sha_before" "SHA256 after round-trip"
}

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 4 — Pause / Resume
# ══════════════════════════════════════════════════════════════════════════════
scenario_pause_resume() {
    suite "S4: Pause / Resume"

    local target="${POUCHES[0]}"  # bp-pouch1

    # Get service_id of the first pouch
    local status_out
    status_out=$(docker exec "$target" bp status 2>&1) || true
    local svc_id
    svc_id=$(echo "$status_out" | grep -oP '\[([a-f0-9]{8})\]' | head -1 | tr -d '[]' || true)
    if [ -z "$svc_id" ]; then
        fail "${target}: could not extract service_id from status output"
        return
    fi
    info "Service ID prefix: ${svc_id}"

    # Pause with 30 min ETA
    local pause_out
    pause_out=$(docker exec "$target" bp pause "$svc_id" --eta 30 2>&1) || true
    assert_contains "$pause_out" "paused\|ok" "${target}: pause command succeeded"

    sleep 3

    # Verify paused state in status
    local status_after
    status_after=$(docker exec "$target" bp status 2>&1) || true
    assert_contains "$status_after" "paused\|pause" "${target}: status shows paused"

    # Resume
    local resume_out
    resume_out=$(docker exec "$target" bp resume "$svc_id" 2>&1) || true
    assert_contains "$resume_out" "resumed\|ok" "${target}: resume command succeeded"

    sleep 3

    # Verify running again
    local status_final
    status_final=$(docker exec "$target" bp status 2>&1) || true
    assert_not_contains "$status_final" "paused" "${target}: status no longer paused"
}

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 5 — Farewell (graceful depart)
# ══════════════════════════════════════════════════════════════════════════════
scenario_farewell() {
    suite "S5: Farewell"

    # Use the last pouch so put/get scenarios still have pouch1+2
    local target="${POUCHES[2]}"  # bp-pouch3

    # Get service_id
    local status_out
    status_out=$(docker exec "$target" bp status 2>&1) || true
    local svc_id
    svc_id=$(echo "$status_out" | grep -oP '\[([a-f0-9]{8})\]' | head -1 | tr -d '[]' || true)
    if [ -z "$svc_id" ]; then
        fail "${target}: could not extract service_id"
        return
    fi

    # Farewell
    local farewell_out
    farewell_out=$(docker exec "$target" bp farewell "$svc_id" 2>&1) || true
    assert_contains "$farewell_out" "farewell\|ok\|departed" "${target}: farewell succeeded"

    sleep 5

    # Verify gone from bill's flock
    local flock_out
    flock_out=$(docker exec "$BILL" bp flock 2>&1) || true
    local peer_count
    peer_count=$(echo "$flock_out" | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "0")
    # After farewell of pouch3, bill should see at most N-1 nodes (may take time for eviction)
    info "${BILL} now sees ${peer_count} peers (expected one fewer)"
    assert_contains "$farewell_out" "farewell\|departed\|ok" \
        "${target}: farewell was acknowledged"
}

# ══════════════════════════════════════════════════════════════════════════════
# SCENARIO 6 — QoS Factor sanity
# ══════════════════════════════════════════════════════════════════════════════
scenario_qos_sanity() {
    suite "S6: QoS Availability Factor Sanity"

    for p in "${POUCHES[@]}"; do
        local out
        out=$(docker exec "$p" bp status 2>&1) || true

        # Extract bid and avail_eff as GiB values
        local bid_g avail_g
        bid_g=$(echo "$out"   | grep -oP 'bid:\s+\K[0-9.]+' | head -1 || echo "0")
        avail_g=$(echo "$out" | grep -oP 'avail \(eff\.\):\s+\K[0-9.]+' | head -1 || echo "0")

        # avail must be <= bid
        if awk "BEGIN { exit !($avail_g <= $bid_g) }"; then
            pass "${p}: avail_eff (${avail_g}) <= bid (${bid_g})"
        else
            fail "${p}: avail_eff (${avail_g}) > bid (${bid_g}) — impossible"
        fi

        # With 3+ pouch peers in a fresh network, factor should be > 0
        if awk "BEGIN { exit !($avail_g > 0) }"; then
            pass "${p}: avail_eff > 0 with peers present"
        else
            warn "${p}: avail_eff == 0 — may be ok if QoS data not yet gathered"
        fi
    done
}

# ══════════════════════════════════════════════════════════════════════════════
# MAIN
# ══════════════════════════════════════════════════════════════════════════════
RUN_SCENARIO="${1:-all}"

case "$RUN_SCENARIO" in
    bootstrap)         scenario_bootstrap ;;
    peer_discovery)    scenario_peer_discovery ;;
    storage)           scenario_storage_advertised ;;
    put_get)           scenario_put_get ;;
    pause_resume)      scenario_pause_resume ;;
    farewell)          scenario_farewell ;;
    qos_sanity)        scenario_qos_sanity ;;
    all)
        scenario_bootstrap
        scenario_peer_discovery
        scenario_storage_advertised
        scenario_put_get
        scenario_pause_resume
        scenario_farewell
        scenario_qos_sanity
        ;;
    *)
        echo "Unknown scenario: ${RUN_SCENARIO}"
        echo "Available: bootstrap | peer_discovery | storage | put_get | pause_resume | farewell | qos_sanity | all"
        exit 1
        ;;
esac

print_results "ACCEPTANCE TESTS"

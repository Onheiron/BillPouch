#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# agents/lib/assert.sh — shared assertion helpers for all agent scripts
# ═══════════════════════════════════════════════════════════════════════════════
# Source this file from any agent script:
#   source "$(dirname "$0")/../lib/assert.sh"
# ═══════════════════════════════════════════════════════════════════════════════

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

PASS=0
FAIL=0
SKIP=0
_CURRENT_SUITE=""

# ── Output helpers ─────────────────────────────────────────────────────────────

pass()   { PASS=$((PASS + 1));  echo -e "  ${GREEN}✓${NC} $1"; }
fail()   { FAIL=$((FAIL + 1));  echo -e "  ${RED}✗${NC} $1"; }
skip()   { SKIP=$((SKIP + 1));  echo -e "  ${YELLOW}⊘${NC} (skip) $1"; }
warn()   { echo -e "  ${YELLOW}⚠${NC} $1"; }
info()   { echo -e "  ${CYAN}▸${NC} $1"; }

suite()  {
    _CURRENT_SUITE="$1"
    echo -e "\n${BOLD}${YELLOW}┌─ $1 ─${NC}"
}

# ── Assertion primitives ───────────────────────────────────────────────────────

# assert_eq <actual> <expected> <label>
assert_eq() {
    local actual="$1" expected="$2" label="$3"
    if [ "$actual" = "$expected" ]; then
        pass "${label}: '${actual}'"
    else
        fail "${label}: expected '${expected}', got '${actual}'"
    fi
}

# assert_ge <actual> <min> <label>
assert_ge() {
    local actual="$1" min="$2" label="$3"
    if [ "$actual" -ge "$min" ] 2>/dev/null; then
        pass "${label}: ${actual} >= ${min}"
    else
        fail "${label}: ${actual} < ${min}"
    fi
}

# assert_contains <haystack> <needle> <label>
assert_contains() {
    local haystack="$1" needle="$2" label="$3"
    if echo "$haystack" | grep -qi "$needle"; then
        pass "${label}: found '${needle}'"
    else
        fail "${label}: '${needle}' not found in output"
    fi
}

# assert_not_contains <haystack> <needle> <label>
assert_not_contains() {
    local haystack="$1" needle="$2" label="$3"
    if ! echo "$haystack" | grep -qi "$needle"; then
        pass "${label}: '${needle}' absent (expected)"
    else
        fail "${label}: '${needle}' unexpectedly found"
    fi
}

# assert_zero_exit <cmd> <label>
assert_zero_exit() {
    local cmd="$1" label="$2"
    if eval "$cmd" >/dev/null 2>&1; then
        pass "${label}"
    else
        fail "${label}: command exited non-zero"
    fi
}

# ── Container helpers ──────────────────────────────────────────────────────────

# bp_cmd <container> <args...>  — run a bp command inside a container
bp_cmd() {
    local container="$1"; shift
    docker exec "$container" bp "$@" 2>&1
}

# wait_daemon <container> [timeout_secs=60]
wait_daemon() {
    local container="$1" timeout="${2:-60}"
    info "Waiting for daemon on ${container} (max ${timeout}s)..."
    for _ in $(seq 1 "$timeout"); do
        local resp
        resp=$(docker exec "$container" bash -c \
            'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null') || true
        if echo "$resp" | grep -q "pong"; then
            return 0
        fi
        sleep 1
    done
    fail "${container} daemon never became ready (timeout ${timeout}s)"
    return 1
}

# hatch <container> <service_type> <network>
hatch() {
    local container="$1" svc="$2" network="$3"
    local out
    out=$(docker exec "$container" bp hatch "$svc" --network "$network" 2>&1) || true
    if echo "$out" | grep -qi "hatched\|service_id\|running"; then
        pass "${container}: hatched '${svc}'"
    else
        fail "${container}: hatch '${svc}' failed — ${out}"
    fi
}

# ── Results summary ────────────────────────────────────────────────────────────

print_results() {
    local suite_label="${1:-RESULTS}"
    local total=$((PASS + FAIL))
    echo ""
    echo -e "${BOLD}${YELLOW}═══ ${suite_label} ═══${NC}"
    echo -e "  ${GREEN}Passed: ${PASS}${NC} / ${total}"
    [ "$SKIP" -gt 0 ] && echo -e "  ${YELLOW}Skipped: ${SKIP}${NC}"
    if [ "$FAIL" -gt 0 ]; then
        echo -e "  ${RED}Failed: ${FAIL}${NC} / ${total}"
        return 1
    fi
    echo -e "\n${GREEN}ALL ${suite_label} PASSED${NC}"
    return 0
}

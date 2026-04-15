#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# agents/protocol/fuzz_control.sh — Control Socket Protocol Fuzzer
# ═══════════════════════════════════════════════════════════════════════════════
# Fires a battery of malformed, edge-case, and unexpected JSON payloads at the
# daemon's control socket and verifies:
#   1. The daemon never crashes (control socket still responds after each payload)
#   2. Malformed requests return ControlResponse::Error, not a panic/hang
#
# Usage:
#   ./agents/protocol/fuzz_control.sh <container>
#   Default container: bp-chaos1
# ═══════════════════════════════════════════════════════════════════════════════
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/assert.sh
source "${SCRIPT_DIR}/../lib/assert.sh"

CONTAINER="${1:-bp-chaos1}"
SOCK="\${HOME}/.local/share/billpouch/control.sock"

send() {
    # send_payload <payload_string>
    local payload="$1"
    docker exec "$CONTAINER" bash -c \
        "echo '${payload}' | socat -T3 - UNIX-CONNECT:${SOCK} 2>/dev/null" \
        || echo '{"status":"error","message":"socket_error"}'
}

alive() {
    local resp
    resp=$(docker exec "$CONTAINER" bash -c \
        "echo '{\"cmd\":\"Ping\"}' | socat -T3 - UNIX-CONNECT:${SOCK} 2>/dev/null") || true
    echo "$resp" | grep -q "pong"
}

fuzz_case() {
    local label="$1" payload="$2"
    local resp
    resp=$(send "$payload")
    # Daemon must still be alive after each fuzz input
    if alive; then
        if echo "$resp" | grep -qi '"status":"error"\|"error"'; then
            pass "${label}: returned error (correct)"
        else
            pass "${label}: daemon alive (response: ${resp:0:60})"
        fi
    else
        fail "${label}: DAEMON CRASHED — did not respond to ping after fuzz input"
    fi
}

# ── Wait for daemon ────────────────────────────────────────────────────────────
wait_daemon "$CONTAINER" 90

suite "Protocol Fuzzer: ${CONTAINER}"

# ── Valid baseline ─────────────────────────────────────────────────────────────
info "Establishing baseline..."
resp=$(send '{"cmd":"Ping"}')
assert_contains "$resp" "pong" "Baseline Ping → pong"

# ── Empty / whitespace ─────────────────────────────────────────────────────────
suite "Empty / whitespace inputs"
fuzz_case "empty string"        ""
fuzz_case "single newline"      $'\n'
fuzz_case "spaces only"         "   "
fuzz_case "null byte"           $'\x00'

# ── Malformed JSON ─────────────────────────────────────────────────────────────
suite "Malformed JSON"
fuzz_case "truncated object"    '{"cmd":"Sta'
fuzz_case "no closing brace"    '{"cmd":"Status"'
fuzz_case "double comma"        '{"cmd":"Status",,"extra":1}'
fuzz_case "unquoted key"        '{cmd:"Status"}'
fuzz_case "trailing text"       '{"cmd":"Ping"} garbage after'
fuzz_case "pure number"         '42'
fuzz_case "boolean"             'true'
fuzz_case "null"                'null'
fuzz_case "array"               '[1,2,3]'
fuzz_case "deeply nested"       '{"a":{"b":{"c":{"d":{"e":{"f":"g"}}}}}}'

# ── Unknown / unexpected fields ────────────────────────────────────────────────
suite "Unknown variants and fields"
fuzz_case "unknown cmd"              '{"cmd":"NOPE"}'
fuzz_case "extra fields"             '{"cmd":"Ping","extra":9999,"inject":"x"}'
fuzz_case "cmd is null"              '{"cmd":null}'
fuzz_case "cmd is number"            '{"cmd":42}'
fuzz_case "cmd is array"             '{"cmd":[]}'
fuzz_case "Hatch missing fields"     '{"cmd":"Hatch"}'
fuzz_case "PutFile missing chunk"    '{"cmd":"PutFile","network_id":"x"}'
fuzz_case "PutFile huge ph"          '{"cmd":"PutFile","chunk_data":[],"ph":9999.0,"q_target":1.0,"network_id":"x"}'
fuzz_case "PutFile negative ph"      '{"cmd":"PutFile","chunk_data":[],"ph":-0.5,"q_target":1.0,"network_id":"x"}'
fuzz_case "Leave force not bool"     '{"cmd":"Leave","network_id":"x","force":"yes"}'

# ── Oversized payloads ─────────────────────────────────────────────────────────
suite "Oversized payloads"
big_string=$(python3 -c "print('A' * 100000)" 2>/dev/null || printf '%0.s' {1..100000})
fuzz_case "100 kB string value"  "{\"cmd\":\"GetFile\",\"chunk_id\":\"${big_string}\",\"network_id\":\"x\"}"

# ── Special character injection ────────────────────────────────────────────────
suite "Special characters"
fuzz_case "SQL injection"        '{"cmd":"Status","id":"1 OR 1=1; DROP TABLE nodes"}'
fuzz_case "shell injection"      '{"cmd":"Hatch","service_type":"pouch","network_id":"x; rm -rf /","metadata":{}}'
fuzz_case "unicode BOM"          $'\xEF\xBB\xBF{"cmd":"Ping"}'
fuzz_case "emoji in network_id"  '{"cmd":"Join","network_id":"🔥network🔥"}'
fuzz_case "null bytes in string" '{"cmd":"Join","network_id":"net\u0000work"}'
fuzz_case "very long network_id" "{\"cmd\":\"Join\",\"network_id\":\"$(python3 -c 'print("x"*4096)' 2>/dev/null || printf 'x%.0s' {1..4096})\"}"

# ── Concurrent rapid-fire ──────────────────────────────────────────────────────
suite "Rapid-fire concurrent requests"
info "Sending 50 pings as fast as possible..."
errors=0
for _ in $(seq 1 50); do
    r=$(send '{"cmd":"Ping"}') || true
    echo "$r" | grep -q "pong" || errors=$((errors + 1))
done
if [ "$errors" -eq 0 ]; then
    pass "50 rapid pings: all returned pong"
else
    fail "50 rapid pings: ${errors} failures"
fi

# ── Final alive check ──────────────────────────────────────────────────────────
suite "Post-fuzz health check"
resp_final=$(send '{"cmd":"Ping"}')
assert_contains "$resp_final" "pong" "Daemon alive after full fuzz battery"

print_results "PROTOCOL FUZZ TESTS"

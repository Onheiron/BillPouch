#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# agents/drift/check_drift.sh — Architecture Invariant Checker
# ═══════════════════════════════════════════════════════════════════════════════
# Verifies that the codebase respects the architectural contracts defined in
# AGENTS.md / CLAUDE.md without building the project.
#
# Run as a fast pre-flight check on every PR.
# Exit code 0 = all invariants hold; non-zero = violation found (blocks PR).
# ═══════════════════════════════════════════════════════════════════════════════
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/assert.sh
source "${SCRIPT_DIR}/../lib/assert.sh"

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "$(cd "${SCRIPT_DIR}/../.." && pwd)")"
CORE="${ROOT}/crates/bp-core/src"
CLI="${ROOT}/crates/bp-cli/src"
API="${ROOT}/crates/bp-api/src"

GREP="grep -rn --include='*.rs'"

# Returns 0 (pass) when the pattern is NOT found in dir
no_match() {
    local pattern="$1" dir="$2" label="$3"
    local out
    out=$(eval "${GREP} \"${pattern}\" \"${dir}\"" 2>/dev/null || true)
    if [ -z "$out" ]; then
        pass "$label"
    else
        fail "$label"
        echo "$out" | head -10 | sed 's/^/    /'
    fi
}

# ── bp-core purity rules ───────────────────────────────────────────────────────

suite "bp-core: No direct I/O"
no_match 'println!'         "$CORE" "no println! in bp-core"
no_match 'eprintln!'        "$CORE" "no eprintln! in bp-core"
no_match 'print!'           "$CORE" "no print! in bp-core (except tests)"
no_match 'process::exit'    "$CORE" "no process::exit in bp-core"
no_match 'std::io::stdout'  "$CORE" "no stdout handle in bp-core"
no_match 'std::io::stderr'  "$CORE" "no stderr handle in bp-core"

suite "bp-core: No anyhow (use thiserror + BpError)"
# anyhow::Result is only banned in lib public surface; allow in test modules
out=$(eval "${GREP} 'anyhow' \"${CORE}\"" 2>/dev/null \
    | grep -v '#\[cfg(test)\]' \
    | grep -v 'mod tests' \
    | grep -v '//' \
    || true)
if [ -z "$out" ]; then
    pass "no anyhow in bp-core (non-test code)"
else
    fail "anyhow found in bp-core non-test code"
    echo "$out" | head -10 | sed 's/^/    /'
fi

suite "bp-cli: No direct libp2p usage"
no_match 'libp2p'           "$CLI"  "no libp2p import in bp-cli"
no_match 'PeerId'           "$CLI"  "no PeerId in bp-cli (use ControlClient)"
no_match 'swarm'            "$CLI"  "no swarm reference in bp-cli"

suite "bp-cli: No process::exit (use anyhow propagation)"
no_match 'process::exit'    "$CLI"  "no process::exit in bp-cli"

suite "bp-core: Error handling"
# Every pub fn that returns Result should use BpResult, not raw anyhow::Result
# (heuristic: look for ' -> anyhow::Result' in public signatures)
out=$(eval "${GREP} '-> anyhow::Result' \"${CORE}\"" 2>/dev/null \
    | grep 'pub fn\|pub async fn' \
    | grep -v '//' \
    || true)
if [ -z "$out" ]; then
    pass "public functions in bp-core use BpResult, not anyhow::Result"
else
    warn "Some public fns return anyhow::Result (consider BpResult):"
    echo "$out" | head -5 | sed 's/^/    /'
fi

suite "General: No TODO/FIXME in non-test code (warnings)"
for dir in "$CORE" "$CLI" "$API"; do
    local_name=$(basename "$(dirname "$dir")")
    count=$(eval "${GREP} 'TODO\|FIXME\|HACK\|XXX' \"${dir}\"" 2>/dev/null \
        | grep -v '#\[cfg(test)\]' \
        | grep -v 'mod tests' \
        | wc -l || echo 0)
    if [ "$count" -gt 0 ]; then
        warn "${local_name}/src: ${count} TODO/FIXME comments (not blocking)"
    else
        pass "${local_name}/src: no pending TODOs"
    fi
done

suite "Workspace: control.sock path consistency"
# The socket path must always be $HOME/.local/share/billpouch/control.sock
out=$(eval "${GREP} 'control\\.sock' \"${ROOT}/crates\"" 2>/dev/null \
    | grep -v 'billpouch/control.sock' \
    | grep -v '//' \
    | grep -v 'test' \
    || true)
if [ -z "$out" ]; then
    pass "control.sock path is consistent everywhere"
else
    fail "control.sock used with unexpected path:"
    echo "$out" | head -5 | sed 's/^/    /'
fi

suite "Protocol: ControlRequest variants documented"
# Every variant name in ControlRequest enum should appear at least once in wiki/
PROTO_FILE="${ROOT}/crates/bp-core/src/control/protocol.rs"
if [ -f "$PROTO_FILE" ]; then
    variants=$(grep -oP '^\s+\K\w+(?=\s*(\{|\,|$))' "$PROTO_FILE" \
        | grep -E '^[A-Z][a-zA-Z]+$' || true)
    undocumented=0
    while IFS= read -r variant; do
        [ -z "$variant" ] && continue
        if ! grep -rql "$variant" "${ROOT}/wiki/" 2>/dev/null; then
            warn "ControlRequest::${variant} not mentioned in wiki/"
            undocumented=$((undocumented + 1))
        fi
    done <<< "$variants"
    if [ "$undocumented" -eq 0 ]; then
        pass "all ControlRequest variants referenced in wiki/"
    fi
fi

# ── Final result ───────────────────────────────────────────────────────────────
print_results "ARCHITECTURE DRIFT CHECK"

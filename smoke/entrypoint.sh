#!/usr/bin/env bash
set -euo pipefail

# ── Args from environment ────────────────────────────────────────────────────
NODE_NAME="${NODE_NAME:-node}"
NETWORK="${NETWORK:-smoketest}"

echo "=== BillPouch node: ${NODE_NAME} on network '${NETWORK}' ==="

# ── Create identity ──────────────────────────────────────────────────────────
bp login --alias "${NODE_NAME}"
echo "[${NODE_NAME}] Identity created."

# ── Start daemon in background ───────────────────────────────────────────────
bp --daemon &
DAEMON_PID=$!
echo "[${NODE_NAME}] Daemon started (PID ${DAEMON_PID})."

# ── Wait for the control socket ──────────────────────────────────────────────
SOCKET_DIR="${HOME}/.local/share/billpouch"
for i in $(seq 1 30); do
    if [ -S "${SOCKET_DIR}/control.sock" ]; then
        echo "[${NODE_NAME}] Control socket ready."
        break
    fi
    sleep 0.2
done

# ── Keep container alive ─────────────────────────────────────────────────────
echo "[${NODE_NAME}] Node is live. Waiting for peers..."
wait "${DAEMON_PID}"

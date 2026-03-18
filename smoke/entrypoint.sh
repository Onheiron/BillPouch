#!/usr/bin/env bash
set -euo pipefail

# ── Args from environment ────────────────────────────────────────────────────
NODE_NAME="${NODE_NAME:-node}"
SERVICE_TYPE="${SERVICE_TYPE:-post}"
NETWORK="${NETWORK:-smoketest}"

echo "=== BillPouch node: ${NODE_NAME} (${SERVICE_TYPE}) on network '${NETWORK}' ==="

# ── Create identity ──────────────────────────────────────────────────────────
bp login --alias "${NODE_NAME}"
echo "[${NODE_NAME}] Identity created."

# ── Start daemon in background ───────────────────────────────────────────────
bp --daemon &
DAEMON_PID=$!
echo "[${NODE_NAME}] Daemon started (PID ${DAEMON_PID})."

# Wait for the control socket to appear
SOCKET_DIR="${HOME}/.local/share/billpouch"
for i in $(seq 1 30); do
    if [ -S "${SOCKET_DIR}/control.sock" ]; then
        echo "[${NODE_NAME}] Control socket ready."
        break
    fi
    sleep 0.5
done

if [ ! -S "${SOCKET_DIR}/control.sock" ]; then
    echo "[${NODE_NAME}] ERROR: control socket never appeared!"
    exit 1
fi

# ── Hatch service ────────────────────────────────────────────────────────────
bp hatch "${SERVICE_TYPE}" --network "${NETWORK}"
echo "[${NODE_NAME}] Service '${SERVICE_TYPE}' hatched on network '${NETWORK}'."

# ── Keep container alive ─────────────────────────────────────────────────────
echo "[${NODE_NAME}] Node is live. Waiting..."
wait "${DAEMON_PID}"

#!/usr/bin/env bash
set -euo pipefail

# ── Config from environment ──────────────────────────────────────────────────
NODE_NAME="${NODE_NAME:-anonymous}"
NETWORK="${NETWORK:-playground}"
SERVICES="${SERVICES:-}"          # comma-separated: "pouch,bill"
INTERACTIVE="${INTERACTIVE:-false}"

echo "╔══════════════════════════════════════════════════╗"
echo "║  BillPouch Playground — ${NODE_NAME}"
echo "╚══════════════════════════════════════════════════╝"

# ── Create identity ──────────────────────────────────────────────────────────
bp login --alias "${NODE_NAME}"
echo "[${NODE_NAME}] Identity created."

# ── Interactive mode: just start daemon, user does the rest ──────────────────
if [ "${INTERACTIVE}" = "true" ]; then
    echo ""
    echo "  You are: ${NODE_NAME}"
    echo "  Network: ${NETWORK}"
    echo ""
    echo "  Your daemon is starting. Use these commands:"
    echo "    bp flock                    — see the network"
    echo "    bp hatch pouch --network ${NETWORK}  — start a storage service"
    echo "    bp hatch bill  --network ${NETWORK}  — start a file I/O service"
    echo "    bp hatch post  --network ${NETWORK}  — start a relay service"
    echo "    bp farewell <service_id>    — stop a service"
    echo ""
    bp --daemon &
    DAEMON_PID=$!

    # Wait for socket
    SOCKET_DIR="${HOME}/.local/share/billpouch"
    for i in $(seq 1 30); do
        [ -S "${SOCKET_DIR}/control.sock" ] && break
        sleep 0.2
    done

    echo "[${NODE_NAME}] Daemon ready. Run 'bp flock' to see the network."
    echo ""

    # Drop into interactive shell
    exec bash
fi

# ── Bot mode: auto-hatch services ───────────────────────────────────────────
bp --daemon &
DAEMON_PID=$!

# Wait for socket
SOCKET_DIR="${HOME}/.local/share/billpouch"
for i in $(seq 1 30); do
    [ -S "${SOCKET_DIR}/control.sock" ] && break
    sleep 0.2
done

# Join network first (gossipsub topic subscription keeps connections alive)
bp join "${NETWORK}" 2>/dev/null || true
echo "[${NODE_NAME}] Joined network '${NETWORK}'."

# Wait for mesh to form before hatching
sleep 5

# Hatch each service
if [ -n "${SERVICES}" ]; then
    IFS=',' read -ra SVC_LIST <<< "${SERVICES}"
    for svc in "${SVC_LIST[@]}"; do
        svc=$(echo "${svc}" | tr -d ' ')
        echo "[${NODE_NAME}] Hatching '${svc}'..."
        bp hatch "${svc}" --network "${NETWORK}" || true
        sleep 1
    done
fi

echo "[${NODE_NAME}] Node is live with services: ${SERVICES}"
echo ""

# Keep alive
wait "${DAEMON_PID}"

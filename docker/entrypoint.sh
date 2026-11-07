#!/usr/bin/env bash
set -euo pipefail

NODE_NAME="${NODE_NAME:-node}"
NETWORK="${NETWORK:-playground}"
SERVICES="${SERVICES:-}"

bp login --alias "${NODE_NAME}"
bp --daemon &
DAEMON_PID=$!

for _ in $(seq 1 30); do
    [ -S "${HOME}/.local/share/billpouch/control.sock" ] && break
    sleep 0.2
done

bp join "${NETWORK}" || true
echo "[${NODE_NAME}] daemon up, joined '${NETWORK}'"

if [ -n "${SERVICES}" ]; then
    sleep 15
    IFS=',' read -ra SVC_LIST <<< "${SERVICES}"
    for svc in "${SVC_LIST[@]}"; do
        if [ "${svc}" = "pouch" ]; then
            bp hatch pouch --network "${NETWORK}" --tier "${STORAGE_TIER:-T1}" || true
        else
            bp hatch "${svc}" --network "${NETWORK}" || true
        fi
    done
fi

if [ "${INTERACTIVE:-false}" = "true" ]; then
    exec bash
fi
wait "${DAEMON_PID}"

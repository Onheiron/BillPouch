#!/usr/bin/env bash
set -euo pipefail

NODE_NAME="${NODE_NAME:-node}"
NETWORK="${NETWORK:-playground}"
SERVICES="${SERVICES:-}"
SHARED="${SHARED:-/shared}"

mkdir -p "${SHARED}/keys" "${SHARED}/tokens" "${SHARED}/devices"
if [ -n "${DEVICE_OF:-}" ]; then
    echo "${DEVICE_OF}" > "${SHARED}/devices/${NODE_NAME}"
    until [ -f "${SHARED}/keys/${NODE_NAME}.key" ]; do sleep 1; done
    bp login --alias "${NODE_NAME}" --ssh-key "${SHARED}/keys/${NODE_NAME}.key" >/dev/null
else
    bp login --alias "${NODE_NAME}" | tail -1 | sed 's/^ *//' > "${SHARED}/keys/${NODE_NAME}.pub"
fi

bp --daemon &
DAEMON_PID=$!
for _ in $(seq 1 30); do
    [ -S "${HOME}/.local/share/billpouch/control.sock" ] && break
    sleep 0.2
done

if [ "${FOUNDER:-false}" = "true" ]; then
    bp network create "${NETWORK}" || true
    (
        while true; do
            for key in "${SHARED}"/keys/*.pub; do
                name=$(basename "${key}" .pub)
                token="${SHARED}/tokens/${name}.token"
                if [ "${name}" != "${NODE_NAME}" ] && [ ! -f "${token}" ]; then
                    bp invite "${NETWORK}" "$(cat "${key}")" > "${token}.tmp" && mv "${token}.tmp" "${token}"
                fi
            done
            for request in "${SHARED}"/devices/*; do
                [ -f "${request}" ] || continue
                device=$(basename "${request}")
                token="${SHARED}/tokens/${device}.token"
                if [ "$(cat "${request}")" = "${NODE_NAME}" ] && [ ! -f "${token}" ]; then
                    cp "${HOME}/.local/share/billpouch/user_ed25519" "${SHARED}/keys/${device}.key"
                    bp invite "${NETWORK}" "$(cat "${SHARED}/keys/${NODE_NAME}.pub")" > "${token}.tmp" && mv "${token}.tmp" "${token}"
                fi
            done
            sleep 1
        done
    ) &
else
    until [ -f "${SHARED}/tokens/${NODE_NAME}.token" ]; do sleep 1; done
    bp join "$(cat "${SHARED}/tokens/${NODE_NAME}.token")"
fi
touch "${HOME}/.joined"
echo "[${NODE_NAME}] member of '${NETWORK}'"

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

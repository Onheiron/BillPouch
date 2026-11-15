#!/usr/bin/env bash
set -euo pipefail

NODE_NAME="${NODE_NAME:-node}"
NETWORK="${NETWORK:-playground}"
SERVICES="${SERVICES:-}"
SHARED="${SHARED:-/shared}"

mkdir -p "${SHARED}/keys" "${SHARED}/tokens" "${SHARED}/devices"
PUBLIC_KEY=$(bp login --alias "${NODE_NAME}" | tail -1 | sed 's/^ *//')
if [ -n "${DEVICE_OF:-}" ]; then
    echo "${PUBLIC_KEY}" > "${SHARED}/devices/${NODE_NAME}.pub"
    echo "${DEVICE_OF}" > "${SHARED}/devices/${NODE_NAME}.owner"
else
    echo "${PUBLIC_KEY}" > "${SHARED}/keys/${NODE_NAME}.pub"
    bp --daemon &
    for _ in $(seq 1 30); do
        [ -S "${XDG_RUNTIME_DIR}/billpouch/control.sock" ] && break
        sleep 0.2
    done
fi

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
            for request in "${SHARED}"/devices/*.owner; do
                [ -f "${request}" ] || continue
                device=$(basename "${request}" .owner)
                token="${SHARED}/tokens/${device}.token"
                if [ "$(cat "${request}")" = "${NODE_NAME}" ] && [ ! -f "${token}" ]; then
                    bp device add "${NETWORK}" "$(cat "${SHARED}/devices/${device}.pub")" > "${token}.tmp" && mv "${token}.tmp" "${token}"
                fi
            done
            sleep 1
        done
    ) &
elif ! bp members "${NETWORK}" >/dev/null 2>&1; then
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
            bp hatch pouch --network "${NETWORK}" --tier "${STORAGE_TIER:-T1}" ${STORAGE_SIZE:+--size "${STORAGE_SIZE}"} || true
        else
            bp hatch "${svc}" --network "${NETWORK}" || true
        fi
    done
fi

if [ "${INTERACTIVE:-false}" = "true" ]; then
    exec bash
fi
exec sleep infinity

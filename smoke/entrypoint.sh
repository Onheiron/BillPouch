#!/usr/bin/env bash
set -euo pipefail

# ── Args from environment ────────────────────────────────────────────────────
NODE_NAME="${NODE_NAME:-node}"

echo "=== BillPouch node: ${NODE_NAME} ==="

# ── Create identity ──────────────────────────────────────────────────────────
bp login --alias "${NODE_NAME}"
echo "[${NODE_NAME}] Identity created."

# ── Start daemon (blocks — this IS the main process) ────────────────────────
echo "[${NODE_NAME}] Starting daemon..."
exec bp --daemon

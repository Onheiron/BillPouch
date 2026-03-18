#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# BillPouch Playground — interactive P2P network simulator
# ═══════════════════════════════════════════════════════════════════════════════
set -euo pipefail

COMPOSE="docker compose -f docker-compose.playground.yml"
USERS=("carlo" "marco" "lucia" "elena" "paolo")

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

banner() {
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║${NC}  ${BOLD}BillPouch Playground${NC} — P2P network simulator               ${CYAN}║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

usage() {
    banner
    echo -e "  ${BOLD}Usage:${NC}  ./playground.sh <command>"
    echo ""
    echo -e "  ${GREEN}up${NC}              Build and start the network (5 users + you)"
    echo -e "  ${GREEN}down${NC}            Stop and remove all containers"
    echo -e "  ${GREEN}enter${NC} [user]     Enter a node interactively (default: you)"
    echo -e "  ${GREEN}status${NC}          Show what each node sees (flock view)"
    echo -e "  ${GREEN}logs${NC} [user]      Show logs for a node (default: all)"
    echo -e "  ${GREEN}network${NC}         Show network topology overview"
    echo -e "  ${GREEN}exec${NC} <user> <cmd>  Run a bp command on a user's node"
    echo ""
    echo -e "  ${BOLD}Users:${NC} carlo (pouch+bill), marco (post+pouch),"
    echo -e "         lucia (bill+post), elena (pouch×2), paolo (bill)"
    echo -e "         me (you — interactive)"
    echo ""
    echo -e "  ${BOLD}Examples:${NC}"
    echo -e "    ./playground.sh up                     # start the network"
    echo -e "    ./playground.sh enter                  # join as yourself"
    echo -e "    ./playground.sh enter carlo             # see carlo's view"
    echo -e "    ./playground.sh exec marco bp flock     # run flock on marco"
    echo -e "    ./playground.sh status                 # network overview"
    echo -e "    ./playground.sh down                   # tear down"
    echo ""
}

cmd_up() {
    banner
    echo -e "${YELLOW}▸ Building BillPouch binary (this takes a few minutes the first time)...${NC}"
    ${COMPOSE} build
    echo ""
    echo -e "${YELLOW}▸ Starting 5 simulated users + your interactive node...${NC}"
    ${COMPOSE} up -d
    echo ""
    echo -e "${GREEN}Network is starting!${NC}"
    echo ""
    echo "  Users coming online:"
    echo "    carlo  — pouch + bill  (storage + personal I/O)"
    echo "    marco  — post + pouch  (relay + storage)"
    echo "    lucia  — bill + post   (personal I/O + relay)"
    echo "    elena  — pouch × 2    (heavy storage)"
    echo "    paolo  — bill          (personal I/O)"
    echo "    me     — (you)         waiting for your commands"
    echo ""
    echo -e "  ${CYAN}Wait ~30s for peers to discover each other, then:${NC}"
    echo ""
    echo -e "    ${BOLD}./playground.sh enter${NC}        — join the network yourself"
    echo -e "    ${BOLD}./playground.sh status${NC}       — see what everyone sees"
    echo -e "    ${BOLD}./playground.sh network${NC}      — topology overview"
    echo ""
}

cmd_down() {
    echo -e "${YELLOW}▸ Tearing down playground...${NC}"
    ${COMPOSE} down -v
    echo -e "${GREEN}Done.${NC}"
}

cmd_enter() {
    local user="${1:-me}"
    local container="bp-${user}"

    # Check container exists and is running
    if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
        echo -e "${RED}Container '${container}' is not running.${NC}"
        echo "  Run './playground.sh up' first."
        exit 1
    fi

    if [ "${user}" = "me" ]; then
        echo -e "${CYAN}╔══════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${CYAN}║${NC}  Entering playground as ${BOLD}yourself${NC}                            ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}  Network: playground                                        ${CYAN}║${NC}"
        echo -e "${CYAN}╠══════════════════════════════════════════════════════════════╣${NC}"
        echo -e "${CYAN}║${NC}  Commands:                                                  ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}    bp flock                  — see the network              ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}    bp hatch pouch --network playground  — start storage     ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}    bp hatch bill  --network playground  — start file I/O    ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}    bp hatch post  --network playground  — start relay       ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}    bp farewell <id>          — stop a service               ${CYAN}║${NC}"
        echo -e "${CYAN}║${NC}    exit                      — leave (node keeps running)   ${CYAN}║${NC}"
        echo -e "${CYAN}╚══════════════════════════════════════════════════════════════╝${NC}"
        echo ""
        docker exec -it "${container}" bash
    else
        echo -e "${CYAN}Entering ${BOLD}${user}${NC}${CYAN}'s node...${NC}"
        echo -e "  (read-only peek — use bp flock, bp status, etc.)"
        echo ""
        docker exec -it "${container}" bash
    fi
}

cmd_status() {
    banner
    echo -e "${BOLD}Network Status — what each node sees:${NC}"
    echo ""

    ALL_NODES=("${USERS[@]}" "me")
    for user in "${ALL_NODES[@]}"; do
        local container="bp-${user}"
        if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
            echo -e "  ${RED}✗ ${user}${NC} — not running"
            continue
        fi

        echo -e "${YELLOW}━━━ ${BOLD}${user}${NC}${YELLOW} ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
        docker exec "${container}" bp flock 2>&1 || echo "  (daemon not ready)"
        echo ""
    done
}

cmd_logs() {
    local user="${1:-}"
    if [ -n "${user}" ]; then
        echo -e "${CYAN}Logs for ${BOLD}${user}${NC}:"
        docker logs "bp-${user}" 2>&1 | tail -30
    else
        echo -e "${CYAN}Logs for all nodes (last 10 lines each):${NC}"
        echo ""
        ALL_NODES=("${USERS[@]}" "me")
        for user in "${ALL_NODES[@]}"; do
            echo -e "${YELLOW}━━━ ${user} ━━━${NC}"
            docker logs "bp-${user}" 2>&1 | tail -10
            echo ""
        done
    fi
}

cmd_network() {
    banner
    echo -e "${BOLD}Network Topology:${NC}"
    echo ""
    echo "  ┌─────────────────────────────────────────────────────┐"
    echo "  │              BillPouch Playground Network            │"
    echo "  │                  network: playground                 │"
    echo "  │                                                     │"
    echo "  │   carlo ──── [pouch] [bill]                         │"
    echo "  │      │                                              │"
    echo "  │   marco ──── [post]  [pouch]    ← relay + storage   │"
    echo "  │      │                                              │"
    echo "  │   lucia ──── [bill]  [post]     ← I/O + relay       │"
    echo "  │      │                                              │"
    echo "  │   elena ──── [pouch] [pouch]    ← heavy storage     │"
    echo "  │      │                                              │"
    echo "  │   paolo ──── [bill]             ← file I/O only     │"
    echo "  │      │                                              │"
    echo "  │   ★ me ──── (you)              ← interactive        │"
    echo "  │                                                     │"
    echo "  │   All connected via mDNS + gossipsub on bridge net  │"
    echo "  └─────────────────────────────────────────────────────┘"
    echo ""

    # Live peer counts
    echo -e "${BOLD}Live peer counts:${NC}"
    ALL_NODES=("${USERS[@]}" "me")
    for user in "${ALL_NODES[@]}"; do
        local container="bp-${user}"
        if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
            echo -e "  ${RED}✗${NC} ${user}: not running"
            continue
        fi
        FLOCK=$(docker exec "${container}" bp flock 2>&1) || true
        PEER_COUNT=$(echo "${FLOCK}" | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "?")
        SVC_COUNT=$(echo "${FLOCK}" | grep -oP 'Local Services\s+\(\K[0-9]+' || echo "?")
        echo -e "  ${GREEN}●${NC} ${user}: ${SVC_COUNT} service(s), ${PEER_COUNT} known peer(s)"
    done
    echo ""
}

cmd_exec() {
    local user="$1"
    shift
    docker exec "bp-${user}" "$@"
}

# ── Main ─────────────────────────────────────────────────────────────────────
case "${1:-help}" in
    up)       cmd_up ;;
    down)     cmd_down ;;
    enter)    cmd_enter "${2:-me}" ;;
    status)   cmd_status ;;
    logs)     cmd_logs "${2:-}" ;;
    network)  cmd_network ;;
    exec)     shift; cmd_exec "$@" ;;
    help|-h|--help) usage ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        usage
        exit 1
        ;;
esac

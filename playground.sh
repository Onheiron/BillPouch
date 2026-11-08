#!/usr/bin/env bash
set -euo pipefail

COMPOSE="docker compose -f docker-compose.playground.yml"
NODES=(carlo marco lucia elena paolo me)

case "${1:-help}" in
    up)     docker build -t billpouch:local . && $COMPOSE up -d
            echo "Started. Wait ~30 s for discovery, then: ./playground.sh enter" ;;
    down)   $COMPOSE down -v ;;
    enter)  docker exec -it "bp-${2:-me}" bash ;;
    exec)   node="$2"; shift 2; docker exec "bp-${node}" "$@" ;;
    logs)   if [ -n "${2:-}" ]; then docker logs "bp-$2" 2>&1 | tail -50; else $COMPOSE logs --tail 10; fi ;;
    app)    url=$(docker exec bp-me bp dashboard --host 0.0.0.0 --no-open | grep -o 'https://[^ ]*')
            echo "$url"
            if command -v open >/dev/null; then open "$url"; elif command -v xdg-open >/dev/null; then xdg-open "$url"; fi ;;
    status) for n in "${NODES[@]}"; do
                echo "━━━ ${n} ━━━"
                docker exec "bp-${n}" bp status 2>&1 || echo "  (not running)"
            done ;;
    *)      cat <<USAGE
Usage: ./playground.sh <command>
  up                 build the image from this checkout and start the network
  down               stop and remove everything
  enter [node]       shell into a node (default: me)
  exec <node> <cmd>  run a command on a node, e.g. exec carlo bp flock
  logs [node]        show logs (default: all nodes)
  app                open the web app of your node (me) in the browser
  status             bp status on every node
Nodes: ${NODES[*]}
USAGE
            ;;
esac

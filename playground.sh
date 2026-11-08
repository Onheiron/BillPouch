#!/usr/bin/env bash
set -euo pipefail

COMPOSE="docker compose -f docker-compose.playground.yml"
NODES=(carlo marco lucia elena paolo me)

port_of() {
    case "$1" in
        carlo) echo 3001 ;; marco) echo 3002 ;; lucia) echo 3003 ;;
        elena) echo 3004 ;; paolo) echo 3005 ;; me) echo 3006 ;;
        *) echo "unknown node: $1" >&2; exit 1 ;;
    esac
}

case "${1:-help}" in
    up)     mkdir -p playground/data
            docker build -t billpouch:local .
            $COMPOSE up -d
            echo "Started. Wait ~30 s for discovery, then: ./playground.sh enter" ;;
    down)   $COMPOSE down ;;
    reset)  $COMPOSE down
            docker run --rm --entrypoint sh -v "$PWD/playground:/playground" billpouch:local -c 'rm -rf /playground/data' ;;
    enter)  docker exec -it "bp-${2:-me}" bash ;;
    exec)   node="$2"; shift 2; docker exec "bp-${node}" "$@" ;;
    logs)   if [ -n "${2:-}" ]; then docker logs "bp-$2" 2>&1 | tail -50; else $COMPOSE logs --tail 10; fi ;;
    app)    target="${2:-me}"
            if [ "$target" = all ]; then targets=("${NODES[@]}"); else targets=("$target"); fi
            for node in "${targets[@]}"; do
                port=$(port_of "$node")
                url=$(docker exec "bp-${node}" bp dashboard --host 0.0.0.0 --no-open | grep -o 'https://[^ ]*' | sed "s#:3000/#:${port}/#")
                echo "${node}: ${url}"
                if [ "$target" != all ]; then
                    if command -v open >/dev/null; then open "$url"; elif command -v xdg-open >/dev/null; then xdg-open "$url"; fi
                fi
            done ;;
    status) for n in "${NODES[@]}"; do
                echo "━━━ ${n} ━━━"
                docker exec "bp-${n}" bp status 2>&1 || echo "  (not running)"
            done ;;
    *)      cat <<USAGE
Usage: ./playground.sh <command>
  up                 build the image from this checkout and start the network
  down               stop the nodes; their data stays in playground/data
  reset              stop the nodes and delete their data
  enter [node]       shell into a node (default: me)
  exec <node> <cmd>  run a command on a node, e.g. exec carlo bp flock
  logs [node]        show logs (default: all nodes)
  app [node|all]     open a node's web app (default: me; all prints every address)
  status             bp status on every node
Nodes: ${NODES[*]}
USAGE
            ;;
esac

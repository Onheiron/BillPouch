#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo -e "  ${GREEN}✓${NC} $1"; }
fail() { FAIL=$((FAIL + 1)); echo -e "  ${RED}✗${NC} $1"; }
info() { echo -e "${CYAN}▸${NC} $1"; }
header() { echo -e "\n${YELLOW}═══ $1 ═══${NC}"; }

CONTAINERS=("bp-pouch" "bp-bill" "bp-post")

header "1. Waiting for daemons to be ready"
for c in "${CONTAINERS[@]}"; do
    info "Checking ${c}..."
    READY=false
    for i in $(seq 1 60); do
        RESP=$(docker exec "${c}" bash -c \
            'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null' \
        ) || true
        if echo "${RESP}" | grep -q "pong"; then
            pass "${c} daemon is responding"
            READY=true
            break
        fi
        sleep 0.5
    done
    if [ "${READY}" = false ]; then
        fail "${c} daemon never became ready (timeout 30s)"
    fi
done

header "1b. Waiting for every node to join the network through an invite"
for c in "${CONTAINERS[@]}"; do
    if timeout 60 bash -c "until docker exec ${c} test -f /root/.joined; do sleep 1; done"; then
        pass "${c} joined via invite"
    else
        fail "${c} never joined (no invite token after 60s)"
    fi
done

if timeout 90 bash -c "until docker exec bp-laptop test -f /root/.joined; do sleep 1; done"; then
    pass "bp-laptop joined as a second device of carlo"
else
    fail "bp-laptop never joined as a second device of carlo"
fi

header "2. Waiting for mDNS discovery + gossipsub mesh formation"
info "Sleeping 20s for peers to discover each other and form gossipsub mesh..."
sleep 20

header "3. Hatching services on each node"

hatch_service() {
    local container="$1"
    local svc_type="$2"
    info "Hatching '${svc_type}' on ${container}..."
    local OUT
    local extra=()
    [ "${svc_type}" = "pouch" ] && extra=(--tier T1)
    OUT=$(docker exec "${container}" bp hatch "${svc_type}" --network smoketest "${extra[@]}" 2>&1) || true
    if echo "${OUT}" | grep -qi "hatched\|service_id\|running"; then
        pass "${container} hatched '${svc_type}' successfully"
    else
        fail "${container} failed to hatch '${svc_type}': ${OUT}"
    fi
}

hatch_service "bp-pouch" "pouch"
hatch_service "bp-bill"  "bill"
hatch_service "bp-post"  "post"

header "4. Waiting for gossipsub to propagate NodeInfo"
info "Sleeping 15s for gossipsub announcements to reach all peers..."
sleep 15

header "5. Peer discovery verification"
for c in "${CONTAINERS[@]}"; do
    info "${c} flock output:"
    FLOCK_OUTPUT=$(docker exec "${c}" bp flock 2>&1) || true
    echo "${FLOCK_OUTPUT}"
    echo ""

    PEER_COUNT=$(echo "${FLOCK_OUTPUT}" | grep -oP 'Known Peers\s+\(\K[0-9]+' || echo "0")

    if [ "${PEER_COUNT}" -ge 2 ]; then
        pass "${c} sees ${PEER_COUNT} peers (expected >= 2)"
    else
        fail "${c} sees only ${PEER_COUNT} peers (expected >= 2)"
    fi
done

header "6. Service type cross-visibility"

check_service_visible() {
    local from_node="$1"
    local service_type="$2"

    local flock_out
    flock_out=$(docker exec "${from_node}" bp flock 2>&1) || true

    local peers_section
    peers_section=$(echo "${flock_out}" | sed -n '/Known Peers/,$ p' | tail -n +2) || true

    if echo "${peers_section}" | grep -qi "${service_type}"; then
        pass "${from_node} sees a '${service_type}' peer"
    else
        fail "${from_node} does NOT see a '${service_type}' peer"
    fi
}

check_service_visible "bp-pouch" "bill"
check_service_visible "bp-pouch" "post"
check_service_visible "bp-bill" "pouch"
check_service_visible "bp-bill" "post"
check_service_visible "bp-post" "pouch"
check_service_visible "bp-post" "bill"

header "7. Health check (ping/pong)"
for c in "${CONTAINERS[@]}"; do
    PING_OUT=$(docker exec "${c}" bash -c \
        'echo "{\"cmd\":\"ping\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null' \
    ) || true

    if echo "${PING_OUT}" | grep -qi "pong"; then
        pass "${c} ping → pong"
    else
        fail "${c} daemon is NOT responding to ping"
    fi
done

header "8. Network membership"
for c in "${CONTAINERS[@]}"; do
    FLOCK_OUT=$(docker exec "${c}" bp flock 2>&1) || true

    if echo "${FLOCK_OUT}" | grep -q "smoketest"; then
        pass "${c} is on network 'smoketest'"
    else
        fail "${c} is NOT on network 'smoketest'"
    fi
done

header "9. Encrypted files and sharing"
docker exec bp-pouch bash -c 'head -c 9437184 /dev/urandom > /tmp/foto.bin'
PUT_OUT=$(docker exec bp-pouch bp put /tmp/foto.bin --network smoketest 2>&1) || true
CHUNK=$(echo "${PUT_OUT}" | grep -oP 'file_id\s*:\s*\K\S+' || true)
if [ -n "${CHUNK}" ] && echo "${PUT_OUT}" | grep -q "in 3 chunk(s)"; then
    pass "carlo-pouch stored a 9 MiB encrypted file in 3 chunks (${CHUNK})"
else
    fail "bp put failed: ${PUT_OUT}"
fi

BILL_FP=$(docker exec bp-bill bp status 2>/dev/null | grep -oP 'fingerprint\s*:\s*\K\S+' || true)
GET_BEFORE=$(docker exec bp-bill bp get "${CHUNK}" /tmp/foto.bin --network smoketest 2>&1) || true
if echo "${GET_BEFORE}" | grep -q "not one of your files"; then
    pass "marco-bill cannot read the file before it is shared"
else
    fail "marco-bill read an unshared file: ${GET_BEFORE}"
fi

SHARE_OUT=$(docker exec bp-pouch bp share "${CHUNK}" "${BILL_FP}" --network smoketest 2>&1) || true
if echo "${SHARE_OUT}" | grep -q "now shared"; then
    pass "carlo-pouch shared the file with marco-bill"
else
    fail "bp share failed: ${SHARE_OUT}"
fi

info "Waiting up to 40s for the share to reach marco-bill..."
for _ in $(seq 1 20); do
    docker exec bp-bill bp ls --network smoketest 2>/dev/null | grep -q "foto.bin" && break
    sleep 2
done
if docker exec bp-bill bp ls --network smoketest 2>&1 | grep -q "foto.bin"; then
    pass "the shared file shows up in marco-bill's bp ls"
else
    fail "the shared file never showed up in marco-bill's bp ls"
fi

docker exec bp-bill bp get "${CHUNK}" /tmp/foto.bin --network smoketest >/dev/null 2>&1 || true
SUM_A=$(docker exec bp-pouch sha256sum /tmp/foto.bin | cut -d' ' -f1)
SUM_B=$(docker exec bp-bill sha256sum /tmp/foto.bin 2>/dev/null | cut -d' ' -f1 || true)
if [ "${SUM_A}" = "${SUM_B}" ]; then
    pass "marco-bill downloaded and decrypted the shared file byte for byte"
else
    fail "marco-bill's copy differs (${SUM_A} vs ${SUM_B})"
fi

info "Waiting up to 60s for carlo's file index to reach carlo-laptop..."
for _ in $(seq 1 30); do
    docker exec bp-laptop bp ls --network smoketest 2>/dev/null | grep -q "foto.bin" && break
    sleep 2
done
if docker exec bp-laptop bp ls --network smoketest 2>&1 | grep -q "foto.bin"; then
    pass "carlo-laptop sees carlo's file through the synced index"
else
    fail "carlo-laptop never saw carlo's file"
fi
docker exec bp-laptop bp get "${CHUNK}" /tmp/foto.bin --network smoketest >/dev/null 2>&1 || true
SUM_C=$(docker exec bp-laptop sha256sum /tmp/foto.bin 2>/dev/null | cut -d' ' -f1 || true)
if [ "${SUM_A}" = "${SUM_C}" ]; then
    pass "carlo-laptop downloaded and decrypted carlo's file"
else
    fail "carlo-laptop's copy differs (${SUM_A} vs ${SUM_C})"
fi

LAPTOP_POUCH=$(docker exec bp-laptop bp hatch pouch --network smoketest --tier T1 2>&1 | grep -oP 'Service ID\s*:\s*\K\S+' || true)
info "Waiting up to 150s for carlo-pouch to fill the new Pouch on carlo-laptop..."
FILLED=false
for _ in $(seq 1 75); do
    if grep -q "filled free space" <<< "$(docker logs bp-pouch 2>&1)"; then
        FILLED=true
        break
    fi
    sleep 2
done
if [ "${FILLED}" = true ]; then
    pass "carlo-pouch filled the free space of the new Pouch with random data"
else
    fail "carlo-pouch never filled the new Pouch"
fi
FILLERS=$(docker exec bp-laptop bash -c 'ls ~/.local/share/billpouch/storage/*/*/fillers 2>/dev/null | wc -l' || echo 0)
if [ "${FILLERS}" -gt 0 ]; then
    pass "carlo-laptop's Pouch holds ${FILLERS} filler blob(s)"
else
    fail "carlo-laptop's Pouch holds no filler"
fi

info "Waiting up to 150s for the laptop's new Pouch to catch up on the existing files..."
CAUGHT_UP=false
for _ in $(seq 1 75); do
    if grep -q "received a recoded fragment" <<< "$(docker logs bp-laptop 2>&1)"; then
        CAUGHT_UP=true
        break
    fi
    sleep 2
done
if [ "${CAUGHT_UP}" = true ]; then
    pass "the laptop's new Pouch received recoded fragments of the files stored before it joined"
else
    fail "the laptop's new Pouch never caught up"
fi
FRAGS=$(docker exec bp-laptop bash -c 'find ~/.local/share/billpouch/storage -name "*.frag" | wc -l' || echo 0)
if [ "${FRAGS}" -gt 0 ]; then
    pass "carlo-laptop's Pouch holds ${FRAGS} recoded fragment(s)"
else
    fail "carlo-laptop's Pouch holds no fragment"
fi

UNSHARE_OUT=$(docker exec bp-pouch bp unshare "${CHUNK}" "${BILL_FP}" --network smoketest 2>&1) || true
if echo "${UNSHARE_OUT}" | grep -q "can no longer fetch"; then
    pass "carlo-pouch revoked marco-bill's access"
else
    fail "bp unshare failed: ${UNSHARE_OUT}"
fi
sleep 5
REVOKED=$(docker exec bp-bill bp get "${CHUNK}" /tmp/again.bin --network smoketest 2>&1) || true
if echo "${REVOKED}" | grep -q "not one of your files"; then
    pass "after the revocation marco-bill can no longer get the file"
else
    fail "marco-bill still reads a revoked file: ${REVOKED}"
fi

docker exec bp-pouch bash -c 'echo temporaneo > /tmp/tmp.txt'
TMP_ID=$(docker exec bp-pouch bp put /tmp/tmp.txt --network smoketest 2>&1 | grep -oP 'file_id\s*:\s*\K\S+' || true)
RM_OUT=$(docker exec bp-pouch bp rm "${TMP_ID}" --network smoketest 2>&1) || true
if echo "${RM_OUT}" | grep -q "deleted" && ! docker exec bp-pouch bp ls --network smoketest 2>&1 | grep -q "tmp.txt"; then
    pass "carlo-pouch deleted a file"
else
    fail "bp rm failed: ${RM_OUT}"
fi

header "10. Expulsion and key rotation"
POST_FP=$(docker exec bp-post bp status 2>/dev/null | grep -oP 'fingerprint\s*:\s*\K\S+' || true)
EXPEL_OUT=$(docker exec bp-pouch bp expel smoketest "${POST_FP}" 2>&1) || true
echo "${EXPEL_OUT}"
if echo "${EXPEL_OUT}" | grep -q "expelled"; then
    pass "carlo-pouch expelled lucia-post"
else
    fail "expulsion was not committed: ${EXPEL_OUT}"
fi

info "Sleeping 20s for the expulsion and the key rotation to reach every node..."
sleep 20

MEMBERS_OUT=$(docker exec bp-bill bp members smoketest 2>&1) || true
echo "${MEMBERS_OUT}"
if echo "${MEMBERS_OUT}" | grep -q "${POST_FP}  expelled"; then
    pass "marco-bill's ledger lists lucia-post as expelled"
else
    fail "marco-bill's ledger does not list lucia-post as expelled"
fi

for c in bp-pouch bp-bill; do
    if grep -q "Moved to the rotated gossip topic" <<< "$(docker logs "${c}" 2>&1)"; then
        pass "${c} moved to the rotated gossip topic"
    else
        fail "${c} did not move to the rotated gossip topic"
    fi
done

if grep -q "Moved to the rotated gossip topic" <<< "$(docker logs bp-post 2>&1)"; then
    fail "the expelled lucia-post got the rotated key"
else
    pass "the expelled lucia-post did not get the rotated key"
fi

ROTATED_AT=$(date +%s)
info "Sleeping 40s for marco-bill to re-announce on the rotated topic..."
sleep 40
FLOCK_JSON=$(docker exec bp-pouch bash -c \
    'echo "{\"cmd\":\"flock\"}" | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock 2>/dev/null') || true
LATEST=$(echo "${FLOCK_JSON}" | grep -oP '"announced_at":\K[0-9]+' | sort -n | tail -1)
if [ -n "${LATEST}" ] && [ "${LATEST}" -ge "${ROTATED_AT}" ]; then
    pass "carlo-pouch still hears marco-bill after the rotation"
else
    fail "carlo-pouch heard nothing after the rotation (latest announcement ${LATEST}, rotation ${ROTATED_AT})"
fi

LEAVE_OUT=$(docker exec bp-bill bp leave smoketest --force 2>&1) || true
echo "${LEAVE_OUT}"
sleep 5
if echo "${LEAVE_OUT}" | grep -q "Left network" \
    && ! grep -q "${BILL_FP}" <<< "$(docker exec bp-pouch bp members smoketest 2>&1)"; then
    pass "marco-bill left the network and carlo's ledger no longer lists him"
else
    fail "marco-bill's leave did not reach the ledger"
fi

header "11. Node logs (last 20 lines each)"
for c in "${CONTAINERS[@]}"; do
    info "--- ${c} ---"
    docker logs "${c}" 2>&1 | tail -20
    echo ""
done

header "RESULTS"
TOTAL=$((PASS + FAIL))
echo -e "  ${GREEN}Passed: ${PASS}${NC} / ${TOTAL}"
if [ "${FAIL}" -gt 0 ]; then
    echo -e "  ${RED}Failed: ${FAIL}${NC} / ${TOTAL}"
    echo ""
    echo -e "${RED}SMOKE TEST FAILED${NC}"
    exit 1
else
    echo ""
    echo -e "${GREEN}ALL SMOKE TESTS PASSED${NC}"
    exit 0
fi

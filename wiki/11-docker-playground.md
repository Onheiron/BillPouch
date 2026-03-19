# 11. Docker & Playground

## Dockerfile (smoke)

Usato dai nodi del smoke test. Multi-stage build:

```dockerfile
# Stage 1 — builder
FROM rust:1.85-bookworm AS builder
COPY . .
RUN cargo build --release --bin bp

# Stage 2 — runtime
FROM debian:bookworm-slim
RUN apt-get install -y ca-certificates socat
COPY --from=builder /app/target/release/bp /usr/local/bin/bp
COPY smoke/entrypoint.sh /entrypoint.sh
ENV RUST_LOG=bp_core=debug,bp_cli=debug
ENTRYPOINT ["/entrypoint.sh"]
```

> Rust 1.85 è richiesto per il supporto edition 2024 nelle dipendenze di libp2p.

---

## Dockerfile.playground

Usato dai nodi del playground interattivo. Aggiunge `bash` per la shell interattiva:

```dockerfile
# Stage 2 differenze rispetto a Dockerfile:
RUN apt-get install -y ca-certificates socat bash
COPY playground/entrypoint.sh /entrypoint.sh
ENV RUST_LOG=bp_core=info,bp_cli=info
```

---

## Smoke Test Cluster

File: [docker-compose.smoke.yml](../docker-compose.smoke.yml)

Rete Docker `bp-lan` (bridge isolata) con 3 nodi:

| Container  | Hostname   | `NODE_NAME`   | `NETWORK`   | Servizio |
|------------|------------|---------------|-------------|----------|
| `bp-pouch` | `bp-pouch` | `carlo-pouch` | `smoketest` | pouch    |
| `bp-bill`  | `bp-bill`  | `marco-bill`  | `smoketest` | bill     |
| `bp-post`  | `bp-post`  | `lucia-post`  | `smoketest` | post     |

### Comandi

```bash
# Avvio
docker compose -f docker-compose.smoke.yml up --build -d

# Esegui smoke test
./smoke/smoke-test.sh

# Vedi log di un nodo
docker compose -f docker-compose.smoke.yml logs bp-pouch

# Stop e cleanup
docker compose -f docker-compose.smoke.yml down
```

### Entrypoint smoke (`smoke/entrypoint.sh`)

1. `bp login --alias ${NODE_NAME}`
2. `bp --daemon &` — avvia daemon in background
3. Attende `control.sock` (polling ogni 200ms, max 30 iterazioni)
4. `bp join ${NETWORK}` — entra nella rete smoketest
5. `sleep 5` — attende formazione mesh
6. `bp hatch ${SERVICE_TYPE} --network ${NETWORK}` — avvia servizio
7. `wait ${DAEMON_PID}` — mantiene il container vivo

---

## Playground

File: [docker-compose.playground.yml](../docker-compose.playground.yml) +
[playground.sh](../playground.sh)

Simula una rete P2P con **5 utenti pre-configurati + 1 nodo interattivo (tu)**.

### Nodi del playground

| Container  | `NODE_NAME` | `SERVICES`    | Descrizione                    |
|------------|-------------|---------------|--------------------------------|
| `bp-carlo` | `carlo`     | `pouch,bill`  | Storage + file I/O             |
| `bp-marco` | `marco`     | `post,pouch`  | Relay + storage                |
| `bp-lucia` | `lucia`     | `bill,post`   | File I/O + relay               |
| `bp-elena` | `elena`     | `pouch,pouch` | Heavy storage (doppio pouch)   |
| `bp-paolo` | `paolo`     | `bill`        | Solo file I/O                  |
| `bp-me`    | `me`        | —             | Nodo interattivo (tu)          |

Rete Docker: `bp-playground` (bridge)

### Comandi playground.sh

```bash
./playground.sh up            # Avvia la rete completa (background)
./playground.sh enter         # Entra nel tuo nodo interattivo (bp-me)
./playground.sh enter carlo   # Entra nel nodo di carlo
./playground.sh status        # Mostra lo stato di ogni nodo
./playground.sh down          # Ferma e rimuove tutti i container
```

### Entrypoint playground (modalità interattiva)

Quando `INTERACTIVE=true` (nodo `bp-me`):
1. `bp login --alias ${NODE_NAME}`
2. `bp --daemon &`
3. Attende `control.sock`
4. `bp join ${NETWORK}` — si unisce subito alla rete
5. Mostra banner con comandi disponibili
6. `exec bash` — shell interattiva per l'utente

### Entrypoint playground (modalità bot)

Per i nodi pre-configurati:
1. `bp login --alias ${NODE_NAME}`
2. `bp --daemon &`
3. Attende `control.sock`
4. `bp join ${NETWORK}`
5. `sleep 5` — attesa formazione mesh
6. Per ogni servizio in `SERVICES` (comma-separated): `bp hatch ${svc} --network ${NETWORK}`
7. `wait ${DAEMON_PID}` — mantiene il container vivo

### Sessione di esempio nel playground

```bash
./playground.sh up
./playground.sh enter

# Nel container bp-me:
bp flock
# → Vedi carlo, marco, lucia, elena, paolo (5 peer)

bp hatch pouch --network playground
# → service_id: abc123...

bp hatch bill --network playground
# → service_id: def456...

bp flock
# → Ora sei visibile anche tu nella rete

./playground.sh status    # In un altro terminale
./playground.sh down
```

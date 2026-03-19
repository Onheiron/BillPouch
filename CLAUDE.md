# BillPouch — Claude Instructions

## Progetto

BillPouch è un **filesystem distribuito P2P sociale** scritto in Rust (edition 2021).
È un Cargo workspace con due crate:

- **`bp-core`** — libreria pura (no I/O diretto, no stdout, no `process::exit`)
- **`bp-cli`** — binario `bp`, thin client CLI che parla col daemon via Unix socket

Documentazione completa nella cartella `wiki/`.

---

## Struttura crate

```
crates/
  bp-core/src/
    identity.rs       # Ed25519 keypair, fingerprint = hex(SHA256(pubkey))[0..8]
    service.rs        # ServiceType (Pouch|Bill|Post), ServiceInfo, ServiceRegistry
    config.rs         # Percorsi XDG con crate `directories`
    error.rs          # BpError (thiserror) + BpResult
    daemon.rs         # run_daemon(), is_running(), PID file
    network/
      behaviour.rs    # BillPouchBehaviour: gossipsub+kad+identify+mdns
      mod.rs          # NetworkCommand, build_swarm(), run_network_loop()
      state.rs        # NetworkState: upsert/evict_stale/in_network
    control/
      protocol.rs     # ControlRequest / ControlResponse (JSON newline-delimited)
      server.rs       # DaemonState (Arc), dispatch(), run_control_server()
  bp-cli/src/
    main.rs           # clap derive CLI
    client.rs         # ControlClient — Unix socket JSON client
    commands/         # auth.rs, hatch.rs, flock.rs, farewell.rs, join.rs
```

---

## Regole del progetto

### bp-core — libreria pura
- **Vietato:** `println!`, `eprintln!`, `std::process::exit`, qualsiasi I/O diretto
- Usa **`thiserror`** per `BpError`, esponi `BpResult<T> = Result<T, BpError>`
- Stato condiviso: `Arc<DaemonState>` con `RwLock<T>` per ogni campo mutabile
- Async con **Tokio** (`features = ["full"]`)
- Logging con **`tracing`** (mai `println!` per debug)

### bp-cli — thin client
- Usa **`anyhow`** per gli errori
- Non importa mai il swarm libp2p direttamente
- Tutta la comunicazione col daemon passa per `ControlClient` su Unix socket

### Convenzioni sui tipi
- `service_id` → sempre `String` (UUID v4 generato con `uuid::Uuid::new_v4()`)
- `user_fingerprint` → `String` immutabile: `hex(SHA-256(pubkey_bytes))[0..8]`
- `network_id` → `String` libero (es. `"amici"`, `"lavoro"`, `"public"`)
- Topic gossipsub → `format!("billpouch/v1/{}/nodes", network_id)`
- Socket → `~/.local/share/billpouch/control.sock`

---

## IPC: CLI ↔ Daemon

Trasporto: Unix domain socket, **JSON newline-delimited**, UTF-8.

```rust
pub enum ControlRequest {
    Ping,
    Status,
    Hatch    { service_type: ServiceType, network_id: String, metadata: HashMap<String, Value> },
    Flock,
    Farewell { service_id: String },
    Join     { network_id: String },
}

pub struct ControlResponse {
    pub status:  String,          // "ok" | "error"
    pub data:    Option<Value>,
    pub message: Option<String>,
}
```

---

## I tre servizi

| Tipo    | Simbolo    | Ruolo                                          |
|---------|------------|------------------------------------------------|
| `Pouch` | 🦤 sacca  | Offre storage locale alla rete                 |
| `Bill`  | 🦤 becco  | Interfaccia file I/O personale                 |
| `Post`  | 🦤 ali    | Relay puro — solo CPU e banda, nessun storage  |

---

## Stack libp2p

```rust
#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    pub gossipsub: gossipsub::Behaviour,           // broadcast NodeInfo
    pub kad:       kad::Behaviour<MemoryStore>,    // DHT peer discovery
    pub identify:  identify::Behaviour,            // /billpouch/id/1.0.0
    pub mdns:      mdns::tokio::Behaviour,         // LAN zero-config
}
// Trasporto: TCP + Noise (cifratura) + Yamux (mux)
```

---

## Comandi per sviluppo

```bash
cargo test --workspace                    # tutti i test
cargo test -p bp-core --test architecture_test  # solo test architettura
cargo clippy --workspace -- -D warnings   # linting (warnings = errori)
cargo fmt --all                           # formattazione

# Smoke test P2P (richiede Docker)
docker compose -f docker-compose.smoke.yml up --build -d
./smoke/smoke-test.sh

# Playground interattivo (5 nodi simulati)
./playground.sh up && ./playground.sh enter
```

---

## Stato del progetto

**v0.1.3 Alpha** — il layer di trasferimento file (chunking, cifratura, erasure coding)
non è ancora implementato. I servizi si annunciano via gossip ma non trasferiscono dati reali.

Per dettagli completi vedi `wiki/12-status-roadmap.md`.

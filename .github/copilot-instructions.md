# BillPouch — GitHub Copilot Instructions

## Progetto

BillPouch è un **filesystem distribuito P2P sociale** scritto in Rust (edition 2021).
È un Cargo workspace con due crate:

- **`bp-core`** — libreria pura (no I/O diretto, no stdout, no `process::exit`)
- **`bp-cli`** — binario `bp`, thin client CLI che parla col daemon via Unix socket

Documentazione completa nella cartella `wiki/`.

---

## Struttura

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

## Regole fondamentali

### bp-core
- **Mai** `println!`, `eprintln!`, `std::process::exit` o I/O diretto
- Error handling con **`thiserror`** → `BpError` + `BpResult`
- Tutto lo stato condiviso è `Arc<DaemonState>` con `RwLock` interni
- Async con **Tokio** (`features = ["full"]`)

### bp-cli
- Error handling con **`anyhow`**
- La CLI non tocca mai il swarm libp2p direttamente
- Comunica col daemon tramite `ControlClient` su Unix socket
- `bp hatch` auto-avvia il daemon se non in esecuzione

### Generale
- Usa **`tracing`** per il logging (mai `println!` per debug)
- I servizi: **Pouch** (storage), **Bill** (file I/O), **Post** (relay puro)
- Il `service_id` è sempre un UUID v4
- Il `user_fingerprint` è immutabile: `hex(SHA-256(pubkey))[0..8]`
- Topic gossipsub: `billpouch/v1/{network_id}/nodes`
- Socket path: `~/.local/share/billpouch/control.sock`

---

## Protocollo di controllo

Trasporto: Unix domain socket, JSON newline-delimited, UTF-8.

```rust
// Request
ControlRequest::Ping
ControlRequest::Status
ControlRequest::Hatch { service_type, network_id, metadata }
ControlRequest::Flock
ControlRequest::Farewell { service_id }
ControlRequest::Join { network_id }

// Response
ControlResponse { status: "ok"|"error", data: Option<Value>, message: Option<String> }
```

---

## Testing

- Test di architettura: `crates/bp-core/tests/architecture_test.rs`
- Integration test: `crates/bp-core/tests/integration_test.rs`
- Smoke test Docker: `smoke/smoke-test.sh` con `docker-compose.smoke.yml`
- Playground interattivo: `./playground.sh up && ./playground.sh enter`

Per i test che richiedono Tokio (es. swarm mDNS): usa `#[tokio::test]`.

---

## Dipendenze chiave

| Crate        | Uso                                    |
|--------------|----------------------------------------|
| libp2p 0.54  | Stack P2P (gossipsub, kad, mdns, noise)|
| tokio 1.x    | Runtime async                          |
| serde_json   | Protocollo controllo + NodeInfo        |
| uuid         | service_id (v4)                        |
| chrono       | Timestamp announced_at                 |
| sha2 + hex   | Calcolo fingerprint                    |
| clap 4.x     | CLI (derive)                           |
| thiserror    | Errori in bp-core                      |
| anyhow       | Errori in bp-cli                       |
| directories  | Percorsi XDG                           |

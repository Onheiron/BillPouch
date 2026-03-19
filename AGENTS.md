# BillPouch — Agent Instructions

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

## Regole fondamentali

### bp-core — libreria pura
- **Vietato:** `println!`, `eprintln!`, `std::process::exit`, I/O diretto
- Error handling con `thiserror` → `BpError` + `BpResult`
- Stato condiviso: `Arc<DaemonState>` con `RwLock` interni
- Async con Tokio (`features = ["full"]`)
- Logging con `tracing` (mai `println!` per debug)

### bp-cli
- Error handling con `anyhow`
- Non accede mai al swarm libp2p direttamente
- Comunica col daemon via `ControlClient` su Unix socket

### Convenzioni
- `service_id` → UUID v4 (`uuid::Uuid::new_v4().to_string()`)
- `user_fingerprint` → `hex(SHA-256(pubkey_bytes))[0..8]` — immutabile
- Topic gossipsub → `format!("billpouch/v1/{}/nodes", network_id)`
- Socket → `~/.local/share/billpouch/control.sock`
- Rete di default → `"public"`

---

## Protocollo di controllo

Trasporto: Unix domain socket, JSON newline-delimited, UTF-8.

**Request:**
```json
{"cmd":"ping"}
{"cmd":"status"}
{"cmd":"hatch","service_type":"pouch","network_id":"amici","metadata":{"storage_bytes":10737418240}}
{"cmd":"hatch","service_type":"bill","network_id":"amici","metadata":{"mount_path":"/mnt/files"}}
{"cmd":"hatch","service_type":"post","network_id":"amici","metadata":{}}
{"cmd":"flock"}
{"cmd":"farewell","service_id":"<uuid>"}
{"cmd":"join","network_id":"lavoro"}
```

**Response:**
```json
{"status":"ok","data":{ ... }}
{"status":"ok"}
{"status":"error","message":"<descrizione>"}
```

---

## I tre tipi di servizio

| Tipo    | CLI              | Ruolo                                         |
|---------|------------------|-----------------------------------------------|
| `pouch` | `bp hatch pouch` | Offre storage locale alla rete                |
| `bill`  | `bp hatch bill`  | Interfaccia file I/O personale                |
| `post`  | `bp hatch post`  | Relay puro (solo CPU + banda, nessun storage) |

---

## NodeInfo (messaggio gossip)

```json
{
  "peer_id":          "12D3KooW...",
  "user_fingerprint": "a3f19c2b",
  "user_alias":       "carlo",
  "service_type":     "pouch",
  "service_id":       "<uuid>",
  "network_id":       "amici",
  "listen_addrs":     ["/ip4/192.168.1.10/tcp/54321"],
  "announced_at":     1710000000,
  "metadata":         { "storage_bytes": 10737418240 }
}
```

---

## Comandi per sviluppo

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
./playground.sh up && ./playground.sh enter   # test interattivo
```

---

## Stato

**v0.1.3 Alpha.** Il layer di trasferimento file non è implementato.
Vedi `wiki/12-status-roadmap.md` per dettagli.

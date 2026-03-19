# 2. Architettura

## Struttura del workspace Cargo

```
BillPouch/
├── Cargo.toml                      # Cargo workspace (edition 2021, v0.1.0)
└── crates/
    ├── bp-core/                    # Libreria core (no I/O diretto)
    │   └── src/
    │       ├── lib.rs              # Punto di entrata: re-esporta tutti i moduli
    │       ├── identity.rs         # Gestione keypair Ed25519 — login/logout
    │       ├── service.rs          # enum ServiceType + ServiceInfo + ServiceRegistry
    │       ├── config.rs           # Percorsi config XDG-aware (directories crate)
    │       ├── error.rs            # Tipo BpError unificato + BpResult alias
    │       ├── daemon.rs           # Orchestratore daemon Tokio + PID file
    │       ├── network/
    │       │   ├── behaviour.rs    # #[derive(NetworkBehaviour)] BillPouchBehaviour
    │       │   ├── mod.rs          # Swarm loop + canale NetworkCommand (mpsc)
    │       │   └── state.rs        # NetworkState: store gossip NodeInfo (HashMap)
    │       └── control/
    │           ├── mod.rs          # Re-export
    │           ├── protocol.rs     # ControlRequest/Response JSON (CLI ↔ daemon)
    │           └── server.rs       # Unix socket server + DaemonState + dispatch
    └── bp-cli/                     # Binario `bp`
        └── src/
            ├── main.rs             # Entry point clap derive CLI
            ├── client.rs           # ControlClient — Unix socket thin client
            └── commands/
                ├── mod.rs
                ├── auth.rs         # login / logout
                ├── hatch.rs        # hatch (+ auto-avvio daemon)
                ├── flock.rs        # flock
                ├── farewell.rs     # farewell
                └── join.rs         # join
```

## Principio di separazione bp-core / bp-cli

`bp-core` è una **libreria Rust pura**:
- Nessuna chiamata `std::process::exit` né I/O diretto al terminale
- Progettata per essere incorporata in qualsiasi adapter (REST, gRPC, WASM, CLI)
- Tutto lo stato è `Arc<DaemonState>` thread-safe

`bp-cli` è un **thin client**:
- Non ha accesso al swarm libp2p
- Comunica col daemon solo via Unix socket JSON
- `hatch` auto-avvia il daemon se non è in esecuzione

## Comunicazione CLI ↔ Daemon (IPC)

```
┌─────────────────────────────────────────────────────────────┐
│  bp hatch / bp flock / bp farewell / bp join / bp status    │
│                    (bp-cli binary)                          │
└────────────────────────┬────────────────────────────────────┘
                         │
              Unix Domain Socket
         ~/.local/share/billpouch/control.sock
              newline-delimited JSON
                         │
┌────────────────────────▼────────────────────────────────────┐
│                    bpd daemon                               │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  DaemonState (Arc)                                  │   │
│  │  ├── identity: Identity (Ed25519 keypair)           │   │
│  │  ├── services: RwLock<ServiceRegistry>              │   │
│  │  ├── networks: RwLock<Vec<String>>                  │   │
│  │  ├── network_state: Arc<RwLock<NetworkState>>       │   │
│  │  └── net_tx: mpsc::Sender<NetworkCommand>           │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  libp2p Swarm (BillPouchBehaviour)                  │   │
│  │  ├── gossipsub  — broadcast NodeInfo                │   │
│  │  ├── Kademlia   — DHT peer discovery                │   │
│  │  ├── Identify   — protocollo e indirizzi            │   │
│  │  └── mDNS       — LAN zero-config discovery         │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

Il socket è situato in `~/.local/share/billpouch/control.sock`.

## Stack libp2p (BillPouchBehaviour)

```rust
#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kad:       kad::Behaviour<kad::store::MemoryStore>,
    pub identify:  identify::Behaviour,
    pub mdns:      mdns::tokio::Behaviour,
}
```

| Protocollo    | Ruolo                                                        |
|---------------|--------------------------------------------------------------|
| **Gossipsub** | Broadcast flooding per gli annunci NodeInfo per rete         |
| **Kademlia**  | DHT per scoperta peer e content addressing (store in memoria)|
| **Identify**  | Scambio versione protocollo `/billpouch/id/1.0.0` e indirizzi |
| **mDNS**      | Scoperta peer sulla rete locale (zero-config, tokio runtime) |
| **Noise**     | Cifratura trasporto (wired nel SwarmBuilder)                 |
| **Yamux**     | Multiplexing stream (wired nel SwarmBuilder)                 |
| **TCP**       | Trasporto base (listen: `/ip4/0.0.0.0/tcp/0`)               |

## DaemonState

```rust
pub struct DaemonState {
    pub identity:      Identity,                      // keypair + fingerprint + alias
    pub services:      RwLock<ServiceRegistry>,       // servizi locali attivi
    pub network_state: Arc<RwLock<NetworkState>>,     // peer gossip noti
    pub networks:      RwLock<Vec<String>>,           // reti joined
    pub net_tx:        mpsc::Sender<NetworkCommand>,  // canale verso swarm
}
```

## NetworkCommand (daemon → swarm)

```rust
pub enum NetworkCommand {
    JoinNetwork  { network_id: String },
    LeaveNetwork { network_id: String },
    Announce     { network_id: String, payload: Vec<u8> },
    Dial         { addr: Multiaddr },
    Shutdown,
}
```

## Flusso di avvio daemon

1. `config::ensure_dirs()` — crea le directory XDG se non esistono
2. `Identity::load()` — legge il keypair da disk
3. Scrive PID file (`~/.local/share/billpouch/bp.pid`)
4. Crea `DaemonState` condiviso con `Arc`
5. `network::build_swarm(keypair)` — costruisce il swarm libp2p
6. `tokio::spawn(network::run_network_loop(...))` — loop swarm asincrono
7. `run_control_server(state)` — loop Unix socket (tokio::select! main loop)

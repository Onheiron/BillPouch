# 2. Architettura

## Struttura del workspace Cargo

```
BillPouch/
├── Cargo.toml                          # Cargo workspace (edition 2021)
└── crates/
    ├── bp-core/                            # Libreria core (no I/O diretto)
    │   └── src/
    │       ├── lib.rs                      # Re-esporta tutti i sottomoduli
    │       ├── identity.rs                 # Ed25519 keypair, export/import, fingerprint
    │       ├── service.rs                  # ServiceType, ServiceStatus, ServiceInfo, ServiceRegistry
    │       ├── config.rs                   # Percorsi XDG-aware (directories crate)
    │       ├── error.rs                    # BpError (thiserror) + BpResult alias
    │       ├── daemon.rs                   # Orchestratore daemon Tokio + PID file
    │       ├── invite.rs                   # Token invito: create/redeem, signed+encrypted
    │       ├── coding/
    │       │   ├── gf256.rs                # GF(2⁸) arithmetic (AES polynomial)
    │       │   ├── rlnc.rs                 # RLNC encode/recode/decode (Gaussian elimination)
    │       │   └── params.rs               # Adaptive k/n da peer QoS + target probability Ph
    │       ├── storage/
    │       │   ├── mod.rs                  # StorageManager — quota, disk layout, FragmentIndex
    │       │   ├── fragment.rs             # In-memory FragmentIndex per chunk
    │       │   ├── manifest.rs             # FileManifest + NetworkMetaKey (BLAKE3)
    │       │   ├── meta.rs                 # PouchMeta (quota, available_bytes)
    │       │   ├── encryption.rs           # ChunkCipher — CEK per-utente (ChaCha20-Poly1305)
    │       │   └── tier.rs                 # StorageTier T1–T5 — quote fisse
    │       ├── network/
    │       │   ├── behaviour.rs            # BillPouchBehaviour (#[derive(NetworkBehaviour)])
    │       │   ├── mod.rs                  # build_swarm(), run_network_loop(), NetworkCommand
    │       │   ├── state.rs                # NetworkState: upsert, evict_stale, in_network
    │       │   ├── qos.rs                  # PeerQos — RTT EWMA + fault score
    │       │   ├── reputation.rs           # ReputationTier R0–R4, ReputationRecord, ReputationStore
    │       │   ├── quality_monitor.rs      # Ping loop 60 s + Proof-of-Storage loop 300 s
    │       │   ├── fragment_gossip.rs      # RemoteFragmentIndex + AnnounceIndex
    │       │   ├── bootstrap.rs            # Persistent Kademlia peer cache (kad_peers.json)
    │       │   └── kad_store.rs            # Kademlia record persistence
    │       └── control/
    │           ├── protocol.rs             # ControlRequest/Response (JSON newline-delimited)
    │           └── server.rs               # DaemonState (Arc), dispatch(), run_control_server()
    ├── bp-cli/                             # Binario `bp`
    │   └── src/
    │       ├── main.rs                     # Entry point clap derive CLI
    │       ├── client.rs                   # ControlClient — Unix socket thin client
    │       └── commands/
    │           ├── auth.rs                 # login / logout / export-identity / import-identity
    │           ├── hatch.rs                # hatch (+ auto-avvio daemon)
    │           ├── flock.rs                # flock
    │           ├── farewell.rs             # farewell / --evict
    │           ├── pause.rs                # pause / resume
    │           ├── leave.rs                # leave
    │           ├── join.rs                 # join (hidden — interno)
    │           ├── put.rs                  # put (RLNC encode + CEK encrypt + distribute)
    │           ├── get.rs                  # get (fetch + RLNC decode + CEK decrypt)
    │           └── invite.rs               # invite create / invite join
    └── bp-api/                             # REST API daemon (axum)
        └── src/
            └── main.rs                     # HTTP server + embedded SPA dashboard
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
    pub identity:              Identity,                          // keypair + fingerprint + alias
    pub services:              RwLock<ServiceRegistry>,           // servizi locali attivi
    pub network_state:         Arc<RwLock<NetworkState>>,         // peer gossip noti
    pub networks:              RwLock<Vec<String>>,               // reti joined
    pub net_tx:                mpsc::Sender<NetworkCommand>,      // canale verso swarm
    pub storage_managers:      StorageManagerMap,                 // un manager per Pouch attivo
    pub qos:                   Arc<RwLock<QosRegistry>>,          // RTT + fault score per peer
    pub reputation:            RwLock<ReputationStore>,           // tier R0–R4 per peer
    pub outgoing_assignments:  OutgoingAssignments,               // chunk_id → Vec<peer_id> tracking PoS
    pub remote_fragment_index: Arc<RwLock<RemoteFragmentIndex>>,  // indice gossippato frammenti remoti
    pub chunk_cek_hints:       RwLock<HashMap<String, [u8;32]>>,  // chunk_id → plaintext_hash CEK
}
```

I campi `storage_managers`, `qos`, `reputation` e `chunk_cek_hints` sono persistiti su disco
(directory XDG). In particolare `chunk_cek_hints` viene salvato in `cek_hints.json` ad ogni
`PutFile` e ricaricato al riavvio del daemon, garantendo che i file rimangano decifrabili.

## NetworkCommand (daemon → swarm)

```rust
pub enum NetworkCommand {
    JoinNetwork         { network_id: String },
    LeaveNetwork        { network_id: String },
    Announce            { network_id: String, payload: Vec<u8> },
    Dial                { addr: Multiaddr },
    DialRelay           { relay_addr: Multiaddr },
    PushFragment        { peer_id: PeerId, fragment: EncodedFragment },
    FetchChunkFragments { chunk_id: String, peer_id: PeerId, reply: oneshot::Sender<…> },
    ProofOfStorage      { peer_id: PeerId, chunk_id: String, fragment_id: String, nonce: u64 },
    AnnounceIndex       { network_id: String, announcements: Vec<…> },
}
```

## Flusso di avvio daemon

1. `config::ensure_dirs()` — crea le directory XDG se non esistono
2. `Identity::load()` — legge il keypair da disk (plaintext o Argon2id+ChaCha20)
3. Scrive PID file (`~/.local/share/billpouch/daemon.pid`)
4. Carica `load_cek_hints()` da `cek_hints.json` (survivono i riavvii)
5. Crea `DaemonState` condiviso con `Arc`
6. `network::build_swarm(keypair)` — costruisce il swarm libp2p (TCP+Noise+Yamux)
7. `tokio::spawn(network::run_network_loop(...))` — loop swarm asincrono
8. `tokio::spawn(run_quality_monitor(...))` — Ping 60 s + PoS 300 s
9. `run_control_server(state)` — main loop Unix socket

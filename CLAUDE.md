# BillPouch — Claude Instructions

## Project

BillPouch is a **P2P social distributed filesystem** written in Rust (edition 2021).
It is a Cargo workspace with three crates:

- **`bp-core`** — pure library (no direct I/O, no stdout, no `process::exit`)
- **`bp-cli`** — `bp` binary, thin CLI client that talks to the daemon via Unix socket
- **`bp-api`** — REST API server (axum) with embedded web dashboard

Full documentation in the `wiki/` folder.

---

## Struttura crate

```
crates/
  bp-core/src/
    identity.rs           # Ed25519 keypair, export/import, fingerprint = hex(SHA256(pubkey))[0..8]
    service.rs            # ServiceType (Pouch|Bill|Post), ServiceInfo, ServiceRegistry
    config.rs             # XDG paths via `directories` crate
    error.rs              # BpError (thiserror) + BpResult
    daemon.rs             # run_daemon(), is_running(), PID file
    invite.rs             # Invite token create/redeem (signed + password-encrypted)
    coding/
      gf256.rs            # GF(2⁸) arithmetic (AES polynomial)
      rlnc.rs             # RLNC encode / recode / decode (Gaussian elimination)
      params.rs           # Adaptive k/n from peer QoS + target recovery probability Ph
    storage/
      mod.rs              # StorageManager — quota, disk layout, FragmentIndex
      fragment.rs         # In-memory per-chunk fragment index
      manifest.rs         # FileManifest + NetworkMetaKey (BLAKE3)
      meta.rs             # PouchMeta — capacity, available_bytes, has_capacity
      encryption.rs       # ChunkCipher — per-user CEK (ChaCha20-Poly1305)
      tier.rs             # StorageTier T1–T5 — fixed quota tiers
    network/
      behaviour.rs        # BillPouchBehaviour: gossipsub+kad+identify+mdns
      mod.rs              # NetworkCommand, build_swarm(), run_network_loop()
      state.rs            # NetworkState: upsert/evict_stale/in_network
      qos.rs              # PeerQos — RTT EWMA + fault score
      reputation.rs       # ReputationTier R0–R4, ReputationStore
      quality_monitor.rs  # Ping loop (60 s) + Proof-of-Storage loop (300 s)
      fragment_gossip.rs  # RemoteFragmentIndex + AnnounceIndex gossip
      bootstrap.rs        # Persistent Kademlia peer cache (kad_peers.json)
      kad_store.rs        # Kademlia record persistence
    control/
      protocol.rs         # ControlRequest / ControlResponse (JSON newline-delimited)
      server.rs           # DaemonState (Arc), dispatch(), run_control_server()
  bp-cli/src/
    main.rs               # clap derive CLI
    client.rs             # ControlClient — Unix socket JSON client
    commands/
      auth.rs             # login / logout / export-identity / import-identity
      hatch.rs            # hatch
      flock.rs            # flock
      status.rs           # status
      farewell.rs         # farewell / --evict
      pause.rs            # pause / resume
      leave.rs            # leave [--force]
      join.rs             # join (hidden, internal)
      put.rs              # put (RLNC encode + CEK encrypt + distribute)
      get.rs              # get (fetch fragments + RLNC decode + CEK decrypt)
      invite.rs           # invite create / invite join
      bootstrap.rs        # bootstrap list / add / remove
      relay.rs            # relay connect
  bp-api/src/
    main.rs               # axum HTTP server, embedded SPA dashboard at GET /
```

---

## Project rules

### bp-core — pure library
- **Forbidden:** `println!`, `eprintln!`, `std::process::exit`, any direct I/O
- Use **`thiserror`** for `BpError`, expose `BpResult<T> = Result<T, BpError>`
- Shared state: `Arc<DaemonState>` with `RwLock<T>` per mutable field
- Async with **Tokio** (`features = ["full"]`)
- Logging with **`tracing`** (never `println!` for debug)

### bp-cli — thin client
- Use **`anyhow`** for errors
- Never imports the libp2p swarm directly
- All daemon communication via `ControlClient` on Unix socket

### Type conventions
- `service_id` → always `String` (UUID v4 via `uuid::Uuid::new_v4()`)
- `user_fingerprint` → immutable `String`: `hex(SHA-256(pubkey_bytes))[0..8]`
- `network_id` → free `String` (e.g. `"friends"`, `"work"`, `"public"`)
- Gossipsub topic → `format!("billpouch/v1/{}/nodes", network_id)`
- Socket → `~/.local/share/billpouch/control.sock`

---

## IPC: CLI ↔ Daemon

Transport: Unix domain socket, **JSON newline-delimited**, UTF-8.

```rust
pub enum ControlRequest {
    Ping,
    Status,
    Hatch           { service_type: ServiceType, network_id: String, metadata: HashMap<String, Value> },
    Flock,
    Farewell        { service_id: String },
    FarewellEvict   { service_id: String },
    Pause           { service_id: String, eta_minutes: u64 },
    Resume          { service_id: String },
    Join            { network_id: String },
    Leave           { network_id: String, force: bool },
    ConnectRelay    { relay_addr: String },
    PutFile         { chunk_data: Vec<u8>, ph: Option<f64>, q_target: Option<f64>, network_id: String },
    GetFile         { chunk_id: String, network_id: String },
    CreateInvite    { network_id: String, invitee_fingerprint: Option<String>, invite_password: String, ttl_hours: Option<u64> },
}

// Responses
ControlResponse::Ok    { data: Option<Value> }
ControlResponse::Error { message: String }
```

---

## Service types

| Type    | Symbol     | Role                                           |
|---------|------------|------------------------------------------------|
| `Pouch` | 🦤 pouch | Bids local storage into the network            |
| `Bill`  | 🦤 bill  | Personal file I/O interface                    |
| `Post`  | 🦤 wings | Pure relay — CPU + bandwidth only, no storage  |

---

## libp2p stack

```rust
#[derive(NetworkBehaviour)]
pub struct BillPouchBehaviour {
    pub gossipsub: gossipsub::Behaviour,           // broadcast NodeInfo
    pub kad:       kad::Behaviour<MemoryStore>,    // DHT peer discovery
    pub identify:  identify::Behaviour,            // /billpouch/id/1.0.0
    pub mdns:      mdns::tokio::Behaviour,         // LAN zero-config
}
// Transport: TCP + Noise (encryption) + Yamux (mux)
```

---

## Development commands

```bash
cargo test --workspace                          # all tests
cargo test -p bp-core --test architecture_test  # architecture tests only
cargo clippy --workspace -- -D warnings         # linting (warnings = errors)
cargo fmt --all                                 # formatting

# P2P smoke test (requires Docker)
docker compose -f docker-compose.smoke.yml up --build -d
./smoke/smoke-test.sh

# Interactive playground (5 simulated nodes)
./playground.sh up && ./playground.sh enter
```

---

## Stato del progetto

**v0.3.0** — All v0.3 features complete: StorageTier T1–T5, ReputationTier R0–R4,
`bp status`, `bp pause/resume`, `bp farewell --evict`, `bp leave --force`,
CEK persistence, invite system, multi-device identity, REST API + web dashboard.
File transfer: `bp put` / `bp get` (RLNC encode/decode, CEK encryption, fragment
distribution, adaptive k/n QoS, Proof-of-Storage, FragmentIndex gossip).

See `wiki/12-status-roadmap.md` for the full feature list.

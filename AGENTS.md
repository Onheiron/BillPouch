# BillPouch — Agent Instructions

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
      farewell.rs         # farewell / --evict
      pause.rs            # pause / resume
      leave.rs            # leave
      join.rs             # join (hidden, internal)
      put.rs              # put (RLNC encode + CEK encrypt + distribute)
      get.rs              # get (fetch fragments + RLNC decode + CEK decrypt)
      invite.rs           # invite create / invite join
  bp-api/src/
    main.rs               # axum HTTP server, embedded SPA dashboard at GET /
```

---

## Core rules

### bp-core — pure library
- **Forbidden:** `println!`, `eprintln!`, `std::process::exit`, any direct I/O
- Error handling with `thiserror` → `BpError` + `BpResult`
- Shared state: `Arc<DaemonState>` with internal `RwLock` per field
- Async with Tokio (`features = ["full"]`)
- Logging with `tracing` (never `println!` for debug)

### bp-cli
- Error handling with `anyhow`
- Never accesses the libp2p swarm directly
- All daemon communication via `ControlClient` on Unix socket

## Conventions
- `service_id` → UUID v4 (`uuid::Uuid::new_v4().to_string()`)
- `user_fingerprint` → `hex(SHA-256(pubkey_bytes))[0..8]` — immutable
- Gossipsub topic → `format!("billpouch/v1/{}/nodes", network_id)`
- Socket → `~/.local/share/billpouch/control.sock`
- Default network → `"public"`

---

## Control protocol

Transport: Unix domain socket, JSON newline-delimited, UTF-8.

```rust
pub enum ControlRequest {
    Ping,
    Status,
    Hatch           { service_type, network_id, metadata },
    Flock,
    Farewell        { service_id },
    Join            { network_id },
    Leave           { network_id },
    ConnectRelay    { relay_addr },
    PutFile         { chunk_data, ph, q_target, network_id },
    GetFile         { chunk_id, network_id },
    ProposeStorage  { network_id, bytes_offered, duration_secs, price_tokens },
    AcceptStorage   { offer_id },
    ListOffers      { network_id },
    ListAgreements  { network_id },
    CreateInvite    { network_id, password },
    RedeemInvite    { token, password },
}

// Responses
ControlResponse::Ok    { data: Option<Value> }
ControlResponse::Error { message: String }
```

---

## Service types

| Type    | CLI              | Role                                       |
|---------|------------------|--------------------------------------------|
| `pouch` | `bp hatch pouch` | Bids local storage into the network        |
| `bill`  | `bp hatch bill`  | Personal file I/O interface                |
| `post`  | `bp hatch post`  | Pure relay — CPU + bandwidth, no storage   |

---

## NodeInfo gossip message

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

## Development commands

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
./playground.sh up && ./playground.sh enter   # interactive test
```

---

## Stato

**v0.3.0-dev** — In sviluppo: StorageTier T1–T5, ReputationTier R0–R4, `bp pause/resume`, `bp farewell --evict`, `bp leave` precondition.

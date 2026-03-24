# BillPouch — GitHub Copilot Instructions

## Project

BillPouch is a **P2P social distributed filesystem** written in Rust (edition 2021).
It is a Cargo workspace with three crates:

- **`bp-core`** — pure library (no direct I/O, no stdout, no `process::exit`)
- **`bp-cli`** — `bp` binary, thin CLI client that talks to the daemon via Unix socket
- **`bp-api`** — REST API server (axum) with embedded web dashboard

Full documentation in the `wiki/` folder.

---

## Workspace structure

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

## Core rules

### bp-core — pure library
- **Forbidden:** `println!`, `eprintln!`, `std::process::exit`, any direct I/O
- Error handling with `thiserror` → `BpError` + `BpResult<T>`
- Shared state: `Arc<DaemonState>` with `RwLock<T>` per mutable field
- Async with Tokio (`features = ["full"]`)

### bp-cli
- Error handling with `anyhow`
- Never imports libp2p directly
- All daemon communication via `ControlClient` on Unix socket
- `bp hatch` auto-starts the daemon if not running

### General
- Logging with `tracing` macros only — never `println!` for debug
- All code comments and doc-comments must be **in English**
- Services: **Pouch** (storage), **Bill** (file I/O), **Post** (pure relay)
- `service_id` → UUID v4 string
- `user_fingerprint` → immutable `hex(SHA-256(pubkey_bytes))[0..8]` (16 hex chars)
- Gossipsub topic → `billpouch/v1/{network_id}/nodes`
- Control socket → `~/.local/share/billpouch/control.sock`
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
    FarewellEvict   { service_id },
    Pause           { service_id, eta_minutes },
    Resume          { service_id },
    Join            { network_id },
    Leave           { network_id, force: bool },
    ConnectRelay    { relay_addr },
    PutFile         { chunk_data, ph, q_target, network_id },
    GetFile         { chunk_id, network_id },
    CreateInvite    { network_id, invitee_fingerprint, invite_password, ttl_hours },
}

// Responses
ControlResponse::Ok    { data: Option<Value> }
ControlResponse::Error { message: String }
```

---

## Testing

- Architecture tests: `crates/bp-core/tests/architecture_test.rs`
- Integration tests: `crates/bp-core/tests/integration_test.rs`
- Docker smoke test: `smoke/smoke-test.sh` with `docker-compose.smoke.yml`
- Interactive playground: `./playground.sh up && ./playground.sh enter`

For async tests requiring Tokio (e.g. swarm mDNS): use `#[tokio::test]`.
To serialize tests that mutate `HOME`/`XDG_DATA_HOME`: use `static ENV_LOCK: Mutex<()>`.

---

## Key dependencies

| Crate            | Purpose                                       |
|------------------|-----------------------------------------------|
| libp2p 0.54      | P2P stack (gossipsub, Kademlia, mDNS, Noise)  |
| tokio 1.x        | Async runtime                                 |
| serde_json       | Control protocol + NodeInfo wire format       |
| uuid             | service_id (v4)                               |
| chrono           | Timestamps on NodeInfo and UserProfile        |
| sha2 + hex       | Fingerprint derivation                        |
| blake3           | NetworkMetaKey + PoS challenge                |
| chacha20poly1305 | CEK encryption + identity key encryption      |
| argon2           | KDF for passphrase-protected identity keys    |
| clap 4.x         | CLI (derive)                                  |
| thiserror        | Errors in bp-core                             |
| anyhow           | Errors in bp-cli                              |
| axum 0.7         | REST API in bp-api                            |
| directories      | XDG-compliant config paths                    |

---

## Current status: v0.3.0

All core features implemented: `bp put` / `bp get` (RLNC encode/decode, CEK encryption,
adaptive k/n, remote fragment distribution), invite system, multi-device identity export/import,
StorageTier T1–T5, ReputationTier R0–R4, `bp status`, `bp pause/resume`, `bp farewell --evict`,
`bp leave --force`, Proof-of-Storage, FragmentIndex gossip, REST API + web dashboard.

See `wiki/12-status-roadmap.md` for the full feature list.

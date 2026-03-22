# bp-core

Core library for BillPouch — a P2P social distributed filesystem.

## Rationale

`bp-core` is a **pure library crate**: it contains all business logic, networking, protocol and
persistence code for BillPouch, but it never performs direct I/O to stdout/stderr and never calls
`process::exit`. This strict separation exists so that:

- The daemon binary (`bp-cli --daemon`) can embed the library and control its lifecycle cleanly.
- Tests can exercise every subsystem in isolation without spawning OS processes.
- Future frontends (GUI, WASM, mobile) can reuse the same crate without changes.

## Modules

| Module | Purpose |
|---|---|
| [`identity`] | Ed25519 keypair generation, persistence, export/import, and fingerprint derivation. |
| [`service`] | `ServiceType` (Pouch / Bill / Post), runtime `ServiceInfo`, and the in-process `ServiceRegistry`. |
| [`config`] | XDG-compliant file paths for keys, profiles, PID, socket, and network state. |
| [`error`] | Unified `BpError` (via `thiserror`) and the `BpResult<T>` alias. |
| [`daemon`] | Entry point for the daemon task: builds the swarm, spawns async workers, runs the control server. |
| [`invite`] | Invite token creation and redemption (Ed25519-signed, Argon2+ChaCha20 encrypted). |
| [`coding::gf256`] | GF(2⁸) field arithmetic over the AES polynomial. |
| [`coding::rlnc`] | RLNC encode / recode / decode (Gaussian elimination over GF(2⁸)). |
| [`coding::params`] | Adaptive k/n selection from live peer QoS scores and target recovery probability Ph. |
| [`storage`] | `StorageManager` — fragment storage, quota management, disk layout. |
| [`storage::fragment`] | In-memory `FragmentIndex` — maps chunk_id to stored fragment IDs. |
| [`storage::manifest`] | `FileManifest` and `NetworkMetaKey` (BLAKE3-based per-network secret). |
| [`storage::meta`] | `PouchMeta` — persisted capacity and available bytes. |
| [`storage::encryption`] | `ChunkCipher` — per-user Content Encryption Key (ChaCha20-Poly1305). |
| [`storage::agreement`] | `StorageOffer` and `AgreementStore` for the storage marketplace. |
| [`network`] | libp2p Swarm (gossipsub + Kademlia + Identify + mDNS), commands channel, event loop. |
| [`network::state`] | `NetworkState` — local view of all known peers, updated via gossip. |
| [`network::qos`] | `PeerQos` and `QosRegistry` — RTT EWMA + fault score per peer. |
| [`network::quality_monitor`] | Ping loop (60 s) and Proof-of-Storage challenge loop (300 s). |
| [`network::fragment_gossip`] | `RemoteFragmentIndex` and `AnnounceIndex` gossip for targeted fragment fetch. |
| [`network::bootstrap`] | Persistent Kademlia peer cache (`kad_peers.json`). |
| [`network::kad_store`] | Kademlia record persistence. |
| [`control::protocol`] | JSON newline-delimited request/response types exchanged over the Unix socket. |
| [`control::server`] | Async Unix socket server that dispatches `ControlRequest`s against `DaemonState`. |

## The three service types

```
Pouch  →  bids local storage into the network  (the pelican's pouch)
Bill   →  personal file I/O interface          (the pelican's bill)
Post   →  pure relay node: CPU + bandwidth     (the pelican's wings)
```

## Key conventions

- `service_id` — UUID v4 string, unique per running service instance.
- `user_fingerprint` — `hex(SHA-256(pubkey_bytes))[0..8]` (16 hex chars), immutable per identity.
- Gossipsub topic — `billpouch/v1/{network_id}/nodes`.
- Control socket — `~/.local/share/billpouch/control.sock` (XDG data dir).
- Shared mutable state — always `Arc<T>` with inner `RwLock<U>`; no `Mutex` except in tests.
- Logging — `tracing` macros only; `println!` / `eprintln!` are banned.

## Error handling

All fallible public functions return `BpResult<T>` (`Result<T, BpError>`).
`BpError` uses `#[from]` blanket impls for `std::io::Error` and `serde_json::Error`; all
other variants carry a descriptive `String` payload for context.

## Dependencies worth knowing

| Crate | Role |
|---|---|
| `libp2p 0.54` | Transport, gossipsub, Kademlia, mDNS, Noise encryption, Yamux mux |
| `tokio` (`full`) | Async runtime |
| `serde` / `serde_json` | Wire protocol serialisation |
| `uuid` | Service IDs |
| `sha2` + `hex` | Fingerprint computation |
| `blake3` | NetworkMetaKey derivation + Proof-of-Storage challenges |
| `chacha20poly1305` | CEK encryption (chunks) + identity key encryption |
| `argon2` | KDF for passphrase-protected identity keys |
| `directories` | XDG-compliant configuration paths |
| `chrono` | Timestamps on `NodeInfo` and `UserProfile` |
| `thiserror` | Ergonomic error derivation |

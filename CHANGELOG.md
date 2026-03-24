# Changelog

All notable changes to BillPouch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] — v0.3.0-dev

### Breaking Changes

- `bp hatch pouch --storage-bytes <N>` rimosso — sostituito da `--tier <T1..T5>`
- `bp join` rimosso dal CLI pubblico — resta hidden per uso interno script
- Storage marketplace rimosso: `bp offer`, `bp agree`, `bp offers`, `bp agreements`, REST `/marketplace/*`

### Added

- **`StorageTier` T1–T5** (`storage/tier.rs`) — tier fissi 10 GiB / 100 GiB / 500 GiB / 1 TiB / 5 TiB con `quota_bytes()`, `participating_tiers()`, `parse()`, serde
- **`ReputationTier` R0–R4** (`network/reputation.rs`) — tier discreti di reputazione basati su storico uptime e PoS, `ReputationRecord`, `ReputationStore` in `DaemonState`
- **`ServiceStatus::Paused`** (`service.rs`) — con `eta_minutes` e `paused_at`; aggiornati `Stopping` e `Error(String)`
- **`ControlRequest::Pause` / `Resume`** — manutenzione temporanea con announcement gossip
- **`ControlRequest::FarewellEvict`** — eviction permanente Pouch: purge storage + gossip `evicting=true` + penalità reputazione
- **`ControlRequest::Leave`** aggiornato — precondition check servizi attivi; risposta con `blocked: true` e lista hint di stop
- **`StorageManager::purge()` / `storage_summary()`** — rimozione definitiva storage su disco
- **One-Pouch-per-network enforcement** — il daemon rifiuta un secondo `hatch pouch --network X` sulla stessa identità
- **`bp pause <service_id> --eta <minutes>`** / **`bp resume <service_id>`** — nuovi comandi CLI
- **`bp farewell --evict`** — flag eviction permanente
- **`BpError::InvalidInput`** — nuova variante per input non validi (es. tier sconosciuto)
- **CEK hints persistence** (`cek_hints.json`) — `load_cek_hints()` + `persist_cek_hints()` in `server.rs`; le chiavi di cifratura sopravvivono al riavvio del daemon
- **`bp flock` tier display** — i servizi Pouch mostrano il tier (`tier: T2`) nella lista locale
- **`bp status`** — nuovo comando compatto: identità daemon (fingerprint, alias, peer_id, versione), servizi attivi, reti, conteggio peer
- **`bp leave --force`** — auto-evict di tutti i servizi attivi sul network (Pouch: eviction permanente; Bill/Post: stop graceful), poi leave; `force: bool` aggiunto a `ControlRequest::Leave` con `#[serde(default)]` per retrocompatibilità
- **`ControlRequest::Status`** + **`StatusData`** — payload strutturato con `peer_id`, `fingerprint`, `alias`, `local_services`, `networks`, `known_peers`, `version`
- **`LeaveData`** struct — payload tipizzato per risposte Leave
- **bp-api route v0.3** — `POST /services/:id/pause`, `POST /services/:id/resume`, `DELETE /services/:id?evict=true`, `POST /invites`, `POST /invites/redeem`

### Tests

- Nuovi test architettura (sezioni 11–13): `StorageTier`, `ReputationTier`, `ServiceStatus::Paused` lifecycle
- Nuovi test architettura: `ControlRequest::Status` serialization roundtrip; `StatusData` field completeness
- Nuovi integration test: `pause_resume_roundtrip`, `farewell_evict_removes_service`, `leave_blocked_by_active_service`, `hatch_second_pouch_same_network_rejected`

## [0.1.3](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.2...billpouch-v0.1.3) (2026-03-18)


### Bug Fixes

* **release:** build binaries in release-please workflow ([9ffe8e2](https://github.com/Onheiron/BillPouch/commit/9ffe8e22111c30349356b4d26e0b9bf3189da75d))

## [0.1.2](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.1...billpouch-v0.1.2) (2026-03-18)


### Bug Fixes

* **release:** trigger on billpouch-v* tags from release-please ([7ad0516](https://github.com/Onheiron/BillPouch/commit/7ad051678f3b8f12d2ccf39203f6d53fe71411a6))

## [0.1.1](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.0...billpouch-v0.1.1) (2026-03-18)


### Features

* **smoke:** add Docker Compose smoke test with 3 P2P nodes ([2ea2ab8](https://github.com/Onheiron/BillPouch/commit/2ea2ab85c57d72afb96a7b26948257c247cbc79d))


### Bug Fixes

* **ci:** accept any swarm build failure on CI environments ([71f2569](https://github.com/Onheiron/BillPouch/commit/71f2569fa81ff4f2698fa641909463d030758ae1))
* **ci:** fix failing GitHub Actions workflows ([1d84dba](https://github.com/Onheiron/BillPouch/commit/1d84dbafbd7a2bb1cf3742415e912cb018a5681a))
* **ci:** fix failing GitHub Actions workflows ([332b854](https://github.com/Onheiron/BillPouch/commit/332b854683c1510b1e061f9996fd621566538313))
* **ci:** fix formatting, release-please, and coverage workflows ([430bb64](https://github.com/Onheiron/BillPouch/commit/430bb640e404e08c4bacf5b96663e72b75b4aa3b))
* **ci:** propagate mDNS errors instead of panicking in build_swarm ([d5d89bb](https://github.com/Onheiron/BillPouch/commit/d5d89bb3c5c086da04eb63e1abb0221dc37da570))
* **ci:** use #[tokio::test] for swarm test to provide Tokio runtime on Linux ([c2cc481](https://github.com/Onheiron/BillPouch/commit/c2cc481499a19ef2cb468fc292ece2d3615522be))
* **docker:** bump Rust image to 1.85 for edition2024 support ([d752b52](https://github.com/Onheiron/BillPouch/commit/d752b52ae0bd4b133d3444e8d644057e141f9c50))
* **smoke:** hatch services after mesh formation for gossipsub propagation ([576740c](https://github.com/Onheiron/BillPouch/commit/576740c126957fe7d793e79ec4733bb2b30bd05c))
* **smoke:** join network before mesh wait to keep connections alive ([d068e17](https://github.com/Onheiron/BillPouch/commit/d068e171b4f4ba9513fb30c24144b995ccec0760))
* **smoke:** match service type without brackets in Known Peers section ([38fc272](https://github.com/Onheiron/BillPouch/commit/38fc272ad5b66fd5d1a42a7faafdc666176d10cc))
* **smoke:** move network join into entrypoint for immediate gossipsub subscription ([1d51b83](https://github.com/Onheiron/BillPouch/commit/1d51b83de33239044d3230653aa69e8220e1bd37))
* **smoke:** use safe arithmetic to avoid set -e exit on zero increment ([3045fbd](https://github.com/Onheiron/BillPouch/commit/3045fbde340f55ae2f702e0284a9f3ccbf312db2))

## [0.1.0] — 2026-03-18

### Features

- Ed25519 identity management (login/logout/fingerprint)
- Three service types: pouch (storage), bill (file I/O), post (relay)
- Service registry with UUID-based instance tracking
- libp2p swarm with gossipsub, Kademlia, mDNS, Noise, Yamux
- Gossip-based NodeInfo DHT with stale peer eviction
- Unix socket control protocol (CLI <-> daemon, newline-delimited JSON)
- Multi-network support (join/leave independent networks)
- CLI commands: login, logout, hatch, flock, farewell, join
- Extensible metadata on NodeInfo for future protocol extensions

### Testing

- 43 architecture verification unit tests
- Integration tests for control protocol over Unix socket

### CI/CD

- GitHub Actions: CI (fmt, clippy, test), security audit, coverage, docs
- Cross-platform release builds (Linux x86_64, macOS x86_64/aarch64)
- GitHub Pages documentation site

[0.1.0]: https://github.com/Onheiron/BillPouch/releases/tag/v0.1.0

# Changelog

All notable changes to BillPouch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

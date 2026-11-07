# 🦤 BillPouch

**A peer-to-peer, social distributed filesystem written in Rust.**

[![CI](https://github.com/Onheiron/BillPouch/actions/workflows/ci.yml/badge.svg)](https://github.com/Onheiron/BillPouch/actions/workflows/ci.yml)
[![Smoke Test](https://github.com/Onheiron/BillPouch/actions/workflows/smoke.yml/badge.svg)](https://github.com/Onheiron/BillPouch/actions/workflows/smoke.yml)
[![Release](https://img.shields.io/github/v/release/Onheiron/BillPouch?color=e94560)](https://github.com/Onheiron/BillPouch/releases)
![Status: alpha](https://img.shields.io/badge/status-alpha-red.svg)

Friends pool part of their disks into a private network. Files are encrypted on
the owner's machine, split with erasure coding (RLNC) and spread across
everyone's storage, so a file stays recoverable while enough peers are online.
There is no server: every node is equal.

## Services

Each node runs one or more services, named after the pelican:

| Service | Role |
|---------|------|
| `pouch` | Contributes a fixed slice of local disk (tier T1 = 10 GiB … T5 = 5 TiB) |
| `bill`  | Your file interface: `bp put` / `bp get` |
| `post`  | Pure relay: bandwidth and routing, no storage |

## Quick start

```bash
cargo build --release          # binaries: target/release/bp, target/release/bp-api

bp login --alias carlo         # create your Ed25519 identity
bp hatch pouch --network friends --tier T1   # starts the daemon, offers 10 GiB
bp hatch bill  --network friends

bp put photo.jpg --network friends          # → chunk_id
bp get <chunk_id> photo-copy.jpg --network friends
bp ls                                        # files you uploaded
bp status                                    # identity, services, storage, peers
```

Friends join a network with an invite (membership is not enforced yet, see [roadmap](docs/roadmap.md)).

```bash
bp invite create --network friends --invite-password <pwd>   # prints a token
bp invite join <token> --invite-password <pwd>               # on the friend's machine
```

To try a whole network on one machine: `./playground.sh up` (Docker, 6 nodes).

## Documentation

| Page | Contents |
|------|----------|
| [docs/architecture.md](docs/architecture.md) | How it works: crates, daemon, networking, storage and coding, QoS and reputation, security |
| [docs/cli.md](docs/cli.md) | Every `bp` command |
| [docs/api.md](docs/api.md) | Control protocol (CLI ↔ daemon) and REST API (`bp-api`) |
| [docs/development.md](docs/development.md) | Build, tests, playground, CI and releases |
| [docs/roadmap.md](docs/roadmap.md) | What is done, known limitations, what is next |
| [CHANGELOG.md](CHANGELOG.md) | Release history (generated) |

## License

MIT OR Apache-2.0

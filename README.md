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

bp login --ssh-key ~/.ssh/id_ed25519   # your SSH key is your identity (or omit to generate one)
bp network create friends              # you are the first member
bp hatch pouch --network friends --tier T1   # offers 10 GiB
bp hatch bill  --network friends

bp put photo.jpg --network friends              # → file_id
bp get <file_id> photo-copy.jpg --network friends
bp ls                                           # your files and the ones shared with you
bp members friends                              # who is in, what they host and store
bp dashboard                                    # the web app: nodes, space, your files
```

Networks are invite-only. Your friend sends you the public key `bp login` printed for them:

```bash
bp invite friends "ssh-ed25519 AAAA… marco@pc"   # prints a token for Marco
bp join <token>                                  # on Marco's machine
```

You can only store what the others have verified you host, so a network needs at
least two users with a Pouch each before anyone can upload.

To try a whole network on one machine: `./playground.sh up` (Docker, 6 nodes, built from this checkout), then `./playground.sh app` for the web app.

## Android

`android/` is the Android app: your phone as another device of yours, running a
**Bill** only. It dials out and never listens, so it needs no open port and holds
nothing for the others. The [Android workflow](.github/workflows/android.yml)
builds the APK (artifact `billpouch-android`). On first launch give it the same
SSH private key as your computer and the token from
`bp invite <network> "<your SSH public key>"` there; it shows the same web app,
with files picked and saved through Android's own dialogs.

## Documentation

| Page | Contents |
|------|----------|
| [docs/architecture.md](docs/architecture.md) | How it works: crates, daemon, networking, storage and coding, QoS and reputation, security |
| [docs/cli.md](docs/cli.md) | Every `bp` command |
| [docs/api.md](docs/api.md) | Control protocol (CLI ↔ daemon) and REST API (`bp-api`) |
| [docs/development.md](docs/development.md) | Build, tests, playground, CI and releases |
| [docs/roadmap.md](docs/roadmap.md) | What is done, known limitations, what is next |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

# BillPouch — Wiki

Benvenuto nella wiki di **BillPouch**, un filesystem distribuito P2P scritto in Rust.

## Indice

| # | Documento | Descrizione |
|---|-----------|-------------|
| 1 | [Panoramica](./01-overview.md) | Cos'è BillPouch, metafora del pellicano, caratteristiche principali |
| 2 | [Architettura](./02-architecture.md) | Struttura crate, IPC CLI↔Daemon, stack libp2p |
| 3 | [Tecnologie](./03-technologies.md) | Tutte le dipendenze Rust con ruolo e versione |
| 4 | [Servizi](./04-services.md) | Pouch, Bill, Post — ciclo di vita, registry |
| 5 | [Protocollo di controllo](./05-control-protocol.md) | Unix socket, ControlRequest/Response JSON |
| 6 | [NodeInfo & Gossip](./06-nodeinfo-gossip.md) | Formato NodeInfo, topic gossipsub, NetworkState |
| 7 | [Identità e sicurezza](./07-identity-security.md) | Ed25519, fingerprint, percorsi XDG |
| 8 | [CLI Reference](./08-cli-reference.md) | Tutti i comandi `bp` con flag ed esempi |
| 9 | [Testing](./09-testing.md) | Architettura test, integration test, smoke test Docker |
| 10 | [CI/CD & DevOps](./10-cicd-devops.md) | GitHub Actions, release, conventional commits |
| 11 | [Docker & Playground](./11-docker-playground.md) | Dockerfile, smoke cluster, playground interattivo |
| 12 | [Stato & Roadmap](./12-status-roadmap.md) | Funzionalità implementate, limitazioni, roadmap |
| 13 | [Use Case](./13-use-cases.md) | Scenari d'uso concreti |

## Quick Reference

```bash
bp login --alias "carlo"          # Crea identità Ed25519
bp hatch pouch --network amici \
  --storage-bytes 10737418240     # Offri 10 GB di storage
bp hatch bill --network amici     # Client file I/O personale
bp hatch post --network amici     # Nodo relay puro
bp flock                          # Mostra peer e rete
bp farewell <service-id>          # Ferma un servizio
bp join <network-id>              # Unisciti a un'altra rete
```

## Versione documentata

**v0.1.3** (Alpha) — Marzo 2026

# 1. Panoramica

## Cos'è BillPouch?

BillPouch è un **filesystem distribuito P2P sociale** scritto interamente in Rust.
Non esiste un server centrale né un coordinatore: ogni nodo è uguale agli altri
(*leaderless*). I partecipanti contribuiscono spazio su disco e la rete traccia
chi contribuisce cosa tramite un DHT basato su gossip.

## La metafora del Pellicano

Il nome e l'architettura traggono ispirazione dal pellicano — un uccello che caccia,
immagazzina e condivide.

| Servizio | Simbolo             | Ruolo                                                        |
|----------|---------------------|--------------------------------------------------------------|
| `pouch`  | 🦤 La sacca (gozzo) | Offre una porzione di **storage locale** alla rete           |
| `bill`   | 🦤 Il becco         | **Interfaccia file I/O** personale — finestra sui file della rete |
| `post`   | 🦤 Le ali           | **Routing / relay** puro — contribuisce solo CPU e RAM, nessun storage |

## Caratteristiche principali

- **Completamente decentralizzato** — nessun server centrale, nessun coordinatore
- **Identità Ed25519** — la chiave pubblica è il tuo ID permanente su ogni rete
- **Scoperta peer zero-config** — mDNS trova i peer sulla LAN senza configurazione
- **Multi-rete** — puoi partecipare a più reti indipendenti contemporaneamente
- **Architettura modulare** — `bp-core` è una libreria Rust pura senza I/O diretto
- **Storage sociale** — lo storage è distribuito tra i partecipanti della rete
- **Gossipsub DHT-like** — i NodeInfo sono propagati e stale-evicted automaticamente
- **Metadata estensibili** — ogni servizio può portare metadati arbitrari

## Componenti principali

```
bp (CLI)          →   bpd (daemon)   →   libp2p swarm
   login/logout       service registry    gossipsub
   hatch/farewell     unix socket IPC     Kademlia DHT
   flock/join         NodeInfo store      mDNS discovery
                                          Noise + Yamux
```

## Stato attuale

| Campo       | Valore                              |
|-------------|-------------------------------------|
| Versione    | `0.1.3` (Alpha)                     |
| Edizione    | Rust 2021                           |
| Licenza     | MIT OR Apache-2.0                   |
| Target      | Linux, macOS (aarch64 e x86_64)     |
| Repository  | https://github.com/Onheiron/BillPouch |
| Docs        | https://onheiron.github.io/BillPouch |

> **Nota Alpha:** il layer di trasferimento file reale (chunkng, cifratura, erasure coding)
> non è ancora implementato. I servizi si annunciano via gossip ma non trasferiscono dati.

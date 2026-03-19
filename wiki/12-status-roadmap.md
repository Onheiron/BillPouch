# 12. Stato del progetto & Roadmap

## Versione corrente

**v0.1.3** (Alpha) — rilasciata Marzo 2026

---

## Funzionalità implementate ✅

### Core (`bp-core`)

| Funzionalità                      | Stato   | Note                                         |
|-----------------------------------|---------|----------------------------------------------|
| Gestione identità Ed25519         | ✅ Done | login, logout, fingerprint, alias            |
| ServiceType (pouch/bill/post)     | ✅ Done | Parsing case-insensitive                     |
| ServiceRegistry (in-memory)       | ✅ Done | register, get, remove, all con UUID v4       |
| libp2p Swarm                      | ✅ Done | gossipsub + Kademlia + mDNS + Noise + Yamux  |
| Gossip NodeInfo                   | ✅ Done | publish, subscribe, deserialize              |
| NetworkState store                | ✅ Done | upsert, evict_stale(120s), in_network        |
| Topic naming gossipsub            | ✅ Done | `billpouch/v1/{network_id}/nodes`            |
| Unix socket IPC                   | ✅ Done | newline-delimited JSON, async Tokio          |
| Protocollo controllo              | ✅ Done | Ping, Status, Hatch, Flock, Farewell, Join   |
| DaemonState condiviso             | ✅ Done | Arc + RwLock thread-safe                     |
| PID file                          | ✅ Done | Rilevamento daemon in esecuzione             |
| Multi-rete                        | ✅ Done | Join/leave reti indipendenti                 |
| Metadata estensibili NodeInfo     | ✅ Done | HashMap<String, Value>                       |
| Percorsi XDG-aware                | ✅ Done | `directories` crate                          |

### CLI (`bp-cli`)

| Comando          | Stato   | Note                                         |
|------------------|---------|----------------------------------------------|
| `bp login`       | ✅ Done | Con `--alias` opzionale                      |
| `bp logout`      | ✅ Done | Rimozione irreversibile keypair              |
| `bp hatch`       | ✅ Done | Auto-avvio daemon, metadata pouch/bill       |
| `bp flock`       | ✅ Done | Output formattato con tabelle                |
| `bp farewell`    | ✅ Done | Stop servizio per UUID                       |
| `bp join`        | ✅ Done | Join rete, errore se già joined              |
| `bp --daemon`    | ✅ Done | Avvio daemon interno                         |

### Testing & DevOps

| Componente                    | Stato   |
|-------------------------------|---------|
| 43+ test di architettura      | ✅ Done |
| Integration test end-to-end   | ✅ Done |
| Smoke test Docker 3 nodi      | ✅ Done |
| Playground interattivo 5+1    | ✅ Done |
| CI (fmt + clippy + test)      | ✅ Done |
| Coverage → Codecov            | ✅ Done |
| Docs → GitHub Pages           | ✅ Done |
| Security audit                | ✅ Done |
| Release binari cross-platform | ✅ Done |

---

## Limitazioni attuali (Alpha) ⚠️

| Limitazione                      | Impatto | Descrizione                                               |
|----------------------------------|---------|-----------------------------------------------------------|
| **Nessun trasferimento file**    | Alto    | Pouch, bill, post sono stub — nessun dato viene trasferito |
| **Kademlia in memoria**          | Medio   | `MemoryStore` si svuota ad ogni riavvio del daemon        |
| **Nessuna persistenza gossip**   | Medio   | I peer noti vengono persi al riavvio                      |
| **Nessun NAT traversal**         | Medio   | Funziona su LAN/Docker, non testato attraverso NAT       |
| **Chiave in chiaro su disco**    | Medio   | Nessuna passphrase/cifratura del file identità            |
| **Nessun multi-device sync**     | Basso   | Non è possibile usare la stessa identità su più macchine  |
| **Nessuna autenticazione rete**  | Basso   | Chiunque conosce il network_id può partecipare            |

---

## Roadmap

### In progress 🔨

| Funzionalità              | Descrizione                                   |
|---------------------------|-----------------------------------------------|
| **Pouch — FUSE mount**    | Mount FUSE per offrire storage reale          |
| **Bill — file chunking**  | Chunking + cifratura `age`                    |
| **Post — relay logic**    | Routing bandwidth-aware                       |

### Pianificato 📋

| Funzionalità              | Descrizione                                   |
|---------------------------|-----------------------------------------------|
| Storage marketplace       | Accordi di storage tra utenti                 |
| REST API (axum)           | Adapter HTTP per integrazioni terze           |
| gRPC API (tonic)          | Adapter gRPC per sistemi enterprise           |
| Persistenza Kademlia      | Store su disco (sled/rocksdb) per il DHT      |
| Bootstrap nodes           | Nodi noti per la scoperta iniziale oltre mDNS |

### Futuro 🔮

| Funzionalità              | Descrizione                                   |
|---------------------------|-----------------------------------------------|
| NAT traversal             | AutoNAT + relay circuit v2                    |
| Web dashboard (Tauri)     | UI desktop cross-platform                     |
| Erasure coding (Pouch)    | Ridondanza dei dati con Reed-Solomon          |
| Encryption at rest        | Cifratura file prima del upload               |
| Passphrase identità       | Cifratura del file `identity.key`             |

---

## Changelog recente

### v0.1.3 (Marzo 2026)
- **fix:** build binari nel workflow release-please

### v0.1.2
- **fix:** trigger release su tag `billpouch-v*`

### v0.1.1
- **feat:** smoke test Docker con 3 nodi P2P reali
- **fix:** accetta swarm build failure in ambienti CI limitati
- **fix:** `#[tokio::test]` per il test swarm su Linux (netlink)
- **fix:** Rust image 1.85 per edition 2024
- **fix:** propagazione errori mDNS invece di panic

### v0.1.0
- Release iniziale — architettura completa, CLI funzionante, gossip DHT

# 12. Stato del progetto & Roadmap

## Versione corrente

**v0.1.3** (Alpha) — Marzo 2026

---

## Funzionalità implementate ✅

### Core (`bp-core`)

| Funzionalità                      | Stato   | Note                                                        |
|-----------------------------------|---------|-------------------------------------------------------------|
| Gestione identità Ed25519         | ✅ Done | login, logout, fingerprint, alias                           |
| ServiceType (pouch/bill/post)     | ✅ Done | Parsing case-insensitive                                    |
| ServiceRegistry (in-memory)       | ✅ Done | register, get, remove, all con UUID v4                      |
| libp2p Swarm                      | ✅ Done | gossipsub + Kademlia + mDNS + Identify + Noise + Yamux      |
| Fragment exchange (req/resp)      | ✅ Done | `/billpouch/fragment/1.1.0` via `request_response::cbor`    |
| Gossip NodeInfo                   | ✅ Done | publish, subscribe, deserialize                             |
| NetworkState store                | ✅ Done | upsert, evict_stale(120s), in_network                       |
| Topic naming gossipsub            | ✅ Done | `billpouch/v1/{network_id}/nodes`                           |
| Unix socket IPC                   | ✅ Done | newline-delimited JSON, async Tokio                         |
| Protocollo controllo              | ✅ Done | Ping, Status, Hatch, Flock, Farewell, Join, Leave           |
| **PutFile / GetFile**             | ✅ Done | Encode+store e decode+retrieve via controllo socket         |
| **Distribuzione automatica frammenti** | ✅ Done | PutFile push round-robin ai Pouch remoti via PushFragment |
| **Raccolta frammenti remoti**     | ✅ Done | GetFile fetch da Pouch remoti via FetchChunkFragments se local < k |
| DaemonState condiviso             | ✅ Done | Arc + RwLock + `StorageManagerMap` type alias               |
| PID file                          | ✅ Done | Rilevamento daemon in esecuzione                            |
| Multi-rete                        | ✅ Done | Join/leave reti indipendenti                                |
| Metadata estensibili NodeInfo     | ✅ Done | `HashMap<String, Value>`                                    |
| Percorsi XDG-aware                | ✅ Done | `directories` crate                                         |
| **GF(2⁸) aritmetica**            | ✅ Done | `coding/gf256.rs` — add/mul/inv/div/mul_acc (AES poly)      |
| **RLNC encode/recode/decode**     | ✅ Done | `coding/rlnc.rs` — Gaussian elimination, 9 unit test        |
| **StorageManager**                | ✅ Done | `storage/mod.rs` — quota, disk layout, FragmentIndex        |
| **PouchMeta**                     | ✅ Done | `storage/meta.rs` — meta.json, available_bytes, has_capacity|
| **FragmentIndex**                 | ✅ Done | `storage/fragment.rs` — in-memory HashMap<chunk_id, Vec<>> |

### CLI (`bp-cli`)

| Comando          | Stato   | Note                                                         |
|------------------|---------|--------------------------------------------------------------|
| `bp login`       | ✅ Done | Con `--alias` opzionale                                      |
| `bp logout`      | ✅ Done | Rimozione irreversibile keypair                              |
| `bp hatch`       | ✅ Done | Auto-avvio daemon; Pouch inits StorageManager + quota        |
| `bp flock`       | ✅ Done | Output formattato con tabelle                                |
| `bp farewell`    | ✅ Done | Stop servizio per UUID                                       |
| `bp join`        | ✅ Done | Join rete, errore se già joined                              |
| `bp --daemon`    | ✅ Done | Avvio daemon interno                                         |
| **`bp put`**     | ✅ Done | RLNC encode + store locale + distribute a Pouch remoti      |
| **`bp get`**     | ✅ Done | Decode da frammenti locali + fetch remoti se necessario      |

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

| Limitazione                        | Impatto | Descrizione                                                              |
|------------------------------------|---------|--------------------------------------------------------------------------|
| **Kademlia in memoria**            | Medio   | `MemoryStore` si svuota ad ogni riavvio del daemon                       |
| **Nessuna persistenza gossip**     | Medio   | I peer noti vengono persi al riavvio                                     |
| **Nessun NAT traversal**           | Medio   | Funziona su LAN/Docker, non testato attraverso NAT                       |
| **Chiave in chiaro su disco**      | Medio   | Nessuna passphrase/cifratura del file identità                           |
| **Nessun multi-device sync**       | Basso   | Non è possibile usare la stessa identità su più macchine                 |
| **Nessuna autenticazione rete**    | Basso   | Chiunque conosce il `network_id` può partecipare                         |

---

## Stato sessione di sviluppo (aggiornato 20 Marzo 2026)

Ultimo commit verde atteso: branch `main` (post push).

### Fatto nelle sessioni recenti
| # | Commit | Descrizione |
|---|--------|-------------|
| 1 | `77dbf6a` | feat(core): GF(2⁸) + RLNC (encode/recode/decode) + StorageManager |
| 2 | `9c157be` | fix: non-ASCII em-dash in byte string literal |
| 3 | `1653c2f` | style(fmt): rustfmt 8 diff (rlnc, meta, mod) |
| 4 | `3f8ee9d` | fix(clippy): needless_range_loop → iter_mut/zip |
| 5 | `116c3ab` | feat: PutFile/GetFile, StorageManager integration, fragment exchange, bp put/get CLI |
| 6 | `7dcb05c` | fix(core): &all_fragments in rlnc::decode |
| 7 | `abeb4e4` | style(fmt): 8 diff rustfmt nei file modificati |
| 8 | `cd2d3c9` | fix(core): clone metadata prima di move in ServiceInfo::new |
| 9 | `7320674` | style(fmt): split long use path server.rs |
| 10 | `e039109` | fix(clippy): StorageManagerMap type alias |
| 11 | `95e8a89` | fix(test+clippy): storage_managers in architecture_test + clamp() |
| 12 | `6a37a44` | fix(test): storage_managers in integration_test.rs |
| 13 | `c92573f` | docs: update roadmap with session progress |
| 14 | *(pending)* | feat: P2P fragment distribution + remote fragment collection |

### Prossimi step consigliati
| Priorità | Cosa | Dove |
|----------|------|------|
| 🟡 Media | **Test end-to-end bp put / bp get** — aggiungere scenario in `integration_test.rs` | `tests/integration_test.rs` |
| 🟡 Media | **FragmentIndex gossip** — broadcast `chunk_id + fragment_ids + pouch_peer_id` su gossipsub per discovery distribuita | nuovo `network/fragment_index.rs` |
| 🟡 Media | **Network quality monitor** — ping challenge + proof-of-storage + fault score | vedi `wiki/16-network-quality.md` |
| 🟢 Bassa | **Parametri N/K dinamici** — calcolo ridondanza in base a numero Pouch disponibili | `control/server.rs` → PutFile |
| 🟢 Bassa | **Persistenza Kademlia** | `network/behaviour.rs` → `kad::store::MemoryStore` → file store |

---

## Roadmap

### Pianificato 📋

| Funzionalità | Descrizione |
|---|---|
| FragmentIndex gossip | Indice distribuito `{chunk_id, fragment_id, fragment_hash, pouch_id}` |
| Raccolta frammenti remoti | `GetFile` interroga Pouch remoti via `FetchFragment` se frammenti locali < k |
| Distribuzione automatica | `PutFile` + Hatch pushano frammenti ai Pouch della rete |
| Parametri N/K dinamici | Calcolo ridondanza in base a numero Pouch, stabilità, target durability |
| Rigenerazione preventiva | Recoding automatico quando un Pouch è `suspected` o `blacklisted` |
| Network quality monitor | Challenge Ping + PoS, fault score, blacklist gossip |
| Storage marketplace | Accordi di storage tra utenti |
| REST API (axum) | Adapter HTTP per integrazioni terze |
| Persistenza Kademlia | Store su disco (sled/rocksdb) per il DHT |
| Bootstrap nodes | Nodi noti per la scoperta iniziale oltre mDNS |

### Futuro 🔮

| Funzionalità              | Descrizione                                   |
|---------------------------|-----------------------------------------------|
| NAT traversal             | AutoNAT + relay circuit v2                    |
| Web dashboard (Tauri)     | UI desktop cross-platform                     |
| Encryption at rest        | Cifratura file prima del upload               |
| Passphrase identità       | Cifratura del file `identity.key`             |

---

## Changelog recente

### v0.1.3 (Marzo 2026)
- **feat:** GF(2⁸) + RLNC erasure coding (`coding/gf256.rs`, `coding/rlnc.rs`)
- **feat:** StorageManager con quota enforcement (`storage/mod.rs`, `meta.rs`, `fragment.rs`)
- **feat:** `ControlRequest::PutFile` / `GetFile` — encode+store e decode locale
- **feat:** `DaemonState::storage_managers` (`StorageManagerMap`) — un manager per Pouch attivo
- **feat:** libp2p `request_response::cbor` su `/billpouch/fragment/1.1.0`
- **feat:** `NetworkCommand::PushFragment` / `FetchChunkFragments` con oneshot channel
- **feat:** `bp put <file>` e `bp get <chunk_id>` CLI
- **feat:** Distribuzione automatica frammenti a Pouch remoti (round-robin)
- **feat:** Raccolta frammenti remoti in GetFile (con timeout 10s)
- **feat:** `FragmentRequest` enum (Fetch/FetchChunkFragments/Store) + `FragmentResponse` esteso
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

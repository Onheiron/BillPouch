# 12. Stato del progetto & Roadmap

## Versione corrente

**v0.1.4** (Alpha) — Marzo 2026

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
| **QoS tracking (PeerQos)**        | ✅ Done | `network/qos.rs` — RTT EWMA + challenge success EWMA → stability score |
| **Parametri k/n adattativi**      | ✅ Done | `coding/params.rs` — Poisson-Binomial → k da Ph e stabilità dei peer |
| **FileManifest + NetworkMetaKey** | ✅ Done | `storage/manifest.rs` — metadati file cifrati con chiave di rete BLAKE3 |
| **PutFile con k/n adattativi**    | ✅ Done | `server.rs` — `compute_coding_params()` sostituisce k/n hardcoded dalla CLI |
| **Network quality monitor**       | ✅ Done | `network/quality_monitor.rs` — Ping loop 60s, RTT EWMA → QosRegistry |
| **Proof-of-Storage challenge**    | ✅ Done | `network/quality_monitor.rs` — PoS loop 300s, BLAKE3 challenge, fault score |
| **FragmentIndex gossip**           | ✅ Done | `network/fragment_gossip.rs` — `RemoteFragmentIndex`, topic `billpouch/v1/{net}/index`, targeted GetFile |

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
| `bp put`     | ✅ Done | RLNC encode + store locale + distribute a Pouch remoti; `--ph`/`--q-target` al posto di `--k`/`--n` |
| `bp get`     | ✅ Done | Decode da frammenti locali + fetch remoti se necessario      |

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
| ~~Chiave in chiaro su disco~~      | ✅ Risolto | Argon2id + ChaCha20-Poly1305 — `identity.key.enc` con passphrase opzionale  |
| ~~NAT traversal~~                  | ✅ Risolto | AutoNAT + relay circuit v2 (commit 30)                                  |
| ~~Persistenza Kademlia~~           | ✅ Risolto | `kad_peers.json` su disco (commit 26)                                   |
| **🔴 CEK encryption mancante**    | Critico | I chunk sono cifrati con la chiave di rete, non con la chiave dell'utente. Chi è nella rete può leggere i dati altrui. |
| **🔴 NetworkMetaKey derivabile**  | Critico | La chiave dei metadati è `BLAKE3("billpouch/meta/v1" \|\| network_id)` — derivabile da chiunque conosca il nome della rete. Deve essere una chiave segreta random distribuita tramite invito. |
| **🟠 Nessun sistema di inviti**   | Alto    | Chiunque conosce il `network_id` può unirsi. L'accesso deve essere su invito firmato e cifrato. |
| **Nessun multi-device**           | Basso   | Manca `bp export-identity` / `bp import-identity` per spostare il keypair su un'altra macchina. |

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
| 14 | `343d8bb` | feat(core): QoS tracking, adaptive k params, FileManifest + NetworkMetaKey |
| 15 | `dbeda3f` | style: fix cargo fmt violations |
| 16 | `6c691bc` | fix(core): clippy — unused var, excessive precision |
| 17 | `a78636c` | fix(core): reversed Horner coefficients in erfc_approx |
| 18 | `616323e` | feat: PutFile adaptive k/n + QosRegistry in DaemonState |
| 19 | `03642d4` | fix(tests): missing qos field in test DaemonState helpers |
| 20 | `4451817` | style: fmt diffs put.rs, server.rs, architecture_test.rs |
| 21 | `dc06e5e` `4eb6b1b` `c418cdc` `7d199a1` | feat: network quality monitor — Ping challenge loop, RTT→QoS |
| 22 | `7461367` `702d8d8` `7a9dee4` `bd294f7` | feat: Proof-of-Storage challenge — fault score, FragmentRequest::ProofOfStorage, OutgoingAssignments |
| 23 | `5bf7c86` `e33bab8` `b3c7637` `bd9d27b` | feat: FragmentIndex gossip — RemoteFragmentIndex, AnnounceIndex, targeted GetFile fetch |
| 24 | `6465596` `6b5fd13` | test: end-to-end PutFile/GetFile with adaptive k/n and local roundtrip |
| 25 | `599aade` `39e6002` `031b85b` | feat: Rigenerazione Preventiva — rerouting frammenti quando fault_score ≥ FAULT_SUSPECTED |
| 26 | `9ffa920` `52a0a87` | feat: Persistenza Kademlia — salvataggio/ripristino peer su disco (kad_peers.json) |
| 27 | `16ff6ed` `cfd8ab5` | feat: Bootstrap nodes — nodi noti per scoperta iniziale (bootstrap.json) |
| 28 | `d709864` `28bcfb1` | feat: `bp bootstrap list/add/remove` — CLI per gestire bootstrap.json |
| 29 | `6e5ca17` `1a258b8` `678007e` | feat: `bp-api` — REST API Axum (GET /status, /peers, /files; POST /hatch, /files; DELETE /services) |
| 30 | `90cec5e` `1ae4a44` `744f421` `2439865` `1d23d51` `1b02520` | feat: **NAT traversal** — AutoNAT + relay client (`network/behaviour.rs`, `network/mod.rs`) |
| 31 | `b9fd5a7` `7f8ded7` `2d5b3ee` `f5b85cf` `78eaaa2` | feat: **Storage marketplace** — accordi di storage gossipati tra utenti |
| 32 | `f4045c5` `871bfc2` `5f35f4c` `214a1fd` | feat: **Passphrase identità** + fix CI (fmt, clap env, rlnc flaky tests) |
| 33 | `b0ada4a` | feat: **Encryption at rest** — ChaCha20-Poly1305 prima di RLNC |

### Prossimi step consigliati
| Priorità | Cosa | Dove |
|----------|------|---------|
| 🔴 Critica | **CEK per-utente** — ogni file ottiene una Content Encryption Key derivata dalla chiave privata dell'utente; `NetworkMetaKey` cifra solo i manifest, mai i chunk; `FileManifest` include `encrypted_cek` per ogni destinatario autorizzato (ECIES su Ed25519→X25519) | `storage/encryption.rs`, `storage/manifest.rs`, `control/server.rs` |
| 🔴 Critica | **NetworkMetaKey come segreto di rete** — generata casualmente alla creazione della rete, non derivata dal `network_id`; storata cifrata localmente; distribuita solo via token di invito | `storage/manifest.rs`, `config.rs`, `control/server.rs` |
| 🟠 Alta | **Sistema di inviti** — `bp invite create --for <fingerprint> [--network] [--ttl]` genera un token firmato (Ed25519) e cifrato (ECIES) che include `network_id` + `NetworkMetaKey` + scadenza + fingerprint destinatario; `bp join --invite <blob>` verifica firma e decifra | nuovo `control/protocol.rs` + `commands/invite.rs` |
| 🔵 Bassa | **Multi-device** — `bp export-identity` / `bp import-identity` per spostare `identity.key.enc` su un'altra macchina | `commands/auth.rs` |
| 🔵 Bassa | **Web dashboard** — UI servita da `bp-api` (HTML/JS statici in Axum, nessuna dipendenza Node.js) | `bp-api/static/` |

---

## Roadmap

### Pianificato 📋

| Funzionalità | Stato | Descrizione |
|---|---|---|
| CEK per-utente + ECIES sharing | ⏳ Prossimo | Chunk cifrati con chiave derivata dal keypair utente; condivisione tramite encrypted_cek nel manifest |
| NetworkMetaKey come segreto di rete | ⏳ Prossimo | Chiave random generata alla creazione rete, non derivabile dal network_id |
| Sistema di inviti | ⏳ Prossimo | Token firmato+cifrato per accesso controllato; veicola NetworkMetaKey al nuovo membro |
| Multi-device identity | ⏳ Pianificato | `bp export-identity` / `bp import-identity` |
| Web dashboard | ⏳ Pianificato | UI HTML/JS servita da `bp-api` Axum |

### Futuro 🔮

| Funzionalità              | Descrizione                                   |
|---------------------------|-----------------------------------------------|
| NAT traversal             | ✅ Done — AutoNAT + relay circuit v2          |
| Passphrase identità       | ✅ Done — Argon2id + ChaCha20-Poly1305 su `identity.key.enc` |
| Encryption at rest (v1)   | ⚠️ Parziale — ChaCha20-Poly1305 per-chunk presente, ma usa NetworkMetaKey invece di CEK utente → da correggere |
| CEK per-utente            | ⏳ Chunk cifrati con chiave derivata dalla chiave privata dell'utente; ECIES per condivisione file tra più identità |
| NetworkMetaKey segreta    | ⏳ Chiave di rete random, non derivabile da `network_id`; distribuita via invite token |
| Sistema di inviti         | ⏳ Accesso a rete su invito firmato+cifrato; unico vettore di distribuzione della NetworkMetaKey |

---

## Changelog recente

### v0.1.6 (Marzo 2026)
- **feat:** Encryption at rest (v1) — `storage/encryption.rs` `ChunkCipher`; ChaCha20-Poly1305; PutFile cifra prima di RLNC, GetFile decifra dopo decode; 6 unit test — ⚠️ usa NetworkMetaKey come chiave chunk (da correggere con CEK utente)
- **fix(rlnc):** encoding sistematico (identity matrix per frammenti 0..k); tutti i test decode deterministici; guardia zero-vector in `recode()`

### v0.1.5 (Marzo 2026)
- **feat:** Passphrase identità — `EncryptedKeyFile` (Argon2id + ChaCha20-Poly1305), cifratura opzionale in `identity.rs`; `--passphrase` flag globale in CLI; daemon legge passphrase da `--passphrase` o `BP_PASSPHRASE`

### v0.1.4 (Marzo 2026)
- **feat:** Storage marketplace — `StorageOffer`, `AgreementStore`, `AcceptStorage`, CLI `bp offer/agree/agreements/offers`, API marketplace endpoints
- **feat:** NAT traversal — AutoNAT + relay client; `bp relay connect`
- **feat:** Bootstrap nodes — `bp bootstrap list/add/remove`
- **feat:** REST API Axum `bp-api` — 14 endpoint HTTP
- **feat:** `network/fragment_gossip.rs` — `FragmentIndexAnnouncement`, `RemoteFragmentIndex`; `NetworkCommand::AnnounceIndex`; PutFile pubblica indice, GetFile fetch mirato
- **feat:** `network/qos.rs` — `PeerQos` + `QosRegistry` + `fault_score` con soglie degraded/suspected/blacklisted
- **feat:** `coding/params.rs` — `compute_coding_params()`, `effective_recovery_probability()`
- **feat:** `storage/manifest.rs` — `FileManifest`, `NetworkMetaKey`
- **feat:** `DaemonState.qos` + `DaemonState.outgoing_assignments` + `PutFile` adattivo (`--ph`/`--q-target`)
- **feat:** `network/quality_monitor.rs` — loop Ping 60s + loop PoS 300s, RTT+fault EWMA → `QosRegistry`; `FragmentRequest::ProofOfStorage`; `NetworkCommand::ProofOfStorage`; `OutgoingAssignments`
- **fix:** coefficienti Horner `erfc_approx`, clippy, fmt

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

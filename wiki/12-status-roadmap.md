# 12. Stato del progetto & Roadmap

## Versione corrente

**v0.2.1** (Alpha) — Marzo 2026  
**v0.3.0-dev** — feature implementate, cleanup/test in corso

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
| **StorageTier T1–T5**              | ✅ Done | `storage/tier.rs` — `quota_bytes()`, `parse()`, `for_file_size()`, serde, 7 unit test |
| **ReputationTier R0–R4**           | ✅ Done | `network/reputation.rs` — `ReputationRecord`, `ReputationStore`, 9 unit test |
| **ServiceStatus::Paused**          | ✅ Done | `service.rs` — `Paused { eta_minutes, paused_at }`, `Stopping`, `Error(String)` |
| **One-Pouch-per-network**          | ✅ Done | `control/server.rs` — rifiuta secondo `hatch pouch` su stesso network per stessa identità |

### CLI (`bp-cli`)

| Comando          | Stato   | Note                                                         |
|------------------|---------|--------------------------------------------------------------|
| `bp login`       | ✅ Done | Con `--alias` opzionale                                      |
| `bp logout`      | ✅ Done | Rimozione irreversibile keypair                              |
| `bp hatch`       | ✅ Done | Auto-avvio daemon; Pouch inits StorageManager + quota; `--tier T1..T5`; one-Pouch-per-network enforced |
| `bp flock`       | ✅ Done | Output formattato con tabelle                                |
| `bp farewell`    | ✅ Done | Stop servizio per UUID; `--evict` per eviction permanente (purge + gossip + reputation) |
| `bp pause`       | ✅ Done | Manutenzione temporanea con ETA; gossip maintenance; `ServiceStatus::Paused` |
| `bp resume`      | ✅ Done | Ripristina servizio paused; re-annuncia gossip willing        |
| `bp leave`       | ✅ Done | Abbandona network; precondition check servizi attivi; blocklist hint |
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
| ~~CEK encryption mancante~~        | ✅ Risolto | `ChunkCipher::for_user` — CEK derivata da identity + BLAKE3(plaintext) (commit 34) |
| ~~NetworkMetaKey derivabile~~      | ✅ Risolto | `NetworkMetaKey::generate/load/save` — chiave random in `network_keys.json` (commit 34) |
| ~~Nessun sistema di inviti~~       | ✅ Risolto | `bp invite create / join` — token firmato+cifrato con password (commit 35) |
| **Multi-device identity**         | ✅ Done | `bp export-identity` / `bp import-identity` — portabilità del keypair tra macchine |

---

## Stato sessione di sviluppo (aggiornato 23 Marzo 2026)

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
| 33 | `b0ada4a` `9366e30` | feat: **Encryption at rest** — ChaCha20-Poly1305 prima di RLNC; fix rustfmt |
| 34 | `23780cb` | feat: **CEK per-utente** + **NetworkMetaKey segreta** — `ChunkCipher::for_user` (CEK da identity + BLAKE3 plaintext); `NetworkMetaKey::generate/load/save` (random, non derivabile dal nome); `chunk_cek_hints` in `DaemonState`; `Identity::secret_material()` |
| 35 | `6be3981` | feat: **Sistema di inviti** — `invite.rs` (`create_invite`, `redeem_invite`, `save_invite_key`); token firmato Ed25519 + cifrato Argon2id+ChaCha20-Poly1305; `bp invite create / join`; Join auto-genera `NetworkMetaKey` per reti nuove; 5 unit test |
| 36 | `a551618` | fix: rimosse `&` spurie in `ControlClient::send()` in `commands/invite.rs` |
| 37 | `8954401` | style: cargo fmt — reformatting `invite.rs`, `server.rs`, `commands/invite.rs` |
| 38 | `068300e` | test: `ENV_LOCK` mutex in test invite — serializzazione test che mutano `HOME`/`XDG_DATA_HOME` |
| 39 | `487a26d` | feat: **Multi-device identity** — `ExportedIdentity`, `ExportedKeyData`; `Identity::export_to_file` / `import_from_file`; `bp export-identity --out` / `bp import-identity [--force]` |
| 40 | `d4cfa3b` | feat: **Web dashboard** — UI HTML/JS embedded in `bp-api`; `GET /` restituisce la dashboard; auto-refresh 5s |
| 41 | `various` | refactor: **Rimozione storage marketplace** — `StorageOffer/Agreement`, `ProposeStorage/AcceptStorage/ListAgreements/ListOffers`, `bp offer/agree` CLI, REST `/marketplace/*` |
| 42 | `various` | feat: **StorageTier** T1–T5 (`storage/tier.rs`); `bp hatch pouch --tier`; one-Pouch-per-network enforced |
| 43 | `various` | feat: **ReputationTier** R0–R4 (`network/reputation.rs`); `ReputationStore` in `DaemonState` |
| 44 | `various` | feat: **Pause/Resume** — `bp pause --eta` / `bp resume`; `ServiceStatus::Paused`; gossip maintenance |
| 45 | `various` | feat: **FarewellEvict** — `bp farewell --evict`; `StorageManager::purge()`; reputation evict_without_notice |
| 46 | `various` | feat: **Leave precondition** — `bp leave` blocca se servizi attivi; hint comandi di stop |
| 47 | `e0bee5e` | docs+tests: wiki 04/05/08; nuovi test architecture e integration per v0.3 features |

---

## Roadmap v0.3 — Design finalizzato (Marzo 2026)

Le decisioni riportate qui sono **design finalized** — implementazione da iniziare.

### 🔴 Breaking changes: rimozioni

| Componente | Azione | Stato |
|---|---|---|
| `bp join <network>` (CLI pubblico) | Rimosso — rimane interno hidden | ✅ Done |
| `bp leave <network>` | Ridisegnare completamente (vedi §Leave) | 🔲 Todo |
| `bp propose-storage` / `bp accept-storage` | Eliminati | ✅ Done |
| `StorageOffer` / `Agreement` | Eliminati | ✅ Done |
| `ControlRequest::ProposeStorage` / `AcceptStorage` / `ListAgreements` / `ListOffers` | Eliminati | ✅ Done |

### 🟡 Modifiche a comandi esistenti

#### `bp hatch pouch` — Storage tier invece di quota libera — ✅ Done

Attualmente accetta `--storage-bytes N` (valore libero). Questo viene sostituito da tier discreti:

```
bp hatch pouch --tier <T1|T2|T3|T4|T5> [--network <id>]
```

**Non è accettato un valore arbitrario di byte.** L'utente sceglie esattamente uno dei tier predefiniti — questo è il solo valore valido che il Pouch dichiara al network.

I tier di storage sono:

| Tier | Dimensione esatta | Nome |
|---|---|---|
| T1 | 10 GB | Pebble |
| T2 | 100 GB | Stone |
| T3 | 500 GB | Boulder |
| T4 | 1 TB | Rock |
| T5 | 5 TB | Monolith |

Un Pouch appartiene al suo tier **e a tutti i tier inferiori**. Un Pouch T3 partecipa ai calcoli N/k/q di T1, T2 e T3.

**Un solo Pouch per macchina per network.** Avere due Pouch sulla stessa macchina nello stesso network crea correlazione della QoS (se la macchina va offline, entrambi i Pouch spariscono insieme), il che vanifica il vantaggio dello storage distribuito. Il daemon deve rifiutare il secondo `hatch pouch --network X` sulla stessa identità se esiste già un Pouch attivo su X — con errore esplicito: `"a Pouch for network X already exists on this node; two correlated Pouches defeat distributed redundancy"`.

#### `bp hatch` senza `--network` — modalità locale

Se `--network` non viene specificato, il servizio gira in **modalità locale**: nessun gossip, nessun peer, storage e accesso solo da parte dell'identità locale (comportamento analogo a GlusterFS). Utile per test, staging, o uso single-device.

### 🟢 Nuovi comandi

#### `bp leave <network>` — procedura di abbandono asincrona — ✅ Done (v1)

Abbandono di un network è una procedura multi-step **non interrompibile** una volta avviata:

1. **Precondition:** Blocca se l'utente ha servizi attivi su quel network (mostra lista)
2. Per ogni Pouch attivo su quel network → avvia **procedura di eviction Pouch** (vedi sotto)
3. Per ogni chunk del quale l'utente è owner (Bill fragments su peer remoti):
   - Fetch di tutti i fragment raggiungibili
   - Ricostruisce il file localmente
   - Presenta lista: "Questi file non saranno più recuperabili dopo l'abbandono. Vuoi scaricarli? [lista con size]"
   - L'utente confirma o rinuncia a ciascuno
4. Quando eviction completa e file materializzati → unsubscribe gossip + rimozione da `active_networks`
5. Progress disponibile via `bp leave-status <network>`

#### `bp pause <service_id> --eta <minutes>` — manutenzione temporanea — ✅ Done

Per spegnimento pianificato con garanzia di ritorno:

1. Annuncia via gossip: "manutenzione, torno entro T minuti"
2. Gli altri peer marcano i suoi fragment come `temporarily_unavailable` (non `lost`)
3. Se non torna entro T → `fault_score++` automatico per ogni PoS challenge mancato
4. Al ritorno → PoS challenge immediato; se passa → nessuna penalità reputazione

#### `bp farewell <service_id> --evict` — rimozione permanente con eviction — ✅ Done (v1)

Procedura asincrona per rimozione definitiva di un Pouch:

1. Annuncia network: "offline definitivo"
2. Per ogni chunk ospitato: propaga i fragment ai peer disponibili (re-distribute fino a raggiungere di nuovo N fragment totali)
3. Presenta all'utente: "Con questo Pouch stai rimuovendo X GB. Questi file dipendono dalla tua quota locale. Quali vuoi materializzare localmente? [lista]"
4. L'utente sceglie. I file non scelti rimangono nel network (finché ci sono ≥k fragment raggiungibili)
5. Solo dopo eviction confermata → Pouch rimosso da `ServiceRegistry`

### 🔵 Nuovo sottosistema: Network Tiering

#### Storage tier — `storage/tier.rs` — ✅ Done

I tier definiscono sia la **dimensione del Pouch** sia la **dimensione dei file** che possono essere distribuiti su quel tier. Un fragment di un file T2 (10–100 GB) non può fisicamente stare su un Pouch T1 (10 GB totali), quindi i calcoli N/k/q sono naturalmente separati per tier.

Calcola N, k, q **per tier** basandosi sullo stato corrente del network:

- **T1 file** (< 10 GB): N = tutti i Pouch (T1+T2+T3+T4+T5), k e q calcolati su tutti
- **T2 file** (10–100 GB): N = Pouch T2+, k e q solo su quelli
- **T3 file** (100–500 GB): N = Pouch T3+
- **T4 file** (500 GB–1 TB): N = Pouch T4+
- **T5 file** (> 1 TB): N = Pouch T5 only

Lo **storage netto** disponibile per un utente con Pouch tier T è la somma dei contributi per ogni tier in cui quel Pouch partecipa, al netto dell'overhead RLNC calcolato live:

$$\text{storage\_netto}(T) = \sum_{i=1}^{T} \frac{S_{\text{tier}_i}}{q_{\text{tier}_i}}$$

dove $S_{\text{tier}_i}$ è la quota fissa del tier $i$ (10 GB, 100 GB, ecc.) e $q_{\text{tier}_i}$ è il coefficiente RLNC calcolato live dai QoS score dei Pouch di quel tier.

**Vincolo di unicità:** il daemon rifiuta un secondo `hatch pouch --network X` se esiste già un Pouch attivo per quel network su questa identità:
```
Error: a Pouch for network X already exists on this node.
       Two correlated Pouches on the same machine defeat distributed redundancy.
       Use `bp farewell --evict <service_id>` to remove the existing one first.
```

#### Reputation tier — `network/reputation.rs` — ✅ Done

| Tier | Nome | Criteri di ingresso |
|---|---|---|
| R0 | Quarantine | `fault_score > threshold_blacklist`; eviction forzata; dati persi senza notifica |
| R1 | Fledgling | Nodo nuovo (< 7 giorni nel network) |
| R2 | Reliable | Uptime ≥ 95% su 30 giorni; PoS pass rate ≥ 98% |
| R3 | Trusted | Uptime ≥ 99% su 90 giorni; PoS pass rate ≥ 99.5% |
| R4 | Pillar | Uptime ≥ 99.9% su 365 giorni; contributo ≥ T3 |

**Regola di placement:** i fragment di file appartenenti a utenti con reputation ≥ R2 vengono preferenzialmente piazzati su Pouch con reputation ≥ R2. I Pouch R0 ricevono solo fragment di utenti R0 — la quarantine è **isolata**.

**Progressione reputazione:**
- Ogni PoS challenge passato → `reputation_score += PASS_INC`
- Ogni PoS challenge fallito → `reputation_score -= FAIL_DEC`
- Ogni 24h di uptime verificato → `reputation_score += UPTIME_INC`
- Pausa con `--eta` rispettata → nessuna penalità
- Pausa con `--eta` sforata → `reputation_score -= LATE_PENALTY`
- Eviction forzata (sparito senza preavviso) → `reputation_score = 0`, tier forzato R0 per 30 giorni

---

## Roadmap — backlog futuro 🔮

| Funzionalità | Priorità | Descrizione |
|---|---|---|
| FUSE mount | Alta | `bp mount <network> <mountpoint>` — filesystem nativo |
| CEK persistence | Alta | Cifrare e persistere `chunk_cek_hints` su disco — sopravvive al riavvio del daemon |
| Bootstrap pubblici | Alta | 3+ nodi pubblici always-on per cold-start del network |
| gRPC API | Media | Alternativa programmatica alla socket Unix control |
| Mobile client | Media | iOS/Android via UniFFI bindings a `bp-core` |
| Content deduplication | Media | Content-addressed storage per file identici dallo stesso utente |
| Resumable uploads | Bassa | Checkpoint state per file grandi |
| Personal AI data vault | Bassa | Strutturazione dei dati per fine-tuning AI locale |

---

## Changelog recente

### v0.3.0-dev (Marzo 2026) — in corso
- **refactor:** Rimosso intero storage marketplace — `StorageOffer`, `StorageAgreement`, `AgreementStore`, `ControlRequest::{ProposeStorage,AcceptStorage,ListAgreements,ListOffers}`, `NetworkCommand::AnnounceOffer`, `/marketplace/*` REST endpoints, CLI `bp offer/agree/agreements/offers`
- **refactor:** `bp join` rimosso dal CLI pubblico — resta come comando nascosto usato dagli script (gossipsub topic subscription); accesso alle reti solo via `bp invite join`
- **feat:** `storage/tier.rs` — `StorageTier` enum (T1–T5 = 10GB/100GB/500GB/1TB/5TB), `quota_bytes()`, `participating_tiers()`, `for_file_size()`, `parse()`, serde, 7 unit test
- **feat:** `BpError::InvalidInput` — nuova variante per input non validi (es. tier sconosciuto)
- **feat:** `bp hatch pouch --tier T2` — sostituisce `--storage-bytes`; one-Pouch-per-network enforced in server
- **feat:** `network/reputation.rs` — `ReputationTier` (R0–R4), `ReputationRecord`, `ReputationStore`; integrato in `DaemonState`
- **feat:** `bp pause <service_id> --eta <minutes>` / `bp resume <service_id>` — manutenzione temporanea; gossip maintenance announcement; `ServiceStatus::Paused`; `ControlRequest::{Pause,Resume}`
- **feat:** `bp farewell <service_id> --evict` — eviction permanente Pouch; purge storage su disco; gossip `evicting=true`; `ControlRequest::FarewellEvict`; `StorageManager::purge()` + `storage_summary()`; reputation `evict_without_notice()`
- **feat:** `bp leave <network>` — precondition check servizi attivi; unsubscribe gossip; `ControlRequest::Leave` aggiornato con blocklist servizi da fermare prima

### v0.2.1 (Marzo 2026)
- **feat:** Web dashboard — `GET /` in `bp-api` restituisce una SPA HTML/JS embedded via `include_str!`; dark UI; sezioni Status, Peers, Files, Marketplace; auto-refresh 5s; zero dipendenze runtime

### v0.2.0 (Marzo 2026)
- **feat:** Multi-device identity — `Identity::export_to_file(dest)` legge l’identità da disco (plaintext o cifrata) e produce un bundle JSON portabile (`ExportedIdentity`); `Identity::import_from_file(src, overwrite)` installa keypair e profilo nel data dir XDG; `ExportedKeyData` enum con variante `Plaintext { key_hex }` e `Encrypted(EncryptedKeyFile)`; `bp export-identity --out <file>` e `bp import-identity <file> [--force]`

### v0.1.9 (Marzo 2026)
- **fix:** `ControlClient::send()` non richiedeva `&ControlRequest` — rimosso `&` spurio in `commands/invite.rs`
- **style:** `cargo fmt` su `invite.rs`, `server.rs`, `commands/invite.rs`
- **test:** `ENV_LOCK` mutex nei test `invite` per evitare race condition su `HOME`/`XDG_DATA_HOME` in esecuzione parallela

### v0.1.8 (Marzo 2026)
- **feat:** Sistema di inviti — `crates/bp-core/src/invite.rs`; `InvitePayload` firmato Ed25519 + cifrato Argon2id+ChaCha20-Poly1305; `create_invite()` / `redeem_invite()` / `save_invite_key()`; `ControlRequest::CreateInvite`; `bp invite create [--network] [--for-fingerprint] [--ttl] --invite-password`; `bp invite join <blob> --invite-password`; `Join` ora genera automaticamente una NetworkMetaKey random per reti create ex-novo; `fingerprint_pubkey()` in identity.rs; 5 unit test (roundtrip, wrong password, expired, tampered, persist)

### v0.1.7 (Marzo 2026)
- **feat:** CEK per-utente — `Identity::secret_material()` (BLAKE3 del keypair); `ChunkCipher::for_user(secret_material, plaintext_hash)` — CEK = `BLAKE3_keyed(secret, "billpouch/cek/v1" || BLAKE3(chunk))`; `chunk_cek_hints: RwLock<HashMap<chunk_id, [u8;32]>>` in `DaemonState`; PutFile/GetFile aggiornati
- **feat:** `NetworkMetaKey` come segreto di rete — `generate()` + `load()` + `save()` + `load_or_create()` su `network_keys.json`; `for_network()` deprecato a `#[doc(hidden)]`
- **feat:** `config::network_keys_path()` — `~/.local/share/billpouch/network_keys.json`

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

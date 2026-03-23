# 9. Testing

## Struttura dei test

```
crates/bp-core/tests/
├── architecture_test.rs    # 60+ test unitari — verifica comportamento documentato
└── integration_test.rs     # test end-to-end — daemon reale su Unix socket
```

---

## Test di architettura (`architecture_test.rs`)

Contiene **60+ test unitari** che verificano che il codice si comporti
esattamente come descritto nel README e nella documentazione.
Sono organizzati in sezioni numerate:

| Sezione | Argomento                                         |
|---------|---------------------------------------------------|
| 1       | Identità: login, fingerprint, alias, peer_id      |
| 2       | Tre servizi: pouch, bill, post (case-insensitive) |
| 3       | ServiceRegistry: register, get, remove, all       |
| 4       | Gossip: NetworkState, upsert, eviction stale peer |
| 5       | Topic naming gossipsub                            |
| 6       | Protocollo controllo: ControlRequest/Response JSON|
| 7       | NodeInfo: serializzazione/deserializzazione JSON  |
| 8       | NetworkCommand: join, leave, announce             |
| 9       | Swarm libp2p: costruzione con tutti i protocolli  |
| 10      | Scenario completo: rete "amici" Carlo/Marco/Lucia |
| 11      | Error handling                                    |
| 12      | StorageTier: quota bytes, parse, ordering         |
| 13      | ReputationTier: tier ordering, is_eligible_for    |
| 14      | ServiceStatus::Paused: display, serializzazione   |

### Selezione test notevoli

**`servizi_tre_tipi_pouch_bill_post`**
```rust
assert_eq!("pouch".parse::<ServiceType>().unwrap(), ServiceType::Pouch);
assert_eq!("BILL".parse::<ServiceType>().unwrap(),  ServiceType::Bill);
assert_eq!("Post".parse::<ServiceType>().unwrap(),  ServiceType::Post);
```

**`gossip_evict_stale_rimuove_nodi_vecchi`**
Verifica che nodi con `announced_at` più vecchio di 120 secondi
vengano rimossi da `NetworkState::evict_stale(120)`.

**`topic_name_formato_corretto`**
```rust
assert_eq!(
    NodeInfo::topic_name("my-network"),
    "billpouch/v1/my-network/nodes"
);
```

**`swarm_si_costruisce_con_tutti_i_protocolli`**
Test `#[tokio::test]` — costruisce il swarm libp2p completo con tutti
i protocolli (incluso mDNS che richiede netlink su Linux).

**`scenario_rete_amici_carlo_marco_lucia`**
Simula:
- Carlo: 1 pouch NAS (10 GB) + 1 bill laptop
- Marco: 2 pouch (desktop 50 GB + VPS 100 GB) + 1 post VPS
- Lucia: 1 bill laptop

Verifica: 6 nodi totali, 3 fingerprint distinte, storage totale 160 GB.

---

## Integration test (`integration_test.rs`)

Test end-to-end — verifica il percorso completo dell'utente e i nuovi
flussi v0.3:

**`full_user_journey`** — hatch ×3, flock, status, join, farewell, ping, error cases

**`put_file_get_file_roundtrip`** — encode RLNC + store + decode locale

**`put_file_without_pouch_returns_error`** — PutFile senza Pouch attivo → errore esplicito

**`get_file_unknown_chunk_returns_error`** — GetFile chunk_id inesistente → errore

**`get_file_missing_cek_hint_returns_error`** — GetFile dopo riavvio daemon (hint scomparso) → errore CEK

**`pause_resume_roundtrip`** (v0.3) — hatch pouch → Pause → verifica `status=paused` → Resume → verifica `status=running`

**`farewell_evict_removes_service`** (v0.3) — hatch pouch → FarewellEvict → servizio scompare da flock

**`leave_blocked_by_active_service`** (v0.3) — hatch → Leave → verifica `blocked=true` + service_id in lista

**`hatch_second_pouch_same_network_rejected`** (v0.3) — hatch pouch → secondo hatch pouch stesso network → errore

---

## Smoke test Docker

File: [smoke/smoke-test.sh](../smoke/smoke-test.sh)

Verifica che 3 nodi Docker si scoprano via mDNS e si scambino NodeInfo via gossipsub.

### Passi del smoke test

| Step | Descrizione                                                    | Timeout |
|------|----------------------------------------------------------------|---------|
| 1    | Attesa daemon ready (ping/pong) su tutti i nodi                | 30 s    |
| 2    | Attesa formazione mesh mDNS                                    | 60 s    |
| 3    | Hatch dei servizi (pouch, bill, post)                          | —       |
| 4    | Attesa propagazione gossipsub                                  | 30 s    |
| 5    | Verifica peer discovery (ogni nodo vede ≥ 2 peer)             | 60 s    |
| 6    | Cross-visibility: bill vede pouch, pouch vede post, ecc.       | —       |
| 7    | Health check ping/pong su tutti i 3 nodi                       | —       |
| 8    | Verifica membership rete "smoketest"                           | —       |
| 9    | Dump log finali per debugging                                  | —       |

### Cluster smoke test

| Container  | NODE_NAME     | Servizio | Rete        |
|------------|---------------|----------|-------------|
| `bp-pouch` | `carlo-pouch` | `pouch`  | `smoketest` |
| `bp-bill`  | `marco-bill`  | `bill`   | `smoketest` |
| `bp-post`  | `lucia-post`  | `post`   | `smoketest` |

Rete Docker: `bp-lan` (bridge isolata)

---

## Esecuzione dei test

```bash
# Tutti i test del workspace
cargo test --workspace

# Solo test di architettura (bp-core)
cargo test -p bp-core --test architecture_test

# Solo integration test
cargo test -p bp-core --test integration_test

# Un test specifico
cargo test -p bp-core --test architecture_test scenario_rete_amici

# Coverage (richiede cargo-tarpaulin)
cargo tarpaulin --packages bp-core --test architecture_test --out xml

# Smoke test (richiede Docker)
docker compose -f docker-compose.smoke.yml up --build -d
./smoke/smoke-test.sh
docker compose -f docker-compose.smoke.yml down

# CI locale (fmt + clippy + test)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

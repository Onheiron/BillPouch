# 9. Testing

## Struttura dei test

```
crates/bp-core/tests/
├── architecture_test.rs    # 43+ test unitari — verifica comportamento documentato
└── integration_test.rs     # 1 test end-to-end — daemon reale su Unix socket
```

---

## Test di architettura (`architecture_test.rs`)

Contiene **43+ test unitari** che verificano che il codice si comporti
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

Test `full_user_journey` — verifica il percorso completo dell'utente:

1. Avvia un daemon reale su Unix socket (processo in-process)
2. `bp login --alias "test-pelican"` — crea identità
3. `bp hatch pouch --network amici` — avvia storage
4. `bp hatch bill  --network amici` — avvia file I/O
5. `bp hatch post  --network amici` — avvia relay
6. `bp flock` → verifica 3 servizi locali
7. `bp status` → verifica peer_id, fingerprint, alias, networks
8. `bp join lavoro` → seconda rete
9. `bp status` → verifica networks = ["amici", "lavoro"]
10. `bp farewell <service_id_post>` → ferma il relay
11. `bp flock` → verifica 2 servizi rimasti
12. `bp farewell <id_inesistente>` → verifica errore
13. `bp join amici` → verifica errore "already joined"

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

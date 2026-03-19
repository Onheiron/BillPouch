# 3. Tecnologie utilizzate

## Linguaggio e runtime

| Tecnologia       | Versione | Uso                                       |
|------------------|----------|-------------------------------------------|
| **Rust**         | ≥ 1.85   | Linguaggio principale (edition 2021)      |
| **Tokio**        | 1.x      | Runtime async (`features = ["full"]`)     |

> Rust 1.85 è richiesto per il supporto edition 2024 nelle dipendenze di libp2p.

## Networking P2P

| Libreria              | Versione | Uso                                          |
|-----------------------|----------|----------------------------------------------|
| **libp2p**            | 0.54     | Stack P2P completo                           |
| └ `gossipsub`         | —        | Broadcast NodeInfo per network               |
| └ `kad`               | —        | DHT Kademlia, store `MemoryStore`            |
| └ `identify`          | —        | Scambio metadati peer (`/billpouch/id/1.0.0`)|
| └ `mdns` (tokio)      | —        | Scoperta LAN zero-config                     |
| └ `noise`             | —        | Cifratura trasporto                          |
| └ `yamux`             | —        | Multiplexing stream                          |
| └ `tcp`               | —        | Trasporto base                               |
| └ `ed25519`           | —        | Keypair per identità libp2p                  |

## Serializzazione & Dati

| Libreria       | Versione | Uso                                           |
|----------------|----------|-----------------------------------------------|
| **serde**      | 1.x      | Serializzazione/deserializzazione (derive)    |
| **serde_json** | 1.x      | JSON per il protocollo di controllo e NodeInfo|
| **uuid**       | 1.x      | UUID v4 per service ID (`features = ["v4", "serde"]`) |
| **chrono**     | 0.4      | Timestamp `announced_at` e `started_at`       |
| **sha2**       | 0.10     | Hash SHA-256 per la fingerprint utente        |
| **hex**        | 0.4      | Encoding hex della fingerprint                |

## Error handling & Utility

| Libreria           | Versione | Uso                                          |
|--------------------|----------|----------------------------------------------|
| **anyhow**         | 1.x      | Error handling flessibile nella CLI          |
| **thiserror**      | 1.x      | Definizione `BpError` strutturato in bp-core |
| **tracing**        | 0.1      | Logging strutturato con spans e fields       |
| **directories**    | 5.x      | Percorsi XDG-aware (config, data, cache)     |
| **futures**        | 0.3      | Combinatori async aggiuntivi (`StreamExt`)   |

## CLI

| Libreria               | Versione | Uso                                      |
|------------------------|----------|------------------------------------------|
| **clap**               | 4.x      | Parser CLI con derive macro + colori     |
| **tracing-subscriber** | 0.3      | Output log con `EnvFilter` e `RUST_LOG`  |

## Tooling di sviluppo

| Strumento           | Uso                                                  |
|---------------------|------------------------------------------------------|
| **cargo fmt**       | Formattazione codice (enforce in CI)                 |
| **cargo clippy**    | Linting con `-D warnings` in CI                      |
| **cargo test**      | Test unitari e di integrazione                       |
| **cargo-tarpaulin** | Coverage report formato Cobertura (per Codecov)      |
| **cargo-audit**     | Audit vulnerabilità RUSTSEC/NVD                      |
| **cargo-deny**      | Policy su licenze e dipendenze (`deny.toml`)         |
| **Docker**          | Containerizzazione per smoke test e playground       |
| **socat**           | Comunicazione con Unix socket nei test/CI/playground |

## CI/CD & Release

| Strumento              | Uso                                                |
|------------------------|----------------------------------------------------|
| **GitHub Actions**     | Pipeline CI/CD completa                            |
| **release-please**     | Automazione release PR e CHANGELOG                 |
| **git-cliff**          | Generazione CHANGELOG da conventional commits      |
| **GitHub Pages**       | Hosting `cargo doc` generata                       |
| **Codecov**            | Visualizzazione e tracking del coverage            |

## Configurazione `deny.toml`

Il progetto applica policy restrittive via `cargo-deny`:
- Licenze consentite: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode
- Duplicati di crate: warning (non errore)
- Vulnerabilità: errore bloccante

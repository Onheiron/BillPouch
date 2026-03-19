# 13. Use Case

## UC-1: Rete di archiviazione tra amici

**Scenario:** Carlo, Marco e Lucia vogliono condividere storage distribuito.

```bash
# Carlo — NAS domestico + laptop
bp login --alias carlo
bp hatch pouch --network amici --storage-bytes 10737418240  # 10 GB NAS
bp hatch bill  --network amici                              # laptop come client

# Marco — desktop + VPS
bp login --alias marco
bp hatch pouch --network amici --storage-bytes 53687091200  # 50 GB desktop
bp hatch pouch --network amici --storage-bytes 107374182400 # 100 GB VPS
bp hatch post  --network amici                              # VPS come relay

# Lucia — solo laptop
bp login --alias lucia
bp hatch bill --network amici
```

**Risultato in `bp flock`:**
- 6 nodi totali nella rete "amici"
- 3 fingerprint utente distinte
- 160 GB di storage totale (10 + 50 + 100)
- Marco funge da relay migliorando la connettività tra LAN diverse

---

## UC-2: Separazione rete lavoro / personale

**Scenario:** Un utente partecipa separatamente a una rete personale e a una di lavoro.

```bash
bp login --alias mio-alias

# Rete personale
bp hatch pouch --network amici --storage-bytes 5368709120

# Entra anche nella rete lavoro
bp join lavoro
bp hatch bill --network lavoro

# bp flock mostrerà servizi e peer per entrambe le reti
bp flock
```

I peer delle due reti restano **isolati** — non si vedono a vicenda a meno
che un nodo non joini esplicitamente entrambe.

---

## UC-3: VPS come nodo relay

**Scenario:** Un VPS con buona connettività ma poco disco contribuisce
come relay per migliorare la topologia della rete.

```bash
# Sul VPS
bp login --alias my-vps
bp hatch post --network my-network
```

Il nodo appare nel `flock` di tutti i peer come relay disponibile.
Contribuisce CPU e banda, nessun storage.

---

## UC-4: Exploration con il playground

**Scenario:** Sviluppatore o utente vuole esplorare BillPouch localmente
senza configurare macchine reali.

```bash
# Avvia la rete simulata (5 nodi pre-configurati)
./playground.sh up

# Entra nel nodo interattivo
./playground.sh enter

# Dentro il container:
bp flock
# → Vedi 5 peer già attivi (carlo, marco, lucia, elena, paolo)

bp hatch pouch --network playground
bp hatch bill  --network playground

bp flock
# → Ora sei presente con i tuoi 2 servizi

# Esci e controlla lo stato di tutti i nodi
# (da terminale esterno)
./playground.sh status

# Fine demo
./playground.sh down
```

---

## UC-5: CI/CD — verifica automatica discovery P2P

**Scenario:** Il workflow GitHub Actions verifica che la rete P2P
si formi correttamente ad ogni push.

```bash
# Lanciato automaticamente in CI:
docker compose -f docker-compose.smoke.yml up --build -d
./smoke/smoke-test.sh
# → PASS: tutti i 3 nodi si descobono e si vedono vicendevolmente
docker compose -f docker-compose.smoke.yml down
```

Il test fallisce se:
- Un nodo non risponde al ping entro 30s
- La mesh mDNS non si forma entro 60s
- Qualche nodo vede meno di 2 peer
- La cross-visibility tra tipi diversi non è verificata

---

## UC-6: Embedding di `bp-core` in un adapter custom

**Scenario:** Uno sviluppatore vuole esporre BillPouch via REST API
senza usare la CLI.

```toml
# Cargo.toml
[dependencies]
bp-core = { path = "path/to/bp-core" }
axum    = "0.7"
tokio   = { version = "1", features = ["full"] }
```

```rust
use bp_core::daemon::run_daemon;

// Il daemon è una libreria — nessun processo separato
tokio::spawn(async { run_daemon().await.unwrap() });

// Poi usa ControlClient per comunicare via socket
// oppure integra DaemonState direttamente
```

`bp-core` non ha dipendenze da I/O diretto al terminale:
ideale per essere wrappato in qualsiasi adapter.

---

## UC-7: Debug con log dettagliati

```bash
# Livello debug completo
RUST_LOG=bp_core=debug,bp_cli=debug bp hatch pouch --network test

# Solo log di rete
RUST_LOG=bp_core::network=debug bp --daemon

# Log silenzioso (solo errori)
RUST_LOG=error bp flock
```

---

## Target principali

| Target                  | Come usa BillPouch                                    |
|-------------------------|-------------------------------------------------------|
| **Utenti casalinghi**   | Condivisione storage tra amici/famiglia su LAN        |
| **Piccoli team**        | Storage distribuito per gruppi di lavoro              |
| **Sviluppatori Rust**   | `bp-core` come libreria per app P2P custom            |
| **Sysadmin / DevOps**   | VPS come nodi relay o storage in reti P2P gestite     |
| **Ricercatori P2P**     | Studio pratico di libp2p, gossipsub, Kademlia in Rust |

# 8. CLI Reference — `bp`

## Installazione

```bash
# Build da sorgente
git clone https://github.com/Onheiron/BillPouch.git
cd BillPouch
cargo build --release
# Binario: ./target/release/bp

# Oppure installa globalmente
cargo install --path crates/bp-cli
```

### Binari pre-compilati

Disponibili nella [pagina Release](https://github.com/Onheiron/BillPouch/releases):

| Piattaforma     | File                     |
|-----------------|--------------------------|
| Linux x86_64    | `bp-linux-x86_64`        |
| macOS x86_64    | `bp-macos-x86_64`        |
| macOS aarch64   | `bp-macos-aarch64`       |

---

## `bp login`

Crea la tua identità (keypair Ed25519) e la salva su disco.

```bash
bp login [--alias <name>]
```

| Flag      | Tipo   | Default | Descrizione                      |
|-----------|--------|---------|----------------------------------|
| `--alias` | string | nessuno | Nome leggibile opzionale         |

**Output:** mostra `peer_id` e `fingerprint` dell'identità creata.

**Esempio:**
```bash
bp login --alias carlo
# Identity created.
# Peer ID   : 12D3KooWGjEMzKMX...
# Fingerprint: a3f19c2b
```

---

## `bp logout`

Rimuove la chiave identità dal disco. **Irreversibile.**

```bash
bp logout
```

> ⚠️ Non esiste recupero. Esegui backup di `~/.local/share/billpouch/identity.key` prima.

---

## `bp hatch`

Avvia un nuovo servizio. Se il daemon non è in esecuzione, lo avvia automaticamente.

```bash
bp hatch <service_type> [--network <id>] [--storage-bytes <n>] [--mount <path>]
```

| Argomento/Flag    | Tipo   | Default    | Descrizione                           |
|-------------------|--------|------------|---------------------------------------|
| `service_type`    | string | *required* | `bill` \| `pouch` \| `post`           |
| `--network`, `-n` | string | `"public"` | ID della rete di destinazione         |
| `--storage-bytes` | u64    | —          | Solo `pouch`: byte da offrire         |
| `--mount`         | string | —          | Solo `bill`: percorso mount locale    |

**Output:** restituisce il `service_id` (UUID v4) del servizio avviato.

**Esempi:**
```bash
bp hatch pouch --network amici --storage-bytes 10737418240  # 10 GiB
bp hatch bill  --network amici --mount /mnt/myfiles
bp hatch post  --network amici
```

---

## `bp flock`

Mostra tutti i peer noti, i servizi locali e il sommario della rete.

```bash
bp flock
```

**Output esempio:**
```
╔══════════════════════════════════════════════════════╗
║             🦤  BillPouch — Flock View               ║
╚══════════════════════════════════════════════════════╝

📋 Local Services  (2)
─────────────────────────────────────────────────────
   [ pouch]  a3f19c2b  │  net: my-network  │  status: running
   [  bill]  7d82e401  │  net: my-network  │  status: running

🌐 Joined Networks  (1)
─────────────────────────────────────────────────────
   my-network  │  4 known peer(s)

🐦 Known Peers  (4)
─────────────────────────────────────────────────────
   12D3KooWGjE │ a3f19c2b │  pouch │ net: my-network
   12D3KooWBxA │ 7c1ea902 │   bill │ net: my-network
```

---

## `bp farewell`

Ferma un servizio attivo tramite il suo UUID.

```bash
bp farewell <service_id>
```

**Esempio:**
```bash
bp farewell 550e8400-e29b-41d4-a716-446655440000
```

**Errore se** il service_id non esiste nel registry locale.

---

## `bp join`

Unisciti a una rete BillPouch già esistente (sottoscrive il topic gossipsub).

```bash
bp join <network_id>
```

**Esempi:**
```bash
bp join amici
bp join lavoro
bp join public
```

**Errore se** sei già joined a quella rete.

---

## `bp --daemon`

*(uso interno)* Avvia il daemon in background. Invocato automaticamente da `bp hatch`
se il daemon non è in esecuzione. Non è pensato per l'uso diretto.

```bash
bp --daemon
```

---

## Variabili d'ambiente

| Variabile   | Uso                                                            |
|-------------|----------------------------------------------------------------|
| `RUST_LOG`  | Livello di log (es. `bp_core=debug,bp_cli=info`)              |

**Esempio:**
```bash
RUST_LOG=bp_core=debug,bp_cli=debug bp hatch pouch --network test
```

---

## Rete di default

Se `--network` non è specificato, il valore di default è **`"public"`**.
Tutti i nodi senza configurazione esplicita finiscono nella rete pubblica.

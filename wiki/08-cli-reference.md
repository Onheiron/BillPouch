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
bp hatch <service_type> [--network <id>] [--tier <tier>] [--mount <path>]
```

| Argomento/Flag    | Tipo   | Default    | Descrizione                           |
|-------------------|--------|------------|---------------------------------------|
| `service_type`    | string | *required* | `bill` \| `pouch` \| `post`           |
| `--network`, `-n` | string | `"public"` | ID della rete di destinazione         |
| `--tier`          | string | —          | Solo `pouch`: `T1` `T2` `T3` `T4` `T5`|
| `--mount`         | string | —          | Solo `bill`: percorso mount locale    |

**Storage tier Pouch:**

| Tier | Dimensione | Nome     |
|------|------------|----------|
| T1   | 10 GiB     | Pebble   |
| T2   | 100 GiB    | Stone    |
| T3   | 500 GiB    | Boulder  |
| T4   | 1 TiB      | Rock     |
| T5   | 5 TiB      | Monolith |

> **Vincolo:** un solo Pouch per network per nodo. Un secondo `hatch pouch --network X`
> viene rifiutato dal daemon.

**Esempi:**
```bash
bp hatch pouch --network amici --tier T2    # 100 GiB
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
bp farewell <service_id> [--evict]
```

| Flag      | Descrizione                                                                 |
|-----------|-----------------------------------------------------------------------------|
| `--evict` | Eviction permanente: purge dello storage su disco, annuncio gossip `evicting=true`, penalita' reputazione. **Irreversibile.** |

**Esempi:**
```bash
bp farewell 550e8400-e29b-41d4-a716-446655440000         # stop semplice
bp farewell 550e8400-e29b-41d4-a716-446655440000 --evict  # rimozione definitiva
```

---

## `bp pause`

Mette in pausa un servizio per manutenzione pianificata. Annuncia ai peer che il nodo
tornerà online entro `--eta` minuti. Se non torna in tempo, il quality monitor applica
incrementi al `fault_score`.

```bash
bp pause <service_id> --eta <minutes>
```

**Esempio:**
```bash
bp pause 550e8400-... --eta 60   # torno entro un'ora
```

---

## `bp resume`

Ripristina un servizio precedentemente messo in pausa. Riannuncia il nodo come disponibile.

```bash
bp resume <service_id>
```

---

## `bp leave`

Abbandona un network. Fallisce se ci sono servizi attivi su quella rete — fermarli prima.

```bash
bp leave <network_id>
```

Se ci sono servizi attivi, il daemon risponde con `blocked: true` e una lista di comandi
da eseguire per fermarli:

```
$ bp leave amici
🚪 Cannot leave 'amici': 2 active service(s) must be stopped first
   • 550e8400-... (pouch)  → bp farewell 550e8400-... --evict
   • 7d82e401-... (bill)   → bp farewell 7d82e401-...
```

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

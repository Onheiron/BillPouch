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

## `bp status`

Mostra identità daemon, servizi locali e conteggio peer in un formato compatto.
Per il dettaglio completo dei peer usa `bp flock`.

```bash
bp status
```

**Output esempio:**
```
╔══════════════════════════════════════════════════════╗
║           🦤  BillPouch — Daemon Status              ║
╚══════════════════════════════════════════════════════╝

🔑 Identity
   fingerprint  : a3f19c2b
   alias         : carlo
   peer_id       : 12D3KooWGjE4XmRWauN…
   daemon version : 0.3.1

🔌 Services  (2)
─────────────────────────────────────────────────────
   [ Pouch]  a3f19c2b  │  net: amici  │  running  tier: T2
   [  Bill]  7d82e401  │  net: amici  │  running

🌐 Networks  amici
📡 Known peers  4
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

Abbandona un network.

```bash
bp leave <network_id> [--force]
```

**Senza `--force`:** fallisce se ci sono servizi attivi su quella rete, elencando i comandi
necessari per fermarli:

```
$ bp leave amici
🚪 Cannot leave 'amici': 2 active service(s) must be stopped first
   • 550e8400-... (pouch)  → bp farewell 550e8400-... --evict
   • 7d82e401-... (bill)   → bp farewell 7d82e401-...

   Tip: use `bp leave amici --force` to auto-evict and leave.
```

**Con `--force`:** il daemon evict automaticamente tutti i servizi attivi sul network
(Pouch: eviction permanente + purge storage; Bill/Post: stop), poi abbandona la rete:

```
$ bp leave amici --force
🚪 Left network 'amici' — 2 service(s) auto-evicted
   Auto-evicted services:
   • 550e8400-... (pouch) — evicted
   • 7d82e401-... (bill)  — stopped
```

> **Nota:** `--force` non esegue materializzazione dei file prima dell'abbandono
> (feature v0.4). Se hai file distribuiti sulla rete che non hai mai scaricato
> localmente, assicurati di recuperarli con `bp get` prima di usare `--force`.

---

## `bp put`

Cifra un file (CEK per-utente, ChaCha20-Poly1305), lo codifica con RLNC e distribuisce
i frammenti ai Pouch della rete. `k` e `n` sono calcolati dal daemon in base alla QoS
dei peer e alla probabilità di recupero richiesta.

```bash
bp put <file> [--network <id>] [--ph <prob>] [--q-target <q>] [--name <nome>]
```

| Opzione      | Default  | Descrizione                                                   |
|--------------|----------|---------------------------------------------------------------|
| `--network`  | `public` | Rete su cui distribuire i frammenti                           |
| `--ph`       | `0.999`  | Probabilità target di poter recuperare il file in ogni momento |
| `--q-target` | `1.0`    | Overhead di ridondanza sopra la soglia di recupero            |
| `--name`     | nome file| Nome leggibile mostrato da `bp ls`                            |

Stampa il `chunk_id` (prefisso BLAKE3) da usare con `bp get`.
Funziona anche da nodi Bill/Post senza un Pouch locale: i frammenti vanno ai Pouch remoti.

---

## `bp get`

Recupera i frammenti (locali e remoti), decodifica RLNC e decifra il file.

```bash
bp get <chunk_id> <output> [--network <id>]
```

---

## `bp ls`

Elenca i file caricati da questo nodo (registro locale popolato da `bp put`).

```bash
bp ls [--network <id>]     # default: tutte le reti
```

---

## `bp storage`

Mostra quota, uso e spazio disponibile per ogni Pouch locale, più i totali e le
statistiche dei file caricati. Lo spazio disponibile effettivo tiene conto della
reputazione e dell'overhead k/N della rete.

```bash
bp storage [--network <id>]   # default: tutte le reti
```

---

## `bp invite`

Le reti sono private: si entra solo con un invito.

```bash
# Crea un token firmato (Ed25519) e cifrato con password (Argon2id + ChaCha20-Poly1305)
bp invite create --network <id> --invite-password <pwd> [--for-fingerprint <fp>] [--ttl <ore>]

# Riscatta il token: decifra, verifica, salva la NetworkMetaKey ed entra nella rete
bp invite join <blob> --invite-password <pwd>
```

Il token e la password vanno condivisi su canali separati. `--ttl` ha default 24 ore;
senza `--for-fingerprint` l'invito è aperto a chiunque abbia token e password.

---

## `bp export-identity` / `bp import-identity`

Sposta la propria identità su un'altra macchina.

```bash
bp export-identity --out identity.json
bp import-identity identity.json [--force]   # --force sovrascrive un'identità esistente
```

---

## `bp bootstrap`

Gestisce i nodi bootstrap WAN (`bootstrap.json`), usati dove mDNS non arriva.
Le modifiche hanno effetto al prossimo riavvio del daemon.

```bash
bp bootstrap list
bp bootstrap add /ip4/203.0.113.1/tcp/4001/p2p/12D3KooW...
bp bootstrap remove /ip4/203.0.113.1/tcp/4001/p2p/12D3KooW...
```

---

## `bp relay`

NAT traversal: si collega a un relay e ottiene una reservation circuit-relay v2,
rendendo il nodo raggiungibile via indirizzi `/p2p-circuit`.

```bash
bp relay connect /ip4/1.2.3.4/tcp/4001/p2p/12D3KooW...
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

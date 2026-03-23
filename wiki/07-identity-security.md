# 7. Identità e sicurezza

## Keypair Ed25519

Ogni utente possiede un **keypair Ed25519** generato al primo `bp login`.
Il keypair viene:
- Generato una volta sola (o rigenerato dopo `bp logout`)
- Salvato su disco nel file di identità
- Usato sia come identità utente che come identità libp2p

### Percorso file

```
~/.local/share/billpouch/identity.key
```

> ⚠️ **ATTENZIONE:** `bp logout` rimuove questa chiave in modo **irreversibile**.
> Non esiste recupero. Esegui un backup prima di fare logout.

---

## Fingerprint utente

La fingerprint è l'**identificatore permanente dell'utente** in tutte le reti.

### Calcolo

```
fingerprint = hex(SHA-256(public_key_bytes))[0..8]
```

Ossia i **primi 8 caratteri esadecimali** dell'hash SHA-256 della chiave pubblica.

### Proprietà

- **Immutabile** — non cambia tra reti o peer diversi dello stesso utente
- **Corta** — 8 caratteri, facile da verificare a voce
- **Basata sulla chiave pubblica** — non dipende dall'alias

### Esempio

```
Chiave pubblica (Ed25519):  [32 bytes]
SHA-256 hash:               a3f19c2b7e4d5f6a...
Fingerprint:                a3f19c2b
```

---

## Alias

L'utente può scegliere un alias leggibile opzionale:

```bash
bp login --alias "carlo"
```

- Salvato nel profilo insieme al keypair
- Incluso nei messaggi NodeInfo come `user_alias` (campo `Option<String>`)
- Non è garantita l'unicità — due utenti diversi possono avere lo stesso alias
- Modificabile solo ri-creando l'identità

---

## Identità struct

```rust
pub struct Identity {
    pub keypair:     libp2p::identity::Keypair,   // keypair Ed25519
    pub peer_id:     PeerId,                       // derivato dalla chiave pubblica
    pub fingerprint: String,                        // hex(SHA-256(pubkey))[0..8]
    pub profile:     UserProfile,                  // { alias: Option<String> }
}
```

---

## Identità libp2p

Lo **stesso keypair Ed25519** è usato anche come identità libp2p del nodo:
- `peer_id` = `PeerId::from_public_key(&keypair.public())`
- La cifratura Noise usa il medesimo keypair
- Questo garantisce che crittografia del trasporto e identità utente coincidano

---

## Sicurezza del trasporto

| Meccanismo     | Descrizione                                          |
|----------------|------------------------------------------------------|
| **Noise XX**   | Handshake crittografico bidirezionale tra peer       |
| **Yamux**      | Multiplexing sicuro degli stream                     |
| **Ed25519**    | Firma e autenticazione — chiave lunga 32 byte        |

Tutte le connessioni peer-to-peer sono **cifrate end-to-end** via Noise.

---

## Isolamento reti

Ogni rete (`network_id`) è un **gossipsub topic separato**:
```
billpouch/v1/{network_id}/nodes
```

Peer in reti diverse non si vedono a meno che un nodo non joini esplicitamente
entrambe le reti. Non esiste un meccanismo di federazione automatica.

---

## Percorsi file (XDG-aware)

Il modulo `config.rs` usa la crate `directories` per percorsi conformi alle specifiche XDG:

| Dato                    | Percorso (`~/.local/share/billpouch/`)    |
|-------------------------|-------------------------------------------|
| Identity key (plaintext)| `identity.key`                            |
| Identity key (cifrata)  | `identity.key.enc` (Argon2id + ChaCha20) |
| User profile            | `profile.json`                            |
| Control socket          | `control.sock`                            |
| PID file                | `daemon.pid`                              |
| Service registry        | `services.json`                           |
| Network membership      | `networks.json`                           |
| Kademlia peers cache    | `kad_peers.json`                          |
| Bootstrap node list     | `bootstrap.json`                          |
| Network secret keys     | `network_keys.json`                       |
| CEK plaintext hints     | `cek_hints.json`                          |
| Storage data            | `storage/<network_id>/<service_id>/`      |

---

## Sicurezza: stato attuale

| Feature                    | Stato                                                       |
|----------------------------|-------------------------------------------------------------|
| Passphrase identità         | ✅ Argon2id KDF + ChaCha20-Poly1305 (`identity.key.enc`)   |
| Multi-device identity      | ✅ `bp export-identity` / `bp import-identity`             |
| Cifratura chunk at rest    | ✅ CEK per-utente (ChaCha20-Poly1305), BLAKE3-derivata      |
| Network secret keys        | ✅ `NetworkMetaKey` random, distribuita solo via invite     |
| Invite token               | ✅ Firmato Ed25519 + cifrato Argon2id+ChaCha20              |
| CEK persistence            | ✅ `cek_hints.json` — file decifrabili dopo riavvio daemon  |
| Revoca keypair             | ❌ Nessun meccanismo di revoca implementato               |
| Backup automatico          | ❌ Backup manuale con `bp export-identity`                 |

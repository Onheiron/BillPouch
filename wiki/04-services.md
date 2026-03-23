# 4. Servizi

## I tre tipi di servizio

BillPouch definisce esattamente **tre tipi di servizio**, rappresentati dall'enum `ServiceType`:

```rust
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    Pouch,  // storage
    Bill,   // file I/O
    Post,   // relay/routing
}
```

Il parsing da stringa è **case-insensitive**: `"pouch"`, `"Pouch"`, `"POUCH"` sono tutti validi.

---

## Pouch — Storage

**Simbolo:** la sacca (gozzo) del pellicano  
**Ruolo:** Offre una porzione di disco locale alla rete.

### Metadata rilevanti

| Chiave metadata   | Tipo   | Descrizione                            |
|-------------------|--------|----------------------------------------|
| `storage_bytes`   | u64    | Byte totali offerti (es. 10737418240)  |
| `free_bytes`      | u64    | Byte liberi attualmente disponibili    |
| `mount_path`      | string | Percorso locale di mount (opzionale)   |

### Avvio

```bash
bp hatch pouch --network my-network --storage-bytes 10737418240
```

### Scenari d'uso tipici

- NAS domestico che condivide spazio alla rete "amici"
- Server VPS con storage abbondante
- Desktop che contribuisce parte del disco

---

## Bill — File I/O

**Simbolo:** il becco del pellicano  
**Ruolo:** Interfaccia personale per leggere e scrivere file distribuiti.

### Metadata rilevanti

| Chiave metadata   | Tipo   | Descrizione                              |
|-------------------|--------|------------------------------------------|
| `mount_path`      | string | Percorso locale di mount per i file      |

### Avvio

```bash
bp hatch bill --network my-network --mount /mnt/billpouch
```

### Scenari d'uso tipici

- Laptop che accede ai propri file distribuiti
- Client leggero senza storage da contribuire

---

## Post — Relay

**Simbolo:** le ali del pellicano  
**Ruolo:** Nodo di routing puro — contribuisce CPU e banda, nessun storage.

### Avvio

```bash
bp hatch post --network my-network
```

### Scenari d'uso tipici

- VPS con buona connettività ma poco disco
- Nodo "bridge" tra diverse subnet

---

## Ciclo di vita di un servizio

```
  bp hatch  →  Starting  →  Running  →  (gossip announce)
                                 │           │
                          bp farewell    bp pause --eta <m>
                                 │           │
                                 ▼           ▼
                             Stopped      Paused  →  bp resume  →  Running
                                │
                         bp farewell --evict
                                │
                                ▼
                          (storage purged + gossip evicting=true)
```

### ServiceStatus

```rust
pub enum ServiceStatus {
    Starting,
    /// Fully operational and announcing itself to the network.
    Running,
    /// Temporarily paused for maintenance (ETA in minutes).
    Paused { eta_minutes: u64, paused_at: u64 },
    Stopping,
    Stopped,
    Error(String),
}
```

> **Nota:** un solo Pouch per network per macchina. Il daemon rifiuta un secondo
> `bp hatch pouch --network X` se esiste già un Pouch attivo su quella rete.

---

## ServiceInfo

Ogni servizio locale è rappresentato internamente da un `ServiceInfo`:

```rust
pub struct ServiceInfo {
    pub id:           String,                          // UUID v4
    pub service_type: ServiceType,
    pub network_id:   String,
    pub status:       ServiceStatus,
    pub started_at:   chrono::DateTime<chrono::Utc>,
    pub metadata:     HashMap<String, serde_json::Value>,
}
```

---

## ServiceRegistry

Il daemon mantiene un `ServiceRegistry` in-memoria:

| Metodo               | Descrizione                                        |
|----------------------|----------------------------------------------------|
| `register(info)`     | Registra un nuovo servizio                         |
| `get(id)` → Option   | Recupera un servizio per UUID                      |
| `remove(id)` → bool  | Rimuove un servizio (restituisce true se trovato)  |
| `all()` → Vec        | Restituisce tutti i servizi attivi                 |

---

## Un utente, più servizi

Lo stesso utente (stessa `fingerprint`) può avere **più istanze di servizio**
su peer_id diversi (es. NAS + laptop). Ogni istanza ha UUID distinto, viene
annunciata indipendentemente via gossip ed è visibile come nodo separato
nel `flock`.

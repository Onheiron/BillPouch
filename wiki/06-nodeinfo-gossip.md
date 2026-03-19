# 6. Protocollo NodeInfo & Gossip

## NodeInfo — il messaggio gossip

Ogni nodo annuncia periodicamente la propria presenza via **gossipsub**.
Il messaggio `NodeInfo` è serializzato in JSON e broadcastato sul topic della rete.

```json
{
  "peer_id":          "12D3KooWGjE...",
  "user_fingerprint": "a3f19c2b",
  "user_alias":       "carlo",
  "service_type":     "pouch",
  "service_id":       "550e8400-e29b-41d4-a716-446655440000",
  "network_id":       "my-network",
  "listen_addrs":     ["/ip4/192.168.1.10/tcp/54321"],
  "announced_at":     1710000000,
  "metadata": {
    "storage_bytes": 10737418240,
    "free_bytes":    8000000000,
    "version":       "0.1.3"
  }
}
```

### Campi NodeInfo

| Campo              | Tipo                                     | Descrizione                                    |
|--------------------|------------------------------------------|------------------------------------------------|
| `peer_id`          | string                                   | libp2p PeerId del nodo                        |
| `user_fingerprint` | string                                   | Primi 8 char hex di SHA-256(public_key)       |
| `user_alias`       | `Option<String>`                         | Alias leggibile (opzionale)                   |
| `service_type`     | `"pouch"` \| `"bill"` \| `"post"`        | Tipo del servizio                             |
| `service_id`       | string (UUID v4)                         | ID univoco dell'istanza servizio              |
| `network_id`       | string                                   | Rete di appartenenza                          |
| `listen_addrs`     | `Vec<String>` (multiaddr)                | Indirizzi di ascolto libp2p                   |
| `announced_at`     | u64 (Unix timestamp secondi)             | Timestamp dell'ultimo annuncio                |
| `metadata`         | `HashMap<String, serde_json::Value>`     | Dati estensibili liberi                       |

---

## Topic Gossipsub

Il topic ha formato standard:

```
billpouch/v1/{network_id}/nodes
```

Esempi:
```
billpouch/v1/amici/nodes
billpouch/v1/lavoro/nodes
billpouch/v1/public/nodes
billpouch/v1/smoketest/nodes
```

Il metodo `NodeInfo::topic_name(network_id)` genera il nome del topic.

Ogni `JoinNetwork` nel daemon eseguirà:
```rust
swarm.behaviour_mut().gossipsub.subscribe(&IdentTopic::new(topic))
```

---

## NetworkState — store in-memory

Il daemon mantiene uno store `NetworkState` che indicizza i `NodeInfo` ricevuti
in una `HashMap<PeerId, NodeInfo>`.

### Operazioni principali

| Metodo                   | Descrizione                                           |
|--------------------------|-------------------------------------------------------|
| `new()`                  | Crea uno store vuoto                                  |
| `upsert(node)`           | Inserisce o aggiorna (chiave: `peer_id`)              |
| `remove(peer_id)`        | Rimuove esplicitamente un nodo                        |
| `evict_stale(secs)`      | Rimuove nodi con `announced_at` più vecchio di `secs` |
| `in_network(network_id)` | Filtra nodi per `network_id`                          |
| `all()`                  | Restituisce tutti i nodi (`Vec<&NodeInfo>`)            |
| `len()`                  | Numero totale di nodi noti                            |

### Eviction degli stale peer

Nodi che non si ri-annunciano per più di **120 secondi** (2 minuti)
vengono rimossi automaticamente:

```rust
state.evict_stale(120);
```

L'eviction è basata sul campo `announced_at` (timestamp Unix).

---

## Flusso gossip completo

```
[nodo locale]                            [nodo remoto]
     │                                        │
     │  bp hatch pouch --network amici        │
     │         ↓                              │
     │  DaemonState: ServiceRegistry upsert   │
     │         ↓                              │
     │  NetworkCommand::Announce { payload }  │
     │         ↓                              │
     │  gossipsub.publish(topic, NodeInfo)    │
     │                │                       │
     │       gossipsub network mesh           │
     │                └──────────────────────►│
     │                                        │  handle_swarm_event
     │                                        │  → deserialize NodeInfo
     │                                        │  → NetworkState::upsert
     │                                        │
     │◄──────────────────────────────────────┤
     │  (ri-annuncio periodico del remoto)    │
     │  NetworkState::upsert / evict_stale    │
```

---

## Metadata estensibili

Il campo `metadata` è aperto e privo di schema. Convenzioni attuali:

| Chiave          | Usato da | Tipo   | Descrizione                   |
|-----------------|----------|--------|-------------------------------|
| `storage_bytes` | `pouch`  | u64    | Byte totali offerti           |
| `free_bytes`    | `pouch`  | u64    | Byte liberi disponibili       |
| `mount_path`    | `bill`   | string | Percorso mount locale         |
| `version`       | tutti    | string | Versione software del nodo    |

Nuovi campi possono essere aggiunti senza rompere la compatibilità.

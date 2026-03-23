# 14. Storage Bidding — Protocollo Pouch

## Concetto

Quando un nodo `Pouch` si connette a una rete, **fa un'offerta** (_bid_): si impegna a
mettere a disposizione una quota di storage locale in cambio di partecipazione alla rete.
Questa quota non è puramente dichiarativa — viene verificata periodicamente dalla rete
(vedere [wiki/16-network-quality.md](./16-network-quality.md)).

---

## Ciclo di vita di un Pouch bid

```
bp hatch pouch --network <id> --tier <T1|T2|T3|T4|T5>
        │
        ▼
1. Daemon verifica quale tier è stato scelto e che il filesystem abbia spazio sufficiente
2. Crea la directory di storage locale con la quota del tier
3. Annuncia il bid via gossip (NodeInfo con metadata tier + storage_bytes_bid)
4. La rete inizia a distribuire fragment sul nuovo Pouch (solo tier ≤ quello del Pouch)
5. Quando la quota è saturata, il Pouch smette di accettare nuovi fragment
6. Il Pouch risponde ai challenge periodici del network quality monitor
```

### Tier disponibili

| Tier | Dimensione | Nome     | Partecipa a          |
|------|------------|----------|----------------------|
| T1   | 10 GiB     | Pebble   | T1                   |
| T2   | 100 GiB    | Stone    | T1, T2               |
| T3   | 500 GiB    | Boulder  | T1, T2, T3           |
| T4   | 1 TiB      | Rock     | T1, T2, T3, T4       |
| T5   | 5 TiB      | Monolith | T1, T2, T3, T4, T5   |

Un Pouch partecipa al calcolo N/k/q di **tutti i tier ≤ il proprio**: un Pouch T3
può ospitare fragment di file T1, T2 e T3.

> **Vincolo:** un solo Pouch per network per nodo. Il daemon rifiuta un secondo
> `hatch pouch --network X` se ne esiste già uno attivo per quella combinazione
> identità/network.

---

## Directory locale di storage

Al momento del bid, il daemon crea la seguente struttura:

```
~/.local/share/billpouch/
  storage/
    <network_id>/
      <pouch_service_id>/
        meta.json          ← quota biddata, spazio usato, network_id
        fragments/
          <chunk_id>/
            <fragment_id>.frag   ← dati del fragment (raw bytes)
            <fragment_id>.vec    ← coding vector GF(2^8) associato
```

Il file `meta.json` contiene:

```json
{
  "network_id":        "amici",
  "service_id":        "<uuid>",
  "tier":              "T2",
  "storage_bytes_bid": 107374182400,
  "storage_bytes_used": 0,
  "joined_at":         1710000000
}
```

---

## Metadata gossippati nel NodeInfo

Il campo `metadata` del [`NodeInfo`](./06-nodeinfo-gossip.md) include:

```json
{
  "tier":                    "T2",
  "storage_bytes_bid":       107374182400,
  "storage_bytes_used":      2147483648,
  "storage_bytes_available": 105226698752
}
```

La rete legge `tier` e `storage_bytes_available` per decidere dove distribuire nuovi fragment
(preferendo Pouch con tier idoneo e spazio disponibile).

---

## Regole di bid

| Regola | Descrizione |
|---|---|
| **Tier fisso** | Il Pouch dichiara esattamente uno dei 5 tier; non sono accettate quote arbitrarie |
| **Quota minima** | T1 (10 GiB) — protezione da nodi triviali |
| **Cambio tier** | Non supportato in v0.3; per cambiare tier: `bp farewell --evict` + nuovo `bp hatch` |
| **Quota non riservata** | Il daemon verifica che il filesystem abbia spazio, non riserva blocchi fisici |
| **Un Pouch per network** | Il daemon rifiuta un secondo `bp hatch pouch --network X` se uno è già attivo |

---

## Stato locale dei fragment

Ogni Pouch mantiene un indice in memoria (e su disco) di quali fragment possiede:

```
FragmentIndex:
  chunk_id  → Vec<(fragment_id, coding_vector, size_bytes)>
```

Questo indice viene ricostruito al riavvio del daemon scansionando
`storage/<network_id>/<service_id>/fragments/`.

---

## Relazione con gli altri componenti

```
bp hatch pouch
    │
    ├── DaemonState.services (ServiceRegistry)
    │       └── ServiceInfo { service_type: Pouch, metadata: {tier, storage_bytes_bid, ...} }
    │
    ├── StorageManager (da implementare)
    │       └── crea directory, inizializza meta.json, gestisce FragmentIndex
    │
    └── NetworkLoop (gossipsub)
            └── annuncia NodeInfo con storage metadata aggiornati
```

---

## Comandi CLI coinvolti

```bash
# Fai un bid T2 (100 GiB) sulla rete "amici"
bp hatch pouch --network amici --tier T2

# Fai un bid minimo T1 (10 GiB)
bp hatch pouch --network amici --tier T1

# Verifica stato del bid (nella sezione Local Services di flock)
bp flock

# Termina il bid (eviction permanente: purge storage + gossip)
bp farewell <service_id> --evict
```

---

## Decisioni di design aperte

| Decisione | Note |
|---|---|
| Rifiutare fragment oltre quota | Il Pouch deve rifiutare push quando `used >= bid` |
| Comportamento alla farewell `--evict` | I fragment locali vengono purgati; la rete li rigenera dai Pouch sopravvissuti |
| Persistenza tra riavvii | `meta.json` + directory → il Pouch riprende da dove ha lasciato |
| Cambio tier | Richiede `farewell --evict` + nuovo `hatch pouch --tier <new>`; nessun resize in-place |

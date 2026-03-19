# 14. Storage Bidding — Protocollo Pouch

## Concetto

Quando un nodo `Pouch` si connette a una rete, **fa un'offerta** (_bid_): si impegna a
mettere a disposizione una quota di storage locale in cambio di partecipazione alla rete.
Questa quota non è puramente dichiarativa — viene verificata periodicamente dalla rete
(vedere [wiki/16-network-quality.md](./16-network-quality.md)).

---

## Ciclo di vita di un Pouch bid

```
bp hatch pouch --network <id> --storage-bytes <N>
        │
        ▼
1. Daemon verifica che la quota sia disponibile su disco
2. Crea la directory di storage locale
3. Annuncia il bid via gossip (NodeInfo con metadata storage_bytes)
4. La rete inizia a distribuire fragment sul nuovo Pouch
5. Quando la quota è saturata, il Pouch smette di accettare nuovi fragment
6. Il Pouch risponde ai challenge periodici del network quality monitor
```

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
  "network_id":       "amici",
  "service_id":       "<uuid>",
  "storage_bytes_bid": 10737418240,
  "storage_bytes_used": 0,
  "joined_at":        1710000000
}
```

---

## Metadata gossippati nel NodeInfo

Il campo `metadata` del [`NodeInfo`](./06-nodeinfo-gossip.md) include:

```json
{
  "storage_bytes_bid":       10737418240,
  "storage_bytes_used":      2147483648,
  "storage_bytes_available": 8589934592
}
```

La rete legge `storage_bytes_available` per decidere dove distribuire nuovi fragment.

---

## Regole di bid

| Regola | Descrizione |
|---|---|
| **Quota minima** | 1 GiB per partecipare come Pouch (protezione da nodi triviali) |
| **Quota massima** | Nessun limite tecnico; la rete può bilanciare su più Pouch |
| **Cambio quota** | Non supportato in v0.2; per ridurre quota: farewell + nuovo hatch |
| **Quota non verificata** | Il daemon verifica che il filesystem abbia spazio, non riserva blocchi fisici |

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
    │       └── ServiceInfo { service_type: Pouch, metadata: {storage_bytes_bid, ...} }
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
# Fai un bid di 10 GiB sulla rete "amici"
bp hatch pouch --network amici --storage-bytes 10737418240

# Verifica stato del bid (nella sezione Local Services di flock)
bp flock

# Termina il bid (i fragment vengono redistribuiti dalla rete)
bp farewell <service_id>
```

---

## Decisioni di design aperte

| Decisione | Note |
|---|---|
| Rifiutare fragment oltre quota | Il Pouch deve rifiutare push quando `used >= bid` |
| Comportamento alla farewell | I fragment locali vanno propagati prima di uscire? O la rete li rigenerà? → **decisione: la rete rigenera** |
| Persistenza tra riavvii | `meta.json` + directory → il Pouch riprende da dove ha lasciato |
| Quota riduzione | Prenotare per v0.3 con un protocollo di drain esplicito |

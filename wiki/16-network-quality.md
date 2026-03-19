# 16. Network Quality Monitor — Challenge, Scoring e Blacklist

## Obiettivo

La rete deve verificare periodicamente che ogni Pouch:

1. Sia ancora **raggiungibile** in tempi accettabili.
2. **Contenga effettivamente i fragment** che ha dichiarato di possedere.

Un Pouch che fallisce questi controlli riceve un penalty sul proprio **fault score**.
Quando il fault score supera la soglia, il nodo viene **blacklistato** e la sua
voce viene propagata a tutta la rete via gossip.

---

## Attori

| Attore | Ruolo |
|---|---|
| **Challenger** | Qualsiasi nodo attivo (Bill o Pouch) che emette challenge. In pratica: ogni nodo fa challenge ai Pouch che conosce. |
| **Challenged** | Il Pouch che riceve il challenge e deve rispondere. |
| **Witnesses** | Gli altri nodi che ricevono il risultato del challenge via gossip e aggiornano il proprio modello di reputazione. |

---

## Tipi di challenge

### 1. Ping / RTT check

Verifica la raggiungibilità di base e misura la latenza.

```
Challenger → Challenged:  ChallengeRequest { type: Ping, nonce: u64 }
Challenged → Challenger:  ChallengeResponse { nonce: u64, ts_recv: u64, ts_send: u64 }
```

RTT = (ts_challenger_recv - ts_challenger_send)
Latency score = f(RTT) — valore 0.0..1.0 (1.0 = ottimo)

### 2. Proof of Storage (PoS)

Verifica che il Pouch abbia ancora un fragment specifico sul disco.

```
Challenger → Challenged:  ChallengeRequest {
  type: ProofOfStorage,
  chunk_id:    "<hex>",
  fragment_id: "<uuid>",
  nonce:       u64,
}

Challenged → Challenger:  ChallengeResponse {
  fragment_id: "<uuid>",
  proof:       BLAKE3(fragment_data || nonce),   ← 32 byte
}
```

Il Challenger può verificare il proof solo se conosce il fragment originale.
**In alternativa:** il Challenged invia i primi 64 byte del fragment in chiaro —
il Challenger (che ha il coding vector) può verificare localmente senza conoscere
il dato completo.

> **Nota di design:** il Challenger deve conoscere almeno il hash del fragment
> per verificare il proof. Questo implica che la rete mantenga un indice gossippato
> di `{chunk_id, fragment_id, fragment_hash}` per ciascun fragment distribuito.

### 3. Bandwidth test (futuro)

Trasferimento di un fragment intero per misurare throughput effettivo.
Prenotato per v0.4.

---

## Scheduling dei challenge

Ogni nodo attivo esegue il monitor con questo timer:

```
ogni 60 secondi:
  per ciascun Pouch noto nella rete:
    se last_challenge_at > 120s fa:
      emetti ChallengeRequest (Ping)
      
ogni 5 minuti:
  per ciascun Pouch noto:
    campiona 1 fragment random tra quelli che il Pouch dichiara di avere:
      emetti ChallengeRequest (ProofOfStorage)
```

Il sampling è probabilistico: su reti grandi (>50 Pouch) ogni nodo campiona
solo un sottoinsieme per evitare overhead di rete.

---

## Fault Score

Ogni nodo mantiene localmente un `fault_score` per ogni Pouch noto:

```
fault_score ∈ [0, 100]    (0 = perfetto, 100 = blacklist threshold)

Evento                          Delta fault_score
─────────────────────────────────────────────────
Ping OK                         -2  (miglioramento graduale)
Ping timeout (>5s)              +5
Ping timeout (>30s)             +15
PoS proof corretto              -3
PoS proof errato                +20
PoS no response (>10s)          +10
RTT > 2000ms                    +1
```

Il fault_score viene **decaduto nel tempo** (mean reversion):
ogni ora senza eventi negativi → `-1`.

### Soglie

| Soglia | Azione |
|---|---|
| `fault_score ≥ 70` | Il nodo viene marcato `degraded` — la rete smette di inviare nuovi fragment |
| `fault_score ≥ 90` | Il nodo viene marcato `suspected` — i suoi fragment vengono rigenerati altrove |
| `fault_score = 100` | **BLACKLIST** — propagato via gossip a tutta la rete |

---

## Propagazione della blacklist via gossip

Quando un nodo raggiunge `fault_score = 100`, il challenger pubblica un
`BlacklistAnnouncement` sul topic gossip:

```json
{
  "type":         "blacklist",
  "peer_id":      "12D3KooW...",
  "fingerprint":  "a3f19c2b",
  "reason":       "proof_of_storage_failure",
  "evidence":     { "failed_challenges": 5, "last_valid_at": 1710000000 },
  "announced_by": "12D3KooWXXXX",
  "announced_at": 1710001000
}
```

I nodi riceventi:
1. Aggiornano il loro modello locale di reputazione.
2. Smettono di inviare fragment al nodo blacklistato.
3. Marcano i fragment detenuti da quel nodo come **a rischio** → avviano recoding.

---

## Gestione dei fragment a rischio

Quando un Pouch viene marcato `suspected` o `blacklisted`, la rete avvia
la **rigenerazione preventiva** dei suoi fragment:

```
per ciascun chunk_id dove il Pouch sospetto ha fragment:
  trova altri Pouch che hanno fragment dello stesso chunk_id
  richiedi recoding → nuovi fragment verso Pouch sani con spazio disponibile
  aggiorna il gossip FragmentIndex
```

Questa operazione usa il **recoding** descritto in [wiki/15-erasure-coding.md](./15-erasure-coding.md):
i Pouch sani generano nuovi fragment senza ricomporre il chunk originale.

---

## Gossip topic dedicato

Il monitor usa un topic separato da `NodeInfo`:

```
billpouch/v1/{network_id}/quality
```

Messaggi propagati su questo topic:

| Tipo | Frequenza | Payload |
|---|---|---|
| `ChallengeResult` | dopo ogni challenge | peer_id, type, outcome, latency_ms |
| `BlacklistAnnouncement` | su blacklist | vedi sopra |
| `BlacklistRevocation` | se nodo torna sano | peer_id, revoked_by, reason |

---

## Protezioni anti-spam / Sybil

| Protezione | Meccanismo |
|---|---|
| Challenge firmati | Ogni ChallengeRequest è firmato con la chiave Ed25519 del challenger |
| Rate limiting | Max 10 challenge/minuto per peer per evitare flooding |
| Reputazione del challenger | Un challenger con fault_score alto viene ignorato |
| Consenso blacklist | La blacklist ha effetto solo se annunciata da ≥ 2 nodi indipendenti |
| Revoca | Il nodo blacklistato può rispondere con proof corretta → revoca dopo K challenge superati |

---

## Struttura dati in-memory (DaemonState)

```rust
pub struct QualityState {
    /// fault_score per peer_id
    pub scores: HashMap<String, u8>,
    /// peer_id → blacklisted_at timestamp
    pub blacklist: HashMap<String, u64>,
    /// peer_id → last_challenge_at timestamp  
    pub last_challenged: HashMap<String, u64>,
}
```

`QualityState` vive dentro `DaemonState` con il solito `RwLock<QualityState>`.

---

## Relazione con il ciclo di vita del Pouch

```
Pouch entra nella rete
    → fault_score = 0
    → riceve challenge Ping ogni ~60s
    → riceve challenge PoS ogni ~5min
    
fault_score ≥ 70:  degraded  → niente nuovi fragment
fault_score ≥ 90:  suspected → rigenerazione fragment avviata
fault_score = 100: blacklist → gossip announcement, nodo escluso
    
Pouch esce nolontariamente (crash, rete persa):
    → timeout sui Ping → fault_score cresce
    → se torna entro finestra di grazia (30min): riprende da fault_score corrente
    → se non torna: blacklist
    
Pouch fa bp farewell (uscita volontaria):
    → annuncio esplicito Farewell via gossip
    → fault_score non aumenta → non viene blacklistato
    → i suoi fragment vengono redistribuiti normalmente
```

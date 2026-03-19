# 15. Chunking ed Erasure Coding — Fountain Codes con Recoding

## Obiettivo

Ogni file caricato nella rete viene trasformato in **fragment ridondanti** tali che:

1. Qualsiasi sottoinsieme di **k fragment** su **N totali** è sufficiente a ricostruire il file.
2. Un Pouch può **generare nuovi fragment validi a partire dai fragment che già possiede**,
   senza mai ricostruire il chunk originale.

La proprietà (2) è detta **recoding** ed è il requisito centrale del sistema.
La tecnica usata è **Random Linear Network Coding (RLNC)** su GF(2⁸).

---

## Pipeline di upload (Bill → rete)

```
File originale
    │
    ▼ 1. Chunking
┌──────────────────────────────────────┐
│  Chunk_1 │ Chunk_2 │ ... │ Chunk_M  │  (dimensione fissa, es. 1 MiB)
└──────────────────────────────────────┘
    │
    ▼ 2. Split in source symbols
┌──────────────────────────────────────┐
│  s_1 │ s_2 │ ... │ s_k              │  k source symbols per chunk
└──────────────────────────────────────┘
    │
    ▼ 3. Encoding RLNC → N encoded fragments
┌──────────────────────────────────────────────────────────┐
│  f_1 = Σ c_1j · s_j  │  f_2 = Σ c_2j · s_j  │  ...  │  (N fragments)
│  vec_1 = [c_11..c_1k] │  vec_2 = [c_21..c_2k] │       │
└──────────────────────────────────────────────────────────┘
    │
    ▼ 4. Distribuzione ai Pouch disponibili
   (ogni Pouch riceve una fetta dei fragment)
```

---

## RLNC — математика in breve

Ogni **chunk** viene suddiviso in **k source symbols** `s_1 ... s_k`
(vettori di byte di lunghezza fissa, es. 64 byte ciascuno).

Un **fragment codificato** è una combinazione lineare su **GF(2⁸)**:

```
f_i = c_i1·s_1 ⊕ c_i2·s_2 ⊕ ... ⊕ c_ik·s_k
```

dove `c_ij ∈ GF(2⁸)` sono coefficienti random.

Il fragment viene memorizzato insieme al suo **coding vector**:

```
(f_i, vec_i)   dove   vec_i = [c_i1, c_i2, ..., c_ik]
```

### Decodifica

Un nodo che possiede **≥ k fragment linearmente indipendenti** può risolvere il sistema:

```
┌ vec_1 ┐   ┌ s_1 ┐   ┌ f_1 ┐
│ vec_2 │ × │ s_2 │ = │ f_2 │
│  ...  │   │ ... │   │ ... │
└ vec_k ┘   └ s_k ┘   └ f_k ┘
```

con eliminazione Gaussiana su GF(2⁸).

---

## Recoding — generare nuovi fragment senza decodificare

Un Pouch che possiede `m` fragment `(f_1, vec_1) ... (f_m, vec_m)` può produrre
un nuovo fragment `(f_new, vec_new)` scegliendo coefficienti random `a_1 ... a_m ∈ GF(2⁸)`:

```
f_new   = a_1·f_1   ⊕ a_2·f_2   ⊕ ... ⊕ a_m·f_m
vec_new = a_1·vec_1 ⊕ a_2·vec_2 ⊕ ... ⊕ a_m·vec_m
```

Il risultato è un fragment **matematicamente valido** (è ancora una combinazione lineare
degli source symbols originali) senza che il Pouch abbia mai visto i dati originali.

**Proprietà chiave:**
- Un Pouch con `m < k` fragment può comunque riencodare (produce fragment linearmente
  dipendenti da quelli che ha, quindi con informazione parziale).
- Un Pouch con `m ≥ k` fragment può produrre fragment completamente nuovi con piena
  entropia informativa.
- La rete usa il recoding per riempire nuovi Pouch che si uniscono **senza round-trip
  verso il Bill originale**.

---

## Struttura su disco di un fragment

```
fragments/
  <chunk_id>/                    ← SHA256(chunk_originale)[0..16] hex
    <fragment_id>.frag            ← raw bytes: f_i (stesso numero di byte del source symbol * k)
    <fragment_id>.vec             ← k byte: il coding vector c_i1...c_ik
```

`fragment_id` = UUID v4 generato al momento dell'encoding / recoding.

`chunk_id` è derivato dal contenuto del chunk originale: permette a qualsiasi Bill
di verificare l'integrità senza conoscere la chiave.

---

## Parametri N e K

`N` e `K` **non sono costanti globali**: vengono calcolati dalla rete per ogni chunk
in base allo stato corrente dei Pouch.

### Calcolo di K (soglia di recovery)

```
K = ceil(chunk_size / symbol_size)
```

tipicamente fisso per una rete (es. K = 16 per chunk da 1 MiB con symbol da 64 KiB).

### Calcolo di N (ridondanza totale)

```
N = K × redundancy_factor

redundancy_factor = f(num_pouches, avg_stability_score, target_durability)
```

Esempio con `target_durability = 99.9%` e stabilità media 80%:

| Pouch connessi | Stabilità media | N suggerito | N/K |
|---|---|---|---|
| 3 | 90% | K×2 | 2.0 |
| 5 | 80% | K×3 | 3.0 |
| 10 | 70% | K×4 | 4.0 |
| 20+ | qualsiasi | K×5 | 5.0 |

Il valore viene propagato nel gossip come parametro di rete (`network_params`),
permettendo a tutti i nodi di concordare su N e K senza coordinazione centralizzata.

### Aggiornamento dinamico

Quando un Pouch entra o esce dalla rete, il Bill coordinatore (o il pouch più anziano)
ricalcola N e annuncia i nuovi parametri. I Pouch esistenti rigenerano fragment aggiuntivi
se N aumenta; i fragment in eccesso vengono ignorati se N diminuisce.

---

## Verifica dell'integrità

Al momento della ricostruzione, il Bill verifica:

1. I fragment decodificano correttamente (il sistema è risolvibile).
2. SHA256 del chunk ricostruito == `chunk_id`.
3. Concatenazione dei chunk ricostruiti == hash del file originale (manifest).

---

## Dipendenze Rust pianificate

| Crate | Ruolo |
|---|---|
| `galois_2p8` o implementazione interna | Aritmetica su GF(2⁸) |
| `blake3` | Hash dei chunk e fragment_id derivati dal contenuto |
| `rayon` | Encoding / decoding parallelo su chunk multipli |
| `bytes` | Zero-copy buffer per i fragment in transito |

---

## Crate RaptorQ vs RLNC custom

RaptorQ (RFC 6330, crate `raptorq`) **non supporta nativamente il recoding** da fragment
a fragment senza decodifica. Per questo motivo, BillPouch implementa RLNC su GF(2⁸)
direttamente, che è più semplice di RaptorQ ma ha la proprietà di recoding richiesta.

Il trade-off: RLNC ha overhead di decodifica O(k²) vs O(k) di RaptorQ. Con k ≤ 32
(scelta tipica) questo è irrilevante in pratica.

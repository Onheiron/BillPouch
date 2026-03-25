# 5. Protocollo di controllo (CLI ↔ Daemon)

## Trasporto

- **Unix domain socket** — `~/.local/share/billpouch/control.sock`
- **Formato:** JSON newline-delimited (ogni messaggio è una singola riga JSON)
- **Encoding:** UTF-8
- **Modello:** request / response sincrono

---

## ControlRequest

```rust
pub enum ControlRequest {
    Ping,
    Status,
    Hatch    { service_type: ServiceType, network_id: String, metadata: HashMap<String, Value> },
    Flock,
    Farewell      { service_id: String },
    FarewellEvict { service_id: String },
    Pause         { service_id: String, eta_minutes: u64 },
    Resume        { service_id: String },
    Join          { network_id: String },  // internal/hidden
    Leave         { network_id: String, force: bool },  // force=true auto-evicts services
    PutFile  { chunk_data: Vec<u8>, ph: Option<f64>, q_target: Option<f64>, network_id: String },
    GetFile  { chunk_id: String, network_id: String },
    ConnectRelay  { relay_addr: String },
    CreateInvite  { network_id: String, invitee_fingerprint: Option<String>, invite_password: String, ttl_hours: Option<u64> },
}
```

### Esempi JSON

```jsonc
// Ping
{"cmd":"ping"}

// Status completo
{"cmd":"status"}

// Avvia pouch tier T2 (100 GiB)
{"cmd":"hatch","service_type":"pouch","network_id":"amici","metadata":{"tier":"T2","storage_bytes":107374182400}}

// Avvia bill con mount
{"cmd":"hatch","service_type":"bill","network_id":"amici","metadata":{"mount_path":"/mnt/files"}}

// Lista peer
{"cmd":"flock"}

// Ferma servizio
{"cmd":"farewell","service_id":"550e8400-e29b-41d4-a716-446655440000"}

// Evict permanente Pouch (purge storage)
{"cmd":"farewell_evict","service_id":"550e8400-e29b-41d4-a716-446655440000"}

// Pausa manutenzione (30 minuti)
{"cmd":"pause","service_id":"550e8400-e29b-41d4-a716-446655440000","eta_minutes":30}

// Ripristino dopo pausa
{"cmd":"resume","service_id":"550e8400-e29b-41d4-a716-446655440000"}

// Abbandona network (richiede nessun servizio attivo)
{"cmd":"leave","network_id":"lavoro"}

// Abbandona network con auto-evict di tutti i servizi attivi
{"cmd":"leave","network_id":"lavoro","force":true}
```

---

## ControlResponse

```rust
pub struct ControlResponse {
    pub status:  String,           // "ok" | "error"
    pub data:    Option<Value>,    // payload opzionale
    pub message: Option<String>,   // messaggio errore
}
```

### Risposta ok con dati

```json
{"status":"ok","data":{ ... }}
```

### Risposta ok senza dati

```json
{"status":"ok"}
```

### Risposta errore

```json
{"status":"error","message":"service not found: 550e8400-..."}
```

---

## Strutture dati nelle risposte

### StatusData (`bp status`)

```json
{
  "peer_id":        "12D3KooWGjE...",
  "fingerprint":    "a3f19c2b",
  "alias":          "carlo",
  "local_services": [ ...ServiceInfo... ],
  "networks":       ["amici", "lavoro"],
  "known_peers":    4,
  "version":        "0.1.3"
}
```

### HatchData (`bp hatch`)

```json
{
  "service_id":   "550e8400-e29b-41d4-a716-446655440000",
  "service_type": "pouch",
  "network_id":   "amici",
  "message":      "service hatched"
}
```

### FlockData (`bp flock`)

```json
{
  "local_services": [ ...ServiceInfo... ],
  "known_peers":    [ ...NodeInfo... ],
  "networks":       ["amici"],
  "peer_count":     4
}
```

### FarewellData (`bp farewell`)

```json
{
  "service_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

---

## Dispatch nel server

Il server (`control/server.rs`) riceve la request, la dispatcha, restituisce la response:

```rust
async fn dispatch(req: ControlRequest, state: &Arc<DaemonState>) -> ControlResponse {
    match req {
        ControlRequest::Ping             => ControlResponse::ok("pong"),
        ControlRequest::Status           => { /* legge DaemonState */ },
        ControlRequest::Hatch{..}        => { /* crea ServiceInfo, StorageManager, Announce */ },
        ControlRequest::Flock            => { /* legge services + network_state */ },
        ControlRequest::Farewell{..}     => { /* rimuove servizio dal registry */ },
        ControlRequest::FarewellEvict{..}=> { /* purge storage, gossip evicting=true, reputation evict */ },
        ControlRequest::Pause{..}        => { /* ServiceStatus::Paused, Announce */ },
        ControlRequest::Resume{..}       => { /* ServiceStatus::Running, Announce */ },
        ControlRequest::Leave{..}        => { /* precondition check, LeaveNetwork */ },
        ControlRequest::Join{..}         => { /* JoinNetwork (internal) */ },
        ControlRequest::PutFile{..}      => { /* RLNC encode, distribute fragments */ },
        ControlRequest::GetFile{..}      => { /* fetch fragments, RLNC decode, CEK decrypt */ },
    }
}
```

---

## Test via socat

```bash
# Ping
echo '{"cmd":"ping"}' | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock
# → {"status":"ok","data":"pong"}

# Status
echo '{"cmd":"status"}' | socat -T2 - UNIX-CONNECT:$HOME/.local/share/billpouch/control.sock
```

---

## Casi di errore gestiti

| Situazione                                | Messaggio d'errore                                              |
|-------------------------------------------|-----------------------------------------------------------------|
| `farewell` con ID inesistente             | `"No service with id '<id>'"` |
| `farewell_evict` su non-Pouch             | Ritorna ok (nessun StorageManager, bytes_freed=0) |
| `pause` su servizio non running           | `"Service '<id>' is not running"` |
| `resume` su servizio non paused           | `"Service '<id>' is not paused"` |
| `leave` con servizi attivi                | ok con `blocked:true` e lista `blocking_services` |
| `hatch pouch` secondo Pouch stesso network| `"a Pouch for network '<id>' already exists"` |
| `join` su rete già joined                 | `"Already a member of network '<id>'"` |
| Identità non trovata                      | `"identity not found"` |
| Parsing JSON malformato                   | Risposta di errore dal server |

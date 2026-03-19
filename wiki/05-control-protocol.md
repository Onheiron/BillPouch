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
    Farewell { service_id: String },
    Join     { network_id: String },
}
```

### Esempi JSON

```jsonc
// Ping
{"cmd":"ping"}

// Status completo
{"cmd":"status"}

// Avvia servizio pouch con 10 GB
{"cmd":"hatch","service_type":"pouch","network_id":"amici","metadata":{"storage_bytes":10737418240}}

// Avvia servizio bill con mount
{"cmd":"hatch","service_type":"bill","network_id":"amici","metadata":{"mount_path":"/mnt/files"}}

// Avvia relay puro
{"cmd":"hatch","service_type":"post","network_id":"amici","metadata":{}}

// Lista peer
{"cmd":"flock"}

// Ferma servizio
{"cmd":"farewell","service_id":"550e8400-e29b-41d4-a716-446655440000"}

// Unisciti a rete
{"cmd":"join","network_id":"lavoro"}
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
        ControlRequest::Ping      => ControlResponse::ok("pong"),
        ControlRequest::Status    => { /* legge DaemonState */ },
        ControlRequest::Hatch{..} => { /* crea ServiceInfo, invia Announce */ },
        ControlRequest::Flock     => { /* legge services + network_state */ },
        ControlRequest::Farewell  => { /* rimuove servizio dal registry */ },
        ControlRequest::Join{..}  => { /* invia JoinNetwork via net_tx */ },
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

| Situazione                          | Messaggio d'errore                        |
|-------------------------------------|-------------------------------------------|
| `farewell` con ID inesistente       | `"service not found: <service_id>"`       |
| `join` su rete già joined           | `"already joined network: <network_id>`" |
| Identità non trovata al login       | `"identity not found"`                    |
| Parsing JSON malformato             | Risposta di errore dal server             |

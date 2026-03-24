# 17 — bp-api: REST API Reference

`bp-api` is an optional HTTP gateway that exposes all daemon capabilities via a REST JSON API and serves an embedded web dashboard.  It proxies every request to the running daemon over the Unix control socket.

---

## Starting bp-api

```bash
bp-api                        # default: 127.0.0.1:3000
bp-api --port 8080 --host 0.0.0.0
```

The daemon **must already be running** (`bp hatch <type>`) before requests can be served.

---

## Base URL

```
http://127.0.0.1:3000
```

All endpoints return JSON.  Error responses have the shape:

```json
{ "error": "<message>" }
```

---

## Endpoints

### `GET /`

Serves the embedded SPA web dashboard (HTML).

---

### `GET /ping`

Liveness check.

**Response 200:**
```json
"pong"
```

---

### `GET /status`

Daemon identity and all local services.

**Response 200:**
```json
{
  "peer_id": "12D3KooW...",
  "fingerprint": "a3f19c2b",
  "alias": "carlo",
  "local_services": [
    {
      "id": "3f2a...",
      "service_type": "Pouch",
      "network_id": "friends",
      "status": "running",
      "metadata": { "tier": "T2" }
    }
  ],
  "networks": ["friends"]
}
```

---

### `GET /peers`

All known gossip peers and network summary (same as `bp flock`).

**Response 200:**
```json
{
  "local_services": [...],
  "networks": ["friends"],
  "known_peers": [
    {
      "peer_id": "12D3KooW...",
      "user_fingerprint": "b7e20f1a",
      "user_alias": "alice",
      "service_type": "Bill",
      "network_id": "friends"
    }
  ],
  "peer_count": 1
}
```

---

### `POST /services`

Hatch (start) a new service.

**Body:**
```json
{
  "service_type": "Pouch",
  "network_id": "friends",
  "metadata": { "tier": "T2" }
}
```

**Response 201:**
```json
{ "service_id": "<uuid>" }
```

---

### `DELETE /services/:service_id`

Stop a service gracefully.

| Query param | Type | Default | Description |
|-------------|------|---------|-------------|
| `evict`     | bool | `false` | If `true`, also purges all stored fragments and announces eviction to the network |

**Examples:**
```
DELETE /services/3f2a...          # graceful stop
DELETE /services/3f2a...?evict=true  # stop + evict fragments
```

**Response 200:**
```json
{ "status": "stopped", "service_id": "3f2a..." }
```

---

### `POST /services/:service_id/pause`

Pause a running service for planned maintenance.  Announces to the network that this Pouch is temporarily unavailable.

**Body:**
```json
{ "eta_minutes": 30 }
```

**Response 200:**
```json
{ "status": "paused", "eta_minutes": 30 }
```

---

### `POST /services/:service_id/resume`

Resume a previously paused service.  Triggers an immediate Proof-of-Storage challenge so reputation is updated without penalty.

**Body:** _(empty)_

**Response 200:**
```json
{ "status": "running" }
```

---

### `POST /networks/join`

Subscribe to a gossip network.

**Body:**
```json
{ "network_id": "friends" }
```

**Response 200:**
```json
{ "status": "joined", "network_id": "friends" }
```

---

### `POST /networks/leave`

Leave a gossip network.

Without `"force": true`: returns `blocked: true` if active services are still attached to the network.  
With `"force": true`: auto-evicts all blocking services (Pouch → permanent eviction; Bill/Post → graceful stop), then leaves.

**Body:**
```json
{ "network_id": "friends" }
```
or with force:
```json
{ "network_id": "friends", "force": true }
```

**Response 200 (success):**
```json
{ "message": "Left network 'friends'", "services_auto_evicted": [] }
```

**Response 200 (blocked, force not set):**
```json
{
  "network_id": "friends",
  "blocked": true,
  "blocking_services": [
    { "id": "3f2a...", "type": "pouch", "hint": "bp farewell 3f2a... --evict" }
  ],
  "message": "Cannot leave 'friends': 1 active service(s) must be stopped first"
}
```

---

### `POST /files`

Encode a file chunk with RLNC and distribute fragments across the network.

**Body:**
```json
{
  "chunk_data": "<base64-encoded bytes>",
  "network_id": "friends",
  "ph": 0.999,
  "q_target": 1.0
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `chunk_data` | string | required | Base64-encoded raw bytes |
| `network_id` | string | `"public"` | Target network |
| `ph` | float | `0.999` | Target recovery probability |
| `q_target` | float | `1.0` | Redundancy overhead |

**Response 201:**
```json
{
  "chunk_id": "a3f19c2b...",
  "fragments_sent": 8,
  "k": 5,
  "n": 8
}
```

---

### `GET /files/:chunk_id`

Retrieve and decode a stored file chunk.

| Query param | Type | Default | Description |
|-------------|------|---------|-------------|
| `network`   | string | `"public"` | Network to search fragments in |

**Example:**
```
GET /files/a3f19c2b...?network=friends
```

**Response 200:**
```json
{
  "chunk_id": "a3f19c2b...",
  "data": "<base64-encoded bytes>"
}
```

**Response 404:** chunk not found.

---

### `POST /relay/connect`

Dial a relay node for NAT traversal (useful when both peers are behind NAT).

**Body:**
```json
{ "relay_addr": "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW..." }
```

**Response 200:**
```json
{ "status": "connected" }
```

---

### `POST /invites`

Create a signed, password-encrypted invite token for a network.  Share the token out-of-band with the intended recipient.

**Body:**
```json
{
  "network_id": "friends",
  "invite_password": "secret",
  "invitee_fingerprint": "a3f19c2b",
  "ttl_hours": 24
}
```

(`invitee_fingerprint` and `ttl_hours` are optional.)

**Response 200:**
```json
{ "blob": "<hex invite token>", "network_id": "friends", "inviter_fingerprint": "a3f19c2b", "expires_at": 1711040000 }
```

---

### `POST /invites/redeem`

Join a network by redeeming an invite token.  Equivalent to `bp invite join <token>`.

**Body:**
```json
{
  "blob": "<hex invite token>",
  "invite_password": "secret"
}
```

**Response 200:**
```json
{ "status": "joined", "network_id": "friends", "inviter_fingerprint": "a3f19c2b" }
```

---

## Error codes

| HTTP status | Meaning |
|-------------|---------|
| 200 | Success |
| 201 | Resource created |
| 404 | Chunk / resource not found |
| 422 | Unprocessable entity (e.g. invalid base64) |
| 500 | Daemon returned an error |
| 503 | Daemon not running (control socket unavailable) |

---

## Web dashboard

`GET /` serves a single-page dashboard (compiled from `bp-api/static/index.html`) that displays live status, peer counts, and service summaries.  It polls `/status` and `/peers` periodically.

---

## See also

- [wiki/08-cli-reference.md](08-cli-reference.md) — full CLI reference
- [wiki/05-control-protocol.md](05-control-protocol.md) — raw IPC protocol
- [wiki/12-status-roadmap.md](12-status-roadmap.md) — feature status

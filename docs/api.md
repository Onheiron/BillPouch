# Interfaces

## Control protocol (CLI ↔ daemon)

Unix socket `~/.local/share/billpouch/control.sock`. A client connects, writes
one JSON object followed by `\n`, reads one JSON line back, and disconnects.
Types: `bp_core::control::protocol`; client helper: `bp_core::control::client::send`.

```json
→ {"cmd":"hatch","service_type":"pouch","network_id":"friends","metadata":{"tier":"T1","storage_bytes":10737418240}}
← {"status":"ok","data":{"service_id":"…","service_type":"pouch","network_id":"friends","message":"…"}}
← {"status":"error","message":"a Pouch for network 'friends' already exists on this node; …"}
```

| `cmd` | Fields | Does |
|-------|--------|------|
| `ping` | — | Returns `"pong"` |
| `status` | — | Identity, services, networks, reputation, Pouch stats, QoS summary |
| `announce_now` | — | Re-gossips every service, waits 2 s |
| `flock` | — | Local services, networks, known peers |
| `hatch` | `service_type`, `network_id`, `metadata` | Starts a service (joins the network if needed) |
| `farewell` | `service_id` | Stops a service |
| `farewell_evict` | `service_id` | Evicts a service (see [cli.md](cli.md)) |
| `pause` | `service_id`, `eta_minutes` | Maintenance mode |
| `resume` | `service_id` | Leaves maintenance mode |
| `create_network` | `name` | Creates a network with you as creator |
| `invite` | `network_id`, `member_key` (OpenSSH line) | Adds a member once committed and returns the join token; with your own key, returns a token for another device of yours (`for_device: true`) |
| `join_network` | `token` | Joins with a token and waits for the ledger to sync |
| `members` | `network_id` | Members with tier, weight, devices, pending expulsions; expelled members |
| `expel` | `network_id`, `member` (fingerprint) | Proposes or approves an expulsion |
| `leave` | `network_id`, `force` | Leaves a network |
| `put_file` | `chunk_data` (bytes), `network_id`, `ph`?, `q_target`?, `file_name`? | Stores a file, returns `chunk_id`, `k`, `n`, placement counts |
| `get_file` | `chunk_id`, `network_id` | Returns the decrypted bytes of a file in your index |
| `share` | `network_id`, `chunk_id`, `member` (fingerprint) | Wraps the file key for a member and delivers it |
| `list_files` | `network_id` (`""` = all) | Your index: your files and those shared with you (`owner`, `mine`, `shared_with`) |
| `storage_info` | `network_id` (`""` = all) | Per-Pouch and aggregate storage |
| `connect_relay` | `relay_addr` | Dials a relay and reserves a circuit |

## REST API (`bp-api`)

`bp-api [--host 127.0.0.1] [--port 3000]` forwards HTTP calls to a running
daemon's socket. `GET /` serves a dashboard that polls `/status` and `/peers`.
Errors are `{"error": "…"}` with 503 (daemon unreachable), 422 (invalid base64) or 500 (daemon error).

| Method | Path | Body / query | Control command |
|--------|------|--------------|-----------------|
| GET | `/ping` | | `ping` |
| GET | `/status` | | `status` |
| GET | `/peers` | | `flock` |
| POST | `/services` | `{service_type, network_id, metadata?}` → 201 | `hatch` |
| DELETE | `/services/:id` | `?evict=true` to evict | `farewell` / `farewell_evict` |
| POST | `/services/:id/pause` | `{eta_minutes}` | `pause` |
| POST | `/services/:id/resume` | | `resume` |
| POST | `/networks` | `{name}` → 201 | `create_network` |
| POST | `/networks/:id/invites` | `{member_key}` → 201 | `invite` |
| POST | `/networks/join` | `{token}` | `join_network` |
| GET | `/networks/:id/members` | — | `members` |
| POST | `/networks/:id/expulsions` | `{member}` (fingerprint) | `expel` |
| POST | `/networks/leave` | `{network_id, force?}` | `leave` |
| POST | `/files` | `{chunk_data: base64, network_id, ph?, q_target?, file_name?}` → 201 | `put_file` |
| GET | `/files/:chunk_id` | `?network=<id>` → `{chunk_id, data: base64, …}` | `get_file` |
| POST | `/files/:chunk_id/shares` | `{network_id, member}` | `share` |
| POST | `/relay/connect` | `{relay_addr}` | `connect_relay` |

There is no authentication: keep it on `127.0.0.1`.

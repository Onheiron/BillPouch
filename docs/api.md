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
| `flock` | — | Local services, networks, known peers and the challenge record of each peer |
| `hatch` | `service_type`, `network_id`, `metadata` (a Pouch needs `tier`, the daemon sets its quota) | Starts a service; one of each type per network on a node |
| `farewell` | `service_id` | Stops a service |
| `farewell_evict` | `service_id` | Evicts a service (see [cli.md](cli.md)) |
| `pause` | `service_id`, `eta_minutes` | Maintenance mode |
| `resume` | `service_id` | Leaves maintenance mode |
| `create_network` | `name` | Creates a network with you as creator |
| `invite` | `network_id`, `member_key` (OpenSSH line) | Adds a member once committed and returns the join token; with your own key, returns a token for another device of yours (`for_device: true`) |
| `join_network` | `token` | Joins with a token and waits for the ledger to sync |
| `members` | `network_id` | Members with tier, weight, devices, trust and open bets, pending expulsions; expelled members; network parameters |
| `expel` | `network_id`, `member` (fingerprint) | Proposes or approves an expulsion |
| `leave` | `network_id`, `force` | Leaves a network |
| `put_file` | `path` (on the daemon's machine), `network_id`, `ph`?, `file_name`? | Streams a file in, returns `file_id`, `chunks`, `k`, `n`, placement counts |
| `get_file` | `file_id`, `network_id`, `output` (path) | Writes the decrypted file to `output` |
| `delete_file` | `file_id`, `network_id` | Deletes one of your files, or drops a shared one from your index |
| `share` | `network_id`, `file_id`, `member` (fingerprint) | Wraps the file key for a member and delivers it |
| `unshare` | `network_id`, `file_id`, `member` | Revokes a member's access |
| `list_files` | `network_id` (`""` = all) | Your index: your files and those shared with you (`owner`, `mine`, `shared_with`) |
| `storage_info` | `network_id` (`""` = all) | Per Pouch: quota, others' fragments, filler; per network: what you offer, your credit and usage, the network's k/n and your quota (offered, verified and used × k/n); your upload totals |
| `connect_relay` | `relay_addr` | Dials a relay and reserves a circuit |

## REST API (`bp-api`)

`bp-api [--host 127.0.0.1] [--port 3000]` forwards HTTPS calls to the daemon
of this device and serves the BillPouch web app. On first start it creates a
self-signed certificate (`api_cert.pem`, `api_key.pem`) and logs its SHA-256
fingerprint at every start. The app's files (`/`, `/app.js`, `/app.css`,
`/sw.js`, `/manifest.webmanifest`, `/icon.svg`) are public; every other route
needs `Authorization: Session <session>`: a session is signed with this
device's user SSH key and lasts 12 h; `bp-api` accepts only sessions of that
key. `bp dashboard` starts `bp-api` when it is not running, signs a session and
opens `https://127.0.0.1:<port>/#session=<session>`; the app keeps it in the
browser until it expires. `GET /files/:file_id` also takes the session as
`?session=<session>`, because a `<video>` or `<audio>` element cannot send a
header: that is how the web app's player streams a file. Such a URL grants
access to that file for as long as the session lasts, so it is never shared.
Audio and video files are served with their media type, and an open-ended
range (`bytes=A-`) returns at most 8 MiB, so playback starts without waiting
for the whole file.

The web app is a progressive web app with a sidebar (a bottom bar on phones):
**Fish**, the folders of each network with their users, Pouches, free space and
k/n, and your files, which you upload into the selected folder by picking or
dropping them, play, download or delete; **Flock**, the network: members,
nodes, space offered and availability, live charts of the nodes online and of
the average availability, what each member hosts and stores, members and trust,
every node with its challenge results, and an invitation form; **Nest**, your
quota, files, devices, reputation in each network and the services you can
start on this node; **Roost**, this node: its services with maintenance, stop
and evict, how far its ledgers are synchronized, its addresses and leaving a
network; **Gossip**, the community, still empty; and, at the bottom, your avatar
with your profile and settings. Browsers only register
its service worker over a trusted certificate, so with the self-signed one it
works as a page you can install but not offline.

Errors are `{"error": "…"}` with 401 (missing, expired or foreign session), 503 (daemon unreachable), 400 (interrupted upload) or 500 (daemon error).

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
| GET | `/files` | `?network=<id>` (all if omitted) | `list_files` |
| POST | `/files` | raw file as the body, `?network=<id>&name=<file name>&ph=<p>?&tier=<T1…T5>?&chunk=<bytes>?` → 201 | `put_file` (streamed to a temporary file) |
| GET | `/storage` | `?network=<id>` (all if omitted) | `storage_info` |
| GET | `/files/:file_id` | `?network=<id>` → the raw file, as an attachment named after it; a `Range: bytes=A-B` header returns 206 with only those bytes (`Accept-Ranges: bytes`) | `get_file` |
| DELETE | `/files/:file_id` | `?network=<id>` | `delete_file` |
| POST | `/files/:file_id/shares` | `{network_id, member}` | `share` |
| DELETE | `/files/:file_id/shares/:member` | `?network=<id>` | `unshare` |
| POST | `/relay/connect` | `{relay_addr}` | `connect_relay` |

It is meant as a local tool: keep it on `127.0.0.1`.

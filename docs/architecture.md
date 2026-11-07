# Architecture

## Crates

| Crate | Kind | Role |
|-------|------|------|
| `bp-core` | library | Everything: identity, daemon, networking, coding, storage, control protocol |
| `bp-cli`  | binary `bp` | Thin client: parses commands, talks to the daemon over the control socket |
| `bp-api`  | binary `bp-api` | HTTP gateway to the same socket, plus an embedded web dashboard |

`bp-core` never prints or exits; it returns `BpResult<T>` (`thiserror`). The
binaries use `anyhow` and own all terminal output.

## Daemon

`bp hatch` starts the daemon (`bp --daemon`) if it is not running. One daemon
per user per machine; it runs these Tokio tasks (`daemon.rs`):

| Task | What it does |
|------|--------------|
| Control server | Unix socket `control.sock`, one JSON request/response per connection ([api.md](api.md)) |
| Network loop | Drives the libp2p swarm, executes `NetworkCommand`s, serves fragment requests |
| Quality monitor | Ping every 60 s, Proof-of-Storage every 300 s → QoS + reputation |
| Re-announce | Gossips every local service at 10, 20, 30 s after start, then every 30 s |
| Stale eviction | Every 60 s drops peers not heard from for 120 s |

All shared state lives in `DaemonState` (`control/server.rs`).

## Files

Everything is under the XDG data dir (`~/.local/share/billpouch/` on Linux):

| File | Contents |
|------|----------|
| `user_ed25519` | Your user key (OpenSSH format, optionally passphrase-encrypted) |
| `device.key`, `device_cert.json` | This device's libp2p key and the certificate your user key signed for it |
| `profile.json` | Alias and creation time |
| `control.sock`, `daemon.pid` | Daemon socket and PID |
| `network_keys.json` | Secret key of each joined network |
| `cek_hints.json` | `chunk_id → BLAKE3(plaintext)`, needed to decrypt your files |
| `file_registry.json` | Files you uploaded (`bp ls`) |
| `kad_peers.json` | Known peer addresses, saved every 600 s and redialled at start |
| `bootstrap.json` | Optional WAN bootstrap peers (`bp bootstrap`) |
| `storage/<network>/<service_id>/` | A Pouch's `meta.json` and `fragments/<chunk_id>/<fragment_id>.frag` |

## Networking

libp2p 0.54 over TCP with Noise and Yamux:

| Behaviour | Use |
|-----------|-----|
| mDNS | Zero-config discovery on the LAN |
| Kademlia | Routing table, seeded by `kad_peers.json` and `bootstrap.json` |
| Identify | `/billpouch/id/1.0.0` |
| Gossipsub | Topics `billpouch/v1/{network}/nodes` (NodeInfo) and `billpouch/v1/{network}/index` (fragment locations) |
| Request-response (CBOR) | `/billpouch/fragment/1.1.0`: store, fetch, ping, Proof-of-Storage |
| AutoNAT + relay client | NAT traversal through a relay (`bp relay connect`) |

A daemon can join several networks at once; each network is a pair of gossip topics.

**NodeInfo** is what every service announces:

```json
{
  "peer_id": "12D3KooW...",
  "user_fingerprint": "SHA256:92XmV1i/JZb/qL1HbVeo6HPYzWG3JrtsgHYWN7Tj3ps",
  "user_alias": "carlo",
  "service_type": "pouch",
  "service_id": "550e8400-...",
  "network_id": "friends",
  "listen_addrs": [],
  "announced_at": 1710000000,
  "metadata": { "tier": "T1", "storage_bytes": 10737418240 }
}
```

`device_cert` (omitted above) proves which user the device belongs to. `metadata` is the service's own metadata. `bp pause` adds `maintenance` and
`eta_minutes`; an eviction adds `evicting: true`.

## Storing a file

`bp put` sends the whole file to the daemon, which treats it as one chunk:

1. **Encrypt.** ChaCha20-Poly1305 with a per-user content key:
   `CEK = BLAKE3_keyed(identity secret, "billpouch/cek/v1" ‖ BLAKE3(plaintext))`.
   `BLAKE3(plaintext)` is kept in `cek_hints.json`; Pouches only ever see ciphertext.
2. **Choose k and n.** From the stability score `p_i` of every known Pouch,
   `k = ⌊μ − z(Ph)·σ + 0.5⌋` with `μ = Σp_i` and `σ² = Σp_i(1−p_i)`: the largest
   `k` such that P(at least k Pouches are reachable) ≥ `Ph` (`--ph`, default 0.999).
   Then `n = round(k·(1 + q_target))` (`--q-target`, default 1.0). Peers never
   pinged count as 0.8.
3. **Encode.** Systematic RLNC over GF(2⁸): `n` fragments, any `k` of which decode.
   `chunk_id` is a 16-hex-char BLAKE3 prefix.
4. **Place.** Fragments go to the local Pouch (if any) and round-robin to the
   remote Pouches of the network. The placement is gossiped on the `index` topic
   and recorded for Proof-of-Storage.

`bp get` collects local fragments, fetches more from the Pouches listed in the
gossiped index (falling back to every known Pouch) until it has `k`, decodes
and decrypts.

RLNC was chosen because a Pouch can **recode**: combine the fragments it holds
into new valid fragments without decoding. This is how fragments held by a
failing peer are replaced.

## Storage tiers

A Pouch offers exactly one tier; at most one Pouch per network per node
(two Pouches on one machine would fail together).

| Tier | Name | Quota |
|------|------|-------|
| T1 | Pebble | 10 GiB |
| T2 | Stone | 100 GiB |
| T3 | Boulder | 500 GiB |
| T4 | Rock | 1 TiB |
| T5 | Monolith | 5 TiB |

The *available* space reported by `bp status` / `bp storage` is the free quota
times the k/N rate the network can currently sustain for this node's
reputation target (`Ph` = 0.70 for R1 … 0.999 for R4; 0 for R0). With few or
unknown peers it can be 0.

## Quality, Proof-of-Storage, reputation

For every peer the daemon keeps a QoS record (`network/qos.rs`):

- **Stability score** `s = 0.6·exp(−rtt/500 ms) + 0.4·success_rate`
  (RTT EWMA α = 0.125, success EWMA β = 0.10). This is the `p_i` used for k/n.
- **Fault score** 0–100: +5 per failed Proof-of-Storage, −1 per passed one.
  At 90 (*suspected*) the fragments it holds are recoded and pushed to a peer
  below 70 (*degraded*); at 100 its reputation becomes R0.

**Proof-of-Storage**: every 300 s the daemon picks one fragment it placed on
each peer and asks for `BLAKE3(fragment ‖ nonce)`. A reply within 10 s passes.

**Reputation** (`network/reputation.rs`), updated after every challenge:

| Tier | Name | Rule |
|------|------|------|
| R0 | Quarantine | fault score 100, or evicted (locked 30 days) |
| R1 | Fledgling | default |
| R2 | Reliable | known ≥ 30 days, PoS pass rate ≥ 98 % |
| R3 | Trusted | known ≥ 90 days, pass rate ≥ 99.5 % |
| R4 | Pillar | known ≥ 365 days, pass rate ≥ 99.9 % |

QoS and reputation live in memory only.

## Identity and access

- **User**: an OpenSSH ed25519 key, imported (`bp login --ssh-key`) or generated.
  Its fingerprint is the standard SSH one (`SHA256:…`, same as `ssh-keygen -lf`).
  Passphrase-protected keys are unlocked with `--passphrase` / `BP_PASSPHRASE`.
- **Device**: every machine has its own libp2p key, so its own `peer_id`. The user
  key signs a certificate `(user key, device key, issued_at)`; logging in with the
  same SSH key on another machine creates another device of the same user.
- **Announcements** carry the certificate. A NodeInfo is accepted only if the
  certificate verifies, its device is the gossip publisher and its user matches
  `user_fingerprint`.
- **Network key**: each network has a random 32-byte secret in `network_keys.json`,
  generated by whoever first joins it.
- **Invites**: `bp invite create` signs a payload (network id, network key,
  inviter fingerprint, optional invitee fingerprint, expiry) with the inviter's
  key and encrypts it with a password (Argon2id + ChaCha20-Poly1305):
  `hex(salt ‖ nonce ‖ ciphertext)`. `bp invite join` decrypts, verifies the
  signature and expiry, saves the key and joins.
- **Transport**: every connection is Noise-encrypted.

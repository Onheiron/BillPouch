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
| Quality monitor | Ping every 60 s, Proof-of-Storage every 300 s → QoS + per-device observations for the ledger |
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
| `networks/<name>.json` | Genesis, your key entry and the committed ledger blocks of each network you belong to |
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
| Gossipsub | One topic per network, `BLAKE3_keyed(network key, "billpouch/topic/v1")`; every message is encrypted with the network key |
| Request-response (CBOR) | `/billpouch/fragment/1.1.0`: store, fetch, ping, Proof-of-Storage |
| AutoNAT + relay client | NAT traversal through a relay (`bp relay connect`) |

A daemon can be in several networks at once. Gossip messages are `Node` (NodeInfo), `Index` (fragment locations) and `Consensus` (ledger ops, proposals, votes, commits). A node accepts NodeInfo, and serves fragment requests, only from devices of ledger members.

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
   remote Pouches of the network. The placement is gossiped as an `Index` message
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
  below 70 (*degraded*).

**Proof-of-Storage**: every 300 s the daemon picks one fragment it placed on
each peer and asks for `BLAKE3(fragment ‖ nonce)`. A reply within 10 s passes.

Every challenge result is counted per device and epoch. In the last 10 % of an
epoch each member submits an `Observations` op with its counts (at most 288
challenges per device per epoch) for the devices of other users.

QoS lives in memory only; reputation lives in the ledger.

## Shared ledger and consensus

Each network has a ledger (`ledger.rs`) built by replaying committed blocks of
signed ops from the genesis entry and the creator's device certificate:

| Op | Effect |
|----|--------|
| `AddMember` | Adds a member entry (with the network key wrapped for them) |
| `RegisterDevice` | Adds one of your devices; the first one becomes your voting device |
| `SetVotingDevice` | Picks which of your devices votes |
| `Observations` | Adds PoS counts to other users' totals |
| `ProposeExpel` / `ApproveExpel` | Expulsion vote |

**Epochs** last one day of block time. At the first block of an epoch the
ledger takes a snapshot of every user's tier and weight; weights stay fixed for
the whole epoch. A member who joined during the epoch has no weight until the
next one.

**Reputation** is per user, over all their devices:

| Tier | Name | Rule |
|------|------|------|
| R1 | Fledgling | default, or fewer than 100 challenges |
| R2 | Reliable | member ≥ 30 days, PoS pass rate ≥ 98 % |
| R3 | Trusted | member ≥ 90 days, pass rate ≥ 99.5 % |
| R4 | Pillar | member ≥ 365 days, pass rate ≥ 99.9 % |

Weight = tier rank (R1 = 1 … R4 = 4). With 4 or more users no user can hold
more than a quarter of the total (capped iteratively).

**Capabilities**: vote R1+, invite R2+, propose an expulsion R3+; while nobody
has reached R2 (R3) the creator alone can invite (propose expulsions). An
expulsion passes with > 2/3 of the other members' weight and at least half of
them by count.

**Consensus** (`consensus.rs`) is Tendermint-style over the snapshot weights:
the round's proposer (validators in order, rotating with height and round)
proposes a block of pending ops, members prevote and precommit, and a block with
precommits from > 2/3 of the weight is final. A node that precommitted a block
is locked on it and only unlocks on a newer > 2/3 prevote, so a committed block
is never replaced; rounds time out (3 s + 1 s per round) and are skipped when
> 1/3 of the weight is already in a later one. Each user votes through one
voting device, so their devices cannot equivocate by accident. Nothing is
proposed while no op is pending.

**Sync**: commits carry their precommits, so any node can verify them alone.
A node that sees a commit more than one block ahead asks the sender for the
missing ones (at most 100 per request); the sender serves them only to a device
whose certificate belongs to a member (`chain.rs`).

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
- **Network**: rooted in a genesis entry signed by the creator; the genesis hash
  identifies the network. Membership is whatever the ledger says.
- **Network key**: 32 random bytes made at creation. Every entry carries it
  encrypted for the new member: X25519 (from the member's ed25519 key) with an
  ephemeral key, BLAKE3-derived wrapping key, ChaCha20-Poly1305. It never travels
  in clear and only the member it was wrapped for can open it.
- **Invites**: `bp invite` submits an `AddMember` op, waits for it to be
  committed and prints a token (genesis, creator's device certificate, your
  entry). `bp join` opens the key from the entry, subscribes, syncs the ledger
  from members and waits until it lists you; the daemon then registers the
  device by itself.
- **Transport**: every connection is Noise-encrypted.

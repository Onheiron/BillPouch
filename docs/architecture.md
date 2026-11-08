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
| `qos.json` | Per-peer QoS (RTT, challenge success rate), saved every 60 s |
| `files.json` | Local copy of your file index per network, the index heads you have seen, shares not yet acknowledged |
| `kad_peers.json` | Known peer addresses, saved every 600 s and redialled at start |
| `bootstrap.json` | Optional WAN bootstrap peers (`bp bootstrap`) |
| `storage/<network>/<service_id>/` | A Pouch's `meta.json`, `fragments/<chunk_id>/` (`<fragment_id>.frag` and the chunk's `owner`), `fillers/`, `revoked.json` |

## Networking

libp2p 0.57 over TCP with Noise and Yamux:

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

`bp put` passes the file's path to the daemon, which streams it in 4 MiB
chunks; only one chunk is in memory at a time. Each file gets a random
`file_id` and a random key.

1. **Encrypt.** Each chunk is sealed with ChaCha20-Poly1305 under the file key.
   The key is wrapped (X25519 + ChaCha20-Poly1305, as for the network key) for
   your SSH key and put in the file's manifest: `file_id`, the ordered
   `chunk_id`s, name, size, owner, one wrap and one grant per authorized user.
   Pouches only ever see ciphertext.
2. **Choose the layer** (see *Storage tiers*). `chunk_id` is a 16-hex-char
   BLAKE3 prefix of the ciphertext. The chunk goes to the lowest layer whose
   every Pouch (yours included) still has room for one fragment in that layer's
   slice; lower layers have more Pouches, so they are at least as stable.
3. **Choose k** (`coding/params.rs`). Every Pouch of the layer gets exactly one
   fragment, so `n` = Pouches in the layer. With `X_i` = Pouch `i` reachable
   (probability `p_i`, its stability score; 0.8 if never pinged), the recovery
   probability `Pe = P(Σ X_i ≥ k)` is computed exactly by dynamic programming,
   and `k` is the largest (≤ 128) with `Pe ≥ Ph` (`--ph`, default 0.999). Each
   Pouch then stores 1/k of the chunk. If even `k = 1` (a full copy on every
   Pouch) misses `Ph`, it is used anyway and `Pe` is reported.
4. **Encode and place.** Systematic RLNC over GF(2⁸): `n` fragments, any `k` of
   which decode. Your own fragment is stored locally; the others are pushed to
   their Pouches with your **owner grant** and the layer. The placement is gossiped as an
   `Index` message and recorded for Proof-of-Storage.

`bp get` looks the file up in your index and, chunk by chunk, collects local
fragments, fetches more from the Pouches listed in the gossiped fragment index
(falling back to every known Pouch) until it has `k`, decodes, checks the
`chunk_id`, decrypts and appends to the output (written as `.bp-partial` and
renamed at the end).

**Grants.** A grant `(file_id, owner key, grantee fingerprint)` is signed with
the owner's SSH key. A Pouch accepts a fragment only from a device of a member
whose own grant (grantee = owner) comes with it, and records owner key and
`file_id` with the chunk; the first owner of a chunk keeps it. It serves a
chunk's fragments only to a device of the grantee of a grant for that file
signed by that owner, unless the grantee was revoked: you (all your devices)
and the members you shared the file with, whose grant travels in the manifest.
Only the owner can share a file.

**Owner orders.** `Delete` and `Revoke{member}` are signed by the owner and
gossiped (and re-gossiped 10 times, every 30 s). A Pouch deletes the chunks, or
records the revocation, only for chunks whose recorded owner key matches. A
revoked member also drops the file from their index; a copy they already
downloaded stays with them.

## Your file index, devices and sharing

- **Index**: every manifest you own or were shared, per network. It is sealed
  with `BLAKE3-derive("billpouch/index-key/v1", user secret)`, so every device
  holding your SSH key can open it and nobody else can, and stored in the network
  like a file. A **head** `(owner key, network, version, file_id, chunk_ids)` signed with
  your SSH key points at the latest version; heads are gossiped, cached by every
  node and re-gossiped by your devices every 30 s (or requested with a
  `HeadRequest` when a device has none).
- **Devices**: `bp invite <network> <your own public key>` prints a token for
  another device of yours, without touching the ledger. On that device,
  `bp login --ssh-key <same key>` and `bp join <token>`: it syncs the ledger,
  registers itself, receives your head and pulls your index. Indexes merge by
  union, so two devices adding files at the same time both keep them; deleted
  files and revoked members are kept as tombstones so a merge never brings them
  back. Each new index version is stored under a new `file_id` and the previous
  one is deleted with an owner order.
- **Sharing**: `bp share` wraps the file key for the member's SSH key (taken
  from the ledger) and gossips the manifest to them; they add it to their index
  and acknowledge. Until the acknowledgement arrives the share is re-sent every
  30 s.

RLNC was chosen because a Pouch can **recode**: combine the fragments it holds
into new valid fragments without decoding. This is how fragments held by a
failing peer are replaced.

## Storage tiers

A Pouch offers exactly one tier; at most one Pouch per network per node
(two Pouches on one machine would fail together). Tiers are nested layers: a
T3 Pouch is also in layers T2 and T1, and gives each layer a slice of its
space: 10 GiB to T1, the next 90 GiB to T2, the next 400 GiB to T3, and so on.
With 100 Pouches of which 30 are T2 and 10 are T3, layer T1 has 100 Pouches,
T2 40 and T3 10. A fragment of layer `L` only goes into the `L` slice, and a
Pouch announces its free space per layer (`layer_free`). Usable space and
redundancy therefore come in steps, one per layer; files fill T1 first.

| Tier | Name | Quota |
|------|------|-------|
| T1 | Pebble | 10 GiB |
| T2 | Stone | 100 GiB |
| T3 | Boulder | 500 GiB |
| T4 | Rock | 1 TiB |
| T5 | Monolith | 5 TiB |

The *available* space reported by `bp status` / `bp storage` is the free quota
times the k/N rate the network can currently sustain, one fragment per Pouch
(largest `k` with `P(ΣX_i ≥ k) ≥ Ph`), for this node's reputation target (`Ph` = 0.70 for R1 … 0.999 for R4; 0 for R0). With few or
unknown peers it can be 0.

## Quality, Proof-of-Storage, reputation

For every peer the daemon keeps a QoS record (`network/qos.rs`):

- **Stability score** `s = 0.6·exp(−rtt/500 ms) + 0.4·success_rate`
  (RTT EWMA α = 0.125, success EWMA β = 0.10). This is the `p_i` used for k/n.
**Repair is the network's job.** Every Pouch of a layer holds one fragment of
every chunk of that layer, so a Pouch that dies costs every chunk of its
layers one fragment and there is nowhere else to put it: redundancy comes back
when a Pouch joins the layer, new or replacing the lost one. Every 60 s each
Pouch, for each of its layers, asks a random Pouch of the layer for its
**catalog** (chunk, owner, `file_id`, `k`) and, for each chunk it lacks (at
most 64 per round), starts a **recoding chain**: `k` random Pouches of the layer
in turn add `c · own fragment` (random `c` in GF(2⁸)) to the running sum and
pass it on, and the last one hands the result to the new Pouch, which stores it
with the chunk's owner. The result is a fresh RLNC fragment; nobody sees more
than its own fragment and one partial sum, so no fetch grant is needed, only
that the requester and the destination are member devices.

Fragment index announcements list every holder, the owner's Pouch included,
and `bp get` falls back to every known Pouch.

**Commitments** (`storage/commitment.rs`): everything a Pouch holds comes with
a commitment `(what, holder, Merkle root, blocks, size)` signed by the device
that produced it: the owner for the fragments of `bp put`, the last hop of a
recoding chain for a recoded fragment, the sponsor for a filler blob. The Pouch
checks it against the data and stores it next to it.

**Proof-of-Storage is done by the network.** Data is split into 4 KiB blocks
committed with a BLAKE3 Merkle tree. Every 60 s every node with a Pouch picks
up to 8 random Pouches of other users in its network and asks each for one
random block, with its Merkle path and commitment, of either a chunk of a
shared layer (taken from the challenger's own catalog: every Pouch of the
layer should hold one fragment of it) or a random one of the filler blobs the
Pouch announces. The reply passes within 10 s if the commitment names that
Pouch, is signed by a member device (a filler's sponsor must be another user),
covers what was asked and the path leads to its root at the index fixed by
the challenger's seed. No one needs to have created the data to check it.

**Filling** (`storage/filler.rs`): a Pouch announces its free space
(`free_bytes`) and the Pouches of the other users fill it at once, each an
equal share, in 1 MiB blobs of random data (at most 256 per sponsor per
minute) generated from a seed that is thrown away, so the Pouch can neither
compress nor rebuild them. Real fragments evict filler. A Pouch announces how
much it stores (`stored_bytes`) and how many blobs (`fillers`); your own
available space counts filler as free.

**Reputation.** Every challenge result is counted per device and epoch. In
the last 10 % of an epoch each member submits an `Observations` op for the
devices of other users: challenges sent and passed (at most 2880 per device
per epoch, scaled down proportionally beyond that).

**Allowance.** Credit and usage are current values in the ledger, not daily
ones. At most every 120 s each member submits a `Usage` op that replaces its
previous one: for every device of another user it challenged, the **verified
bytes** (what that Pouch announces it stores × its challenge success rate as
seen by the challenger), and, for every other owner, the bytes its own Pouches
hold for them. A user's **credit** is, summed over their devices, the median
of what the challengers verified; their
**usage** is the sum of what the others hold for them (`bp members`). You may
store on other users' Pouches only up to your credit: `bp put` refuses a file
that would exceed it and a Pouch refuses fragments from a user over it; your
own devices' Pouches are not counted. With no credit you store nothing on the
others. A new Pouch is filled with filler at once and challenged within a
minute, so its credit shows up within a few minutes.

QoS is local to each node (`qos.json`); reputation lives in the ledger.

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
| `Leave` | The author leaves: no longer a member, weight removed, rotation due |
| `RotateKey` | New network key, wrapped for every current member |

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
Every 15 s, and whenever it sees a commit more than one block ahead, a node asks
its peers for the blocks after its height (at most 100 per request); a peer
serves them only to a device whose certificate belongs to a current member
(`chain.rs`).

**Key rotation**: the network key is replaced once per epoch, and as soon as an
expulsion passes. The ledger holds the commitment `BLAKE3-derive("billpouch/
key-commitment/v1", key)` of the current key; a `RotateKey` op is valid only
while a rotation is due, must wrap the new key for exactly the current members
and sets the new commitment. The rotator is one validator, chosen by epoch and
moving to the next one every 60 s until a rotation is committed. Every member
opens its own wrap, checks it against the commitment and moves to the new gossip
topic, listening on the old one for 10 more minutes. Expelled members get no
wrap, so they no longer read or reach the gossip, and nobody serves them blocks.
Invitations carry the commitment too: one made with an old key is rejected.

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
- **Network key**: 32 random bytes, made at creation and at every rotation.
  Every entry or rotation carries it encrypted for each member: X25519 (from the member's ed25519 key) with an
  ephemeral key, BLAKE3-derived wrapping key, ChaCha20-Poly1305. It never travels
  in clear and only the member it was wrapped for can open it.
- **Invites**: `bp invite` submits an `AddMember` op, waits for it to be
  committed and prints a token (genesis, creator's device certificate, your
  entry). `bp join` opens the key from the entry, subscribes, syncs the ledger
  from members and waits until it lists you; the daemon then registers the
  device by itself.
- **Transport**: every connection is Noise-encrypted.

# Architecture

## Crates

| Crate | Kind | Role |
|-------|------|------|
| `bp-core` | library | Everything: identity, daemon, networking, coding, storage, control protocol |
| `bp-cli`  | binary `bp` | Thin client: parses commands, talks to the daemon over the control socket |
| `bp-api`  | binary `bp-api` | HTTPS gateway to the same socket, plus the embedded web app |
| `bp-dlna` | library | A DLNA media server over any `Library` that lists files and streams byte ranges; knows nothing of BillPouch |

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

**A Bill is only an interface.** A node that runs only a Bill (the Android
app, a laptop that just reads and writes files) holds no fragments and no
filler, is never pinged, challenged, filled, chosen as a holder, asked for a
fragment or offered a trust bet, and does not challenge anyone. It follows the
ledger, which it needs to know who holds what, and reads and writes your
files with your key. Reputation, credit, availability and trust come only from
Pouches.

**Plugins on a Bill.** `bp hatch bill --dlna [port]` plugs a DLNA media server
(`bp-dlna`) on the Bill: the Bill is its library, so finding the holders,
fetching the fragments, decoding, decrypting and decompressing stay the
Bill's job, and the plugin only speaks UPnP to the TVs and players of the
local network. It announces itself over SSDP (multicast 239.255.255.250:1900),
shows the audio, video and pictures of your files in that network in three
folders, and streams each byte range a player asks for straight from the
network, chunk by chunk, as `bp get --range` does. It serves only clients on
private, loopback or link-local addresses, without any login: anyone on your
local network can watch what the Bill can read. It starts and stops with the
Bill and comes back with it when the daemon restarts.

## Files

The control socket `control.sock` is in the runtime dir (`$XDG_RUNTIME_DIR/billpouch/`,
or the data dir when there is none). Everything else is under the XDG data dir
(`~/.local/share/billpouch/` on Linux):

| File | Contents |
|------|----------|
| `user_ed25519` | Your user key (OpenSSH format, optionally passphrase-encrypted) |
| `device.key`, `device_cert.json` | This device's libp2p key and the certificate your user key signed for it |
| `profile.json` | Alias and creation time |
| `services.json` | The services this node runs, reopened when the daemon starts |
| `daemon.log` | The log of a daemon the CLI started |
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
| Kademlia | Routing table, seeded by invite tokens, `kad_peers.json`, `bootstrap.json` and the addresses in NodeInfo |
| Identify | `/billpouch/id/1.0.0` |
| Gossipsub | One topic per network, `BLAKE3_keyed(network key, "billpouch/topic/v1")`; every message is encrypted with the network key |
| Request-response (CBOR) | `/billpouch/fragment/1.1.0`: store, fetch, ping, Proof-of-Storage |
| AutoNAT + relay client | NAT traversal through a relay (`bp relay connect`) |

Peers are found without any local-network discovery: nodes on one LAN tend to
fail together, so they are no better than any other peer. An invite token
carries the addresses the inviter listens on (VPN ones included), and joining
dials them. Every NodeInfo carries its node's addresses, so whoever accepts it
can reach that node; identify, saved peers (`kad_peers.json`) and
`bp bootstrap add` with an address from `bp status` do the rest.

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

`bp put` passes the file's path to the daemon, which streams it in chunks of
4 MiB (or `--chunk-size`, a power of two from 64 KiB: smaller chunks start
streaming sooner); only one chunk is in memory at a time. `bp put -` reads
stdin through a named pipe, so the size need not be known: the quota is then
checked chunk by chunk. Each file gets a random `file_id` and a random key.

1. **Compress.** Each chunk is compressed with zstd (level 3) when that saves
   at least 3 %; a first chunk that does not compress turns compression off
   for the file (photos, videos and archives are already compressed). A byte
   in front of each chunk says which, so streaming and ranges keep working.
2. **Encrypt.** Each chunk is sealed with ChaCha20-Poly1305 under the file key.
   The key is wrapped (X25519 + ChaCha20-Poly1305, as for the network key) for
   your SSH key and put in the file's manifest: `file_id`, the chunk size, a
   hash of each sealed chunk (checked after decoding), name, size, owner, one
   wrap and one grant per authorized user. Pouches only ever see ciphertext.
   The id of chunk `i` is derived from the file, `BLAKE3("billpouch/chunk/" ‖
   file_id ‖ "/" ‖ i)`, so the ledger records only how many chunks a file has,
   whatever their size.
3. **The folder** (see *Storage tiers*) is the layer you put the file in
   (`--tier`, default T1). Every chosen holder must still have room for one fragment in
   that layer's slice, or the folder is full.
4. **Choose k and the holders** (`coding/params.rs`). With `X_i` = Pouch `i`
   reachable, with probability `p_i` (its conservative availability, below),
   the recovery probability `Pe = P(Σ X_i ≥ k)` over `n` holders is computed
   exactly by dynamic programming, and for a given set of holders `k` is the
   largest (≤ 128) with `Pe ≥ Ph` (`--ph`, default 0.999). Each holder stores
   one fragment of 1/k of the chunk, so the overhead is `n/k`. The holders are
   the subset of the layer that maximises `k/n`, taking Pouches in order of
   availability: a Pouch that cannot raise `k` gets no fragment, since it would
   only add overhead. Ties are broken by a hash of the file id and the Pouch, so
   every chunk of a file goes to the same holders and different files spread.
   If no subset reaches `Ph`, the target becomes a loss chance at most twice
   that of a full copy on every Pouch of the layer, so a Pouch too new to
   add anything gets nothing; if even that is hopeless (a loss chance above
   one half), every Pouch gets a full copy (`k = 1`). `Pe` is reported.
5. **Encode and place.** Systematic RLNC over GF(2⁸): `n` fragments, any `k` of
   which decode. Your own fragment is stored locally; the others are pushed to
   their Pouches with your **owner grant** and the layer. The placement is gossiped as an
   `Index` message and recorded for Proof-of-Storage.

`bp get` looks the file up in your index and fetches up to `BP_FETCH_PARALLEL`
chunks at once (default 4), writing them in order. For each chunk it takes the
fragments it has locally, then asks the file's live holders from the ledger,
nearest first by measured round-trip time, then the Pouches of the gossiped
fragment index and finally every other Pouch. It asks as many as it lacks plus
`BP_FETCH_SPARE` (default 1), uses answers as they arrive, and asks the next
candidate whenever one fails or nothing arrives within three times the nearest
round trip (between 1 and 5 s). With `k` fragments it decodes, checks the
`chunk_id` and decrypts; the output is written as `.bp-partial` and renamed at
the end. With `--range A-B` (or an HTTP `Range` header) only the chunks
holding those bytes are fetched; with `-` as the output (or a named pipe) the
file is written as it arrives, for streaming.

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
A Pouch may offer less than its tier (`--size`); its slices then stop where
the offer ends, so a 512 MiB T1 Pouch has a 512 MiB T1 slice.
With 100 Pouches of which 30 are T2 and 10 are T3, layer T1 has 100 Pouches,
T2 40 and T3 10. A fragment of layer `L` only goes into the `L` slice, and a
Pouch announces its free space per layer (`layer_free`). Usable space and
redundancy therefore come in steps, one per layer.

**Folders.** Each layer is a folder you choose when you store a file
(`bp put --tier T2`, or the folder picker of the web app). `bp storage` and the
web app show each folder's users, Pouches, free space, `k/n` and recovery
probability. A folder works only once **at least 3 users** have a Pouch that
reaches it: with fewer, nobody can cross-check the others. Until then a file
put there stays on your own Pouches that reach the folder (`k = 1`, a full copy
on each), costs no quota and is shown as *local only*; with no such Pouch of
yours the upload is refused. Every 30 s the daemon looks for a local file of
yours whose folder now has 3 users, checks your quota, spreads it over the
folder like a new upload, registers the new placement (an owner may re-place
its own file) and drops its local copies. Example: ten 10 GiB Pouches and one
of 1 TiB make a working T1 folder of eleven Pouches, while the 1 TiB owner's T2
to T4 folders stay local until two more users offer that much.

| Tier | Name | Quota |
|------|------|-------|
| T1 | Pebble | 10 GiB |
| T2 | Stone | 100 GiB |
| T3 | Boulder | 500 GiB |
| T4 | Rock | 1 TiB |
| T5 | Monolith | 5 TiB |

`bp storage` and the web app draw two bars on the scale of what you offer:
the Pouch's setup (others' fragments, filler, free space) and your **quota**
(used, free, not verified yet, and the share that goes to redundancy).

## Quality, Proof-of-Storage, reputation

**Availability for k/n comes from the ledger**, so every node sees the same
value and computes the same `k`, `n` and quota. The ledger keeps, for every
device, the challenges it passed and failed as reported by the other users,
both multiplied by 0.9 at each new epoch (a memory of about ten days). With
`a = 1 + passed` and `b = 1 + failed`, the device's `p_i` is the lower 5 % bound
of Beta(a, b), `mean − 1.645·σ`: a Pouch nobody has reported yet counts as about
0.03, and one that always answers climbs towards 1, so evidence, not hope,
earns a place among the holders.

For every peer the daemon also keeps its own QoS record (`network/qos.rs`):

- **Its own availability estimate**, built the same way from the pings and
  challenges it saw itself, with a memory that decays by 0.9995 per
  observation. It is private: it is this node's forecast when it bets on a
  newcomer, never the input of k/n.
- RTT EWMA (α = 0.125) and challenge success EWMA (β = 0.10, used for credit).
**Repair is the network's job.** After storing a file its owner registers it
in the ledger (`Place`): chunks, `k`, layer, the bytes each holder stores and holders. A holder counts while
it is **live**: its user is a member and the device passed a challenge within
`grace` epochs of the last epoch in which any device of the network passed
one (a network nobody challenges, such as a single user's, ages nobody). An
evicted Pouch `Release`s what it held; a user who leaves or is expelled loses
every place at once; in all these cases live extra holders take the free
places. Every 60 s each Pouch looks at the
registered files of its layers: a file with fewer live holders than planned
lacks that many, and among the Pouches of the layer that do not hold it, the
ones ranked first by a hash of the file and the Pouch, as many as are lacking,
rebuild every chunk of it, so nothing is rebuilt twice. Once it has every
chunk, the Pouch registers itself as a holder (`Hold`). Deleting a file
`Drop`s it.

**Newcomers hold real fragments.** A Pouch with free space (free plus filler)
beyond half of what it offers volunteers for up to 2 files per round that it
does not hold, those with fewest live holders first, as long as the file fits
in the space beyond that half. It rebuilds them like a repair and registers
with `Hold`: a file that already has all its planned holders takes it as an
**extra** holder. Extra holders are not charged to the owner's quota, and live
ones take the place of charged holders that are gone. Real fragments evict
filler, so a new Pouch stores redundancy that serves instead of random data;
filler only fills what no file needs. When less than a quarter of a Pouch is
free or filler, it gives up one extra file per round (`Release`), so charged
fragments always find room.

**Proof of Trust.** A charged holder can hand its place to an extra holder of
another user with `Trust`: it trusts that the fragment the newcomer already
has is safe and deletes its own. It states its confidence, its forecast of the
share of challenges the newcomer will pass during the bet, which is its own
availability estimate of that Pouch. The bet settles after `horizon` epochs:
the outcome is the share of challenges the newcomer's device passed in the
meantime, or 0 as soon as it releases the file or stops counting as live (a
bet on a device nobody challenged is void). It is scored with the Brier rule
against the ledger's public estimate of that device at the time of the bet,
`(passed + 1)/(challenged + 2)` over its whole history: `(public − o)² −
(confidence − o)²`. Honest forecasts pay best, a confident miss costs most,
and betting what the ledger already knows earns nothing: trust is earned by
seeing better than the network. A device that is often offline, such as a
laptop that sleeps at night, fails the challenges of those hours and is
forecast, and scored, accordingly.

**When to trust** is a random event: every minute a holder trusts a given
newcomer with chance `confidence × surplus / rounds`, so the wait is roughly
exponential. `rounds` is the mean wait for a good newcomer, set by
`BP_TRUST_AFTER` (minutes, default 360). `surplus` measures how much the file
can spare: the recovery probability of the file without this holder's
fragment, with every other live holder (the newcomer included) at its
estimated availability, expressed in nines relative to `ph` (capped at 4).
Below `ph` the chance is 0, so a file never drops under its target, and a
file with many more holders than it needs is let go of sooner. Random waits
also keep the holders of a file from all trusting the same newcomer at once.

A member's **trust** is the sum of these scores, multiplied by `decay` at each
settled bet, plus what their invitations earned; it is not voting weight.
**Invitations stake trust**: an invitation stays open for `horizon` epochs;
if the new member leaves or is expelled before then the inviter loses 0.25,
if by then one of their devices proved its storage the inviter gains 0.05,
otherwise nothing changes. A bet on a member you invited is allowed but not
scored, so nobody farms trust on users they brought in. Every open bet and
invitation locks 0.25 of trust: a member has at most `8 + trust / 0.25` of
them open together. Safety floor: a file has at most `n − k` open bets, so it
survives every untested newcomer vanishing. Nobody bets on their own devices.
A node trusts at most one file per network per round. A dropped file voids
its bets. A proposal the ledger does not take is retried
after 10 minutes.

Rebuilding is a **recoding chain**: `k` live holders in turn add `c · own fragment` (random `c`
in GF(2⁸)) to the running sum and
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
much it stores (`stored_bytes`) and how many blobs (`fillers`).

**Reputation.** Every challenge result is counted per device and hour. Once
an hour is over each member submits an `Observations` op for that hour, for the
devices of other users: challenges sent and passed (at most 120 per device per
hour, scaled down proportionally beyond that). The ledger accepts each
member's hours in order and at most two hours late.

**Allowance.** Credit and usage are current values in the ledger, not daily
ones. Each member submits a `Usage` op that replaces its previous one when a
value moved by more than 5 % or one appeared or disappeared (at most every
120 s), and otherwise at most once an hour if anything changed. It carries,
for every device of another user it challenged, the **verified
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

**Quota.** Credit counts raw bytes, but a file costs more than its size: its
chunks go out as `n` fragments of which any `k` rebuild it, so a file of `s`
bytes takes `s·n/k` of credit on the others. Your **quota**, the size of the
files you can store, is therefore your credit × k/n, with k and n those the
network would use for a new file right now (`n` = the fragments that land on
other users' Pouches of the layer). Offering 10 GiB with k/n = 3/4 gives a
7.5 GiB quota once all of it is verified, and never the whole offer: the rest
pays for your files' redundancy. `bp put` checks the file against the quota
left, and k/n moves with the number and stability of the layer's Pouches.

QoS is local to each node (`qos.json`); reputation lives in the ledger.

## Shared ledger and consensus

Each network has a ledger (`ledger.rs`) built by replaying committed blocks of
signed ops from the genesis entry and the creator's device certificate. Every
op carries the time it was signed and is valid for one hour (and at most 5
minutes ahead): the ledger remembers the ids of applied ops only for that hour,
which is enough to refuse a replay, so this set does not grow with history.

| Op | Effect |
|----|--------|
| `AddMember` | Adds a member entry (with the network key wrapped for them) |
| `RegisterDevice` | Adds one of your devices |
| `Observations` | Adds an hour of PoS counts to other users' and their devices' totals |
| `ProposeExpel` / `ApproveExpel` | Expulsion vote |
| `Leave` | The author leaves: no longer a member, weight removed, rotation due |
| `Place` | Owner registers (or re-places) a file: chunks, `k`, layer, the bytes each holder stores and the holders it was planned with |
| `Hold` / `Release` | A user's device starts or stops holding a file's fragments, as a charged or an extra holder |
| `Drop` | Owner deletes a file from the registry |
| `Trust` | A holder hands its place in a file to an extra holder of another user and bets that they keep it |
| `SetParams` | Votes the network parameters; they change when the votes for the same values hold > 2/3 of the weight |
| `RotateKey` | New network key, wrapped for every current member |

**Parameters** (`bp params`) are in the ledger: `ph`, the default target
recovery probability of a file (0.999); `decay`, the share of trust kept at each settled bet
(0.9); `horizon`, the epochs a trust bet lasts (7); `grace`, the epochs a device
may go without passing a challenge before it stops counting as a holder (2).

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

Weight = tier rank (R1 = 1 … R4 = 4), but only for users with a live device
that has passed a challenge, that is a Pouch: once anyone has one, users
without (who only run Bills, and so have no quota either) stay members with no
weight. Until then, in a brand-new network, everyone weighs by tier. With 4 or
more users no user can hold more than a quarter of the total (capped
iteratively).

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
voting device, so their devices cannot equivocate by accident: at each epoch
it is the user's device that most recently passed a challenge, which is a
Pouch, and only while none has, the first device they registered. Nothing is
proposed while no op is pending. Blocks batch ops: the periodic reports
(`Usage`, `Observations`) wait up to 60 s, or until 64 ops are pending, so they
share one block and its votes; any other op is proposed at once and takes the
waiting ones along.

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

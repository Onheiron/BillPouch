# Status and roadmap

Alpha. Current version in `version.txt`; history in [CHANGELOG.md](../CHANGELOG.md).

## Works today

- Identity: SSH ed25519 user key, per-device keys with signed certificates, verified announcements.
- Services `pouch` / `bill` / `post`; storage tiers T1–T5; one Pouch per network per node.
- Discovery (mDNS, Kademlia, bootstrap list, saved peers), gossip, NAT relay client.
- `bp put` / `bp get`: per-user encryption, RLNC coding with adaptive k/n,
  distribution to remote Pouches, targeted fetch via the gossiped fragment index.
- Ping and Proof-of-Storage loops → QoS, fault score, rerouting of fragments
  away from suspected peers, R0–R4 reputation.
- Invite-only networks: signed member registry, network key wrapped per member, encrypted gossip, members-only fragment exchange.
- `pause` / `resume`, `farewell --evict`, `leave [--force]`, `ls`, `storage`, `status`.
- REST gateway with dashboard; Docker smoke test and playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Files | A file is a single chunk, sent to the daemon in memory as a JSON byte array: fine for small files, not for large ones |
| Proof-of-Storage | The challenger does not hold the fragment, so it cannot check the returned hash: any reply passes |
| State | QoS and reputation are in memory and reset when the daemon restarts |
| Leave / evict | Neither fetches your files nor redistributes the evicted fragments; the network relies on PoS rerouting |
| Placement | Round-robin; tiers and reputation do not influence where fragments go or how k/n is computed per tier |
| Available space | Comes out as 0 until enough reliable peers are known |
| `bp-api` | No authentication; bind to localhost only |

## Next

1. Weighted BFT consensus on an encrypted shared log: >2/3 of reputation weight, weights fixed per epoch, per-user cap; reputation per user across their nodes; capabilities from epoch snapshots; expulsion.
2. Envelope encryption: per-file key wrapped for every authorized key, encrypted personal index, sharing files with other users, adding devices.
3. Deterministic placement: preferably one fragment per node, at most ⌊k/4⌋; exact Poisson-binomial k/n; owner-signed fetch and repair permissions.
4. Proof-of-Storage by filling the bid with fragments or random data, Merkle challenges anyone can verify, credit = verified bytes.
5. Real chunking and streaming for `put` / `get`.

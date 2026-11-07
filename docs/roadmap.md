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
- Invites, `pause` / `resume`, `farewell --evict`, `leave [--force]`, `ls`, `storage`, `status`.
- REST gateway with dashboard; Docker smoke test and playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Access control | The network key travels in invites but nothing checks it yet: anyone who knows a network id can subscribe to its gossip and exchange fragments. Data stays encrypted, but membership is not enforced |
| Files | A file is a single chunk, sent to the daemon in memory as a JSON byte array: fine for small files, not for large ones |
| Proof-of-Storage | The challenger does not hold the fragment, so it cannot check the returned hash: any reply passes |
| State | QoS and reputation are in memory and reset when the daemon restarts |
| Leave / evict | Neither fetches your files nor redistributes the evicted fragments; the network relies on PoS rerouting |
| Placement | Round-robin; tiers and reputation do not influence where fragments go or how k/n is computed per tier |
| Available space | Comes out as 0 until enough reliable peers are known |
| `bp-api` | No authentication; bind to localhost only |

## Next

1. Enforce network membership with the network key (gossip and fragment protocol).
2. Real chunking and streaming for `put` / `get`.
3. Verifiable Proof-of-Storage (keep a hash commitment per placed fragment).
4. Persist QoS and reputation.
5. Placement by tier and reputation; per-tier k/n.
6. Complete `leave` / `farewell --evict`: fetch own files, re-replicate fragments.
7. Later: local-only mode, FUSE mount, public bootstrap nodes, mobile bindings.

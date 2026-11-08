# Status and roadmap

Alpha. Current version in the root `Cargo.toml`; history in the GitHub releases.

## Works today

- Identity: SSH ed25519 user key, per-device keys with signed certificates, verified announcements.
- Services `pouch` / `bill` / `post`; storage tiers T1–T5; one Pouch per network per node.
- Discovery (mDNS, Kademlia, bootstrap list, saved peers), gossip, NAT relay client.
- `bp put` / `bp get`: random per-file key wrapped per authorized user, RLNC coding with adaptive k/n,
  distribution to remote Pouches, targeted fetch via the gossiped fragment index.
- Rendezvous placement, one fragment per Pouch when possible and at most ⌊k/4⌋ otherwise, with the exact recovery probability `P(Σ m_i X_i ≥ k)`; fragments stored and served only with grants signed by the file's owner.
- Personal file index sealed with your SSH key, stored in the network and synced to all your devices; `bp share` with other members; extra devices with a self-invite.
- Ping and Proof-of-Storage loops → QoS, fault score, rerouting of fragments
  away from suspected peers.
- Invite-only networks: network key wrapped per member, encrypted gossip, members-only fragment exchange.
- Shared ledger per network, committed by weighted BFT consensus: members, devices, per-user reputation from PoS observations, epoch snapshots, capabilities, expulsion votes, network key rotation every epoch and after each expulsion.
- `pause` / `resume`, `farewell --evict`, `leave [--force]`, `ls`, `storage`, `status`.
- REST gateway with dashboard; Docker smoke test and playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Files | A file is a single chunk, sent to the daemon in memory as a JSON byte array: fine for small files, not for large ones |
| Proof-of-Storage | The challenger does not hold the fragment, so it cannot check the returned hash: any reply passes |
| State | QoS is in memory and resets when the daemon restarts |
| Index versions | Every index update stores a new version; old versions stay on the Pouches |
| Unsharing | A file cannot be unshared or deleted yet |
| Leave / evict | Neither fetches your files nor redistributes the evicted fragments; the network relies on PoS rerouting |
| Placement | Rendezvous order and stability only: storage tiers and reputation do not change where fragments go |
| Repair | Only the owner reroutes fragments away from a suspected Pouch |
| Available space | Comes out as 0 until enough reliable peers are known |
| `bp-api` | No authentication; bind to localhost only |

## Next

1. Proof-of-Storage by filling the bid with fragments or random data, Merkle challenges anyone can verify, credit = verified bytes.
2. Real chunking and streaming for `put` / `get`.

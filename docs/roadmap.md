# Status and roadmap

Alpha. Current version in the root `Cargo.toml`; history in the GitHub releases.

## Works today

- Identity: SSH ed25519 user key, per-device keys with signed certificates, verified announcements.
- Services `pouch` / `bill` / `post`; storage tiers T1–T5; one Pouch per network per node.
- Discovery (mDNS, Kademlia, bootstrap list, saved peers), gossip, NAT relay client.
- `bp put` / `bp get`: files streamed in 4 MiB chunks, random per-file key wrapped per authorized user, RLNC coding with adaptive k/n,
  distribution to remote Pouches, targeted fetch via the gossiped fragment index.
- Rendezvous placement, one fragment per Pouch when possible and at most ⌊k/4⌋ otherwise, with the exact recovery probability `P(Σ m_i X_i ≥ k)`; fragments stored and served only with grants signed by the file's owner.
- `bp rm` and `bp unshare` with owner-signed orders that Pouches enforce; old index versions are deleted.
- Personal file index sealed with your SSH key, stored in the network and synced to all your devices; `bp share` with other members; extra devices with a self-invite.
- Ping and Proof-of-Storage loops → QoS, fault score, rerouting of fragments
  away from suspected or evicting Pouches, rebuilt from the other holders. Challenges ask for a random 4 KiB block with its Merkle path, checked against a root the challenger kept.
- Per-user credit in verified bytes and usage in held bytes, recorded in the ledger every epoch; Pouches refuse users storing more than they host plus 1 GiB.
- Free Pouch space is filled at once with random blobs by the other Pouches and challenged like fragments; real fragments evict filler.
- Invite-only networks: network key wrapped per member, encrypted gossip, members-only fragment exchange.
- Shared ledger per network, committed by weighted BFT consensus: members, devices, per-user reputation from PoS observations, epoch snapshots, capabilities, expulsion votes, network key rotation every epoch and after each expulsion.
- `pause` / `resume`, `farewell --evict`, `leave [--force]`, `ls`, `storage`, `status`.
- REST gateway with dashboard; Docker smoke test and playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Allowance | Usage and credit lag one epoch, and a user who joined today is not limited until tomorrow |
| State | QoS is in memory and resets when the daemon restarts |
| Leave / evict | `bp leave` does not fetch your files first |
| Placement | Rendezvous order and stability only: storage tiers and reputation do not change where fragments go |
| Repair | Only the owner repairs its chunks: a chunk whose owner stays offline is not repaired |
| Available space | Comes out as 0 until enough reliable peers are known |
| `bp-api` | No authentication; bind to localhost only |

## Next

1. Persist QoS across restarts.


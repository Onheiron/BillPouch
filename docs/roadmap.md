# Status and roadmap

Alpha. Current version in the root `Cargo.toml`; history in the GitHub releases.

## Works today

- Identity: SSH ed25519 user key, per-device keys with signed certificates, verified announcements.
- Services `pouch` / `bill` / `post`; storage tiers T1–T5; one Pouch per network per node.
- Discovery (mDNS, Kademlia, bootstrap list, saved peers), gossip, NAT relay client.
- `bp put` / `bp get`: files streamed in 4 MiB chunks, random per-file key wrapped per authorized user, RLNC coding with adaptive k/n,
  distribution to remote Pouches, targeted fetch via the gossiped fragment index.
- Tiers as nested layers: a chunk gets one fragment on every Pouch of the lowest layer with room, `k` from the exact recovery probability `P(Σ X_i ≥ k)`; fragments stored and served only with grants signed by the file's owner.
- `bp rm` and `bp unshare` with owner-signed orders that Pouches enforce; old index versions are deleted.
- Personal file index sealed with your SSH key, stored in the network and synced to all your devices; `bp share` with other members; extra devices with a self-invite.
- Ping and Proof-of-Storage loops → QoS, fault score, rerouting of fragments
  away from suspected or evicting Pouches, rebuilt from the other holders. Challenges ask for a random 4 KiB block with its Merkle path, checked against a root the challenger kept.
- Per-user credit in verified bytes and usage in held bytes, recorded in the ledger every epoch; Pouches refuse users storing more than they host plus 1 GiB.
- Free Pouch space is filled at once with random blobs by the other Pouches and challenged like fragments; real fragments evict filler.
- Invite-only networks: network key wrapped per member, encrypted gossip, members-only fragment exchange.
- Shared ledger per network, committed by weighted BFT consensus: members, devices, per-user reputation from PoS observations, epoch snapshots, capabilities, expulsion votes, network key rotation every epoch and after each expulsion.
- `pause` / `resume`, `farewell --evict`, `ls`, `storage`, `status`; `leave` records the departure in the ledger, deletes your files and rotates the key without you.
- REST gateway with dashboard; Docker smoke test and playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Allowance | Usage and credit lag one epoch, and a user who joined today is not limited until tomorrow |
| Repair | Only the owner repairs its chunks: a chunk whose owner stays offline is not repaired |
| Available space | Comes out as 0 until enough reliable peers are known |
| `bp-api` | A single bearer token and no TLS: keep it on localhost or behind a TLS proxy |

## Next

1. Repair by other members when a chunk's owner is offline, with owner-signed repair permissions.


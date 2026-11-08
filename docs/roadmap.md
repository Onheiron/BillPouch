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
- Proof-of-Storage done by the network: every node challenges random Pouches of other users on a random 4 KiB block of a chunk of a shared layer or of a filler blob, checked against a commitment signed by the device that produced the data; credit is the median of the challengers' reports.
- Allowance: credit (bytes verified on your Pouches) and usage (bytes the others hold for you) are current values in the ledger; you store on the others only up to your credit.
- Repair by the network: a Pouch that joins a layer catches up on every chunk of it through recoding chains across `k` Pouches.
- Free Pouch space is filled at once with random blobs by the other users' Pouches; real fragments evict filler.
- Invite-only networks: network key wrapped per member, encrypted gossip, members-only fragment exchange.
- Shared ledger per network, committed by weighted BFT consensus: members, devices, per-user reputation from PoS observations, epoch snapshots, capabilities, expulsion votes, network key rotation every epoch and after each expulsion.
- `pause` / `resume`, `farewell --evict`, `ls`, `storage`, `status`; `leave` records the departure in the ledger, deletes your files and rotates the key without you.
- Local REST gateway and dashboard over HTTPS, with sessions signed by your SSH key (`bp dashboard`); Docker smoke test and playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Repair | A lost Pouch lowers every chunk's margin until a new one joins its layer |
| Credit | A Pouch's announced `stored_bytes` is trusted up to its challenge success rate: one that holds half and lies about the rest fails about half the filler challenges |


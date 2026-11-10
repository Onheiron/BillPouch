# Status and roadmap

Alpha. Current version in the root `Cargo.toml`; history in the GitHub releases.

## Works today

- **Identity**: SSH ed25519 user key, per-device keys with signed certificates, verified announcements.
- **Services** `pouch` / `bill` / `post`; storage tiers T1–T5 as nested layers; one Pouch per network per node.
- **Discovery** through invite tokens, addresses in announcements, Kademlia, bootstrap list and saved peers; gossip; NAT relay client. No mDNS.
- **Invite-only networks**: network key wrapped per member, encrypted gossip, members-only fragment exchange.
- **Shared ledger** per network, committed by weighted BFT consensus: members, devices, per-user reputation, per-device challenge counts, epoch snapshots, capabilities, expulsion votes, key rotation every epoch and after each expulsion, voted network parameters (`bp params`). Periodic reports share batched blocks; ops expire after an hour.
- **Folders**: each tier is a folder you put files in (`--tier`). A folder spreads files once at least 3 users have a Pouch that reaches it; until then files stay on your own Pouches and are spread automatically when it fills up.
- **Files**: streamed in 4 MiB chunks, random per-file key wrapped per authorized user, systematic RLNC with `k/n` chosen to reach `ph` from the availability recorded in the ledger (the same for every node). `bp get` fetches from the nearest holders first, with spares, answers as they arrive and several chunks at once.
- **Registry of holders** in the ledger (`Place`, `Hold`, `Release`, `Drop`): repair when a holder is gone (left, expelled, evicted, or silent for `grace` epochs), recoding chains so nobody sees the file.
- **Newcomers** hold real fragments as extra holders instead of filler; filler only fills what no file needs.
- **Proof of Trust**: holders hand their place to a newcomer at random times (sooner for confident forecasts and redundant files, never below `ph`) and bet on its availability, scored with Brier against the ledger's estimate; invitations stake trust; trust is not voting weight.
- **Proof-of-Storage** by the network on random 4 KiB blocks of fragments or filler, against commitments signed by the producing device; credit is the median of the challengers' reports; quota = credit × k/n.
- `bp rm` / `bp unshare` with owner-signed orders; personal file index synced across your devices; `bp share`.
- `pause` / `resume`, `farewell --evict`, `ls`, `storage`, `status`, `members`, `leave`.
- Web app over HTTPS with sessions signed by your SSH key (`bp dashboard`): quota, Pouches, folders, members and trust, files.
- Android app (Bill only), built by CI as a debug APK.
- Docker smoke test in CI and a 6-node playground.

## Known limitations

| Area | Limitation |
|------|------------|
| Small networks | In a network of two users, if one disappears the other can prove nothing: the missing user's Pouches count as holders until expelled |
| Folders | Spreading a local file drops its copy on the node that spreads it; copies on your other devices stay until removed by hand |
| Folders | Your file index always goes to T1, so it stays local while T1 has fewer than 3 users |
| Availability | A new Pouch counts for `k/n` only after its first full hour of reported challenges; the list of candidate Pouches still comes from announcements |
| Trust | The whole trust cycle (hand over, delete, settle) is covered by ledger tests only, not by an end-to-end test |
| Credit | A Pouch's announced `stored_bytes` is trusted up to its challenge success rate |

## TODO

- **Smaller ops**: sign ops with the author's fingerprint instead of the whole SSH key (the key is already in the member registry), and store and gossip ledger blocks in a binary encoding (CBOR) instead of JSON with hex. About half the size of every op.
- **Snapshots and pruning**: the network periodically agrees on the ledger state at a height (certified by that block's precommits); nodes keep the snapshot and recent blocks and drop the rest, and a joining node downloads the snapshot instead of replaying everything. The old history can be archived as a BillPouch file in the network itself.
- **Distributed file registry**: for hundreds of nodes and millions of files, keep each file's placement record with its holders and only a Merkle root of the registry in the ledger, so the replicated state stops growing with the number of files.
- Playground: configurable epoch length, so bets, invitations and silent holders settle in minutes instead of days.
- Android: signed release APK in the release workflow (needs the keystore secrets).

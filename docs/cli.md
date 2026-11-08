# CLI reference — `bp`

`bp <command> --help` prints the authoritative flags. Global option:
`--passphrase <P>` (or `BP_PASSPHRASE`) unlocks a passphrase-protected SSH key.
Commands other than `login`, `logout` and `bootstrap` need the daemon; it is
started automatically when missing.

## Identity

| Command | Description |
|---------|-------------|
| `bp login [--ssh-key <path>] [--alias <name>]` | Log in on this device. `--ssh-key` uses an existing OpenSSH ed25519 key as your identity; without it a new one is generated (encrypted if `--passphrase` is given). Prints your public key, to share when asking for an invite |
| `bp logout` | Delete the user key copy, device key and certificate from this machine |

## Services

| Command | Description |
|---------|-------------|
| `bp hatch <pouch\|bill\|post> --network <id> [--tier T1..T5]` | Start a service in a network you are a member of. `--tier` is required for `pouch`; one Pouch per network per node |
| `bp status` | Identity, services, reputation, per-Pouch storage, peer count (re-announces first) |
| `bp flock` | Local services, joined networks and every known peer |
| `bp pause <service_id> --eta <minutes>` | Announce planned maintenance |
| `bp resume <service_id>` | Back from maintenance |
| `bp farewell <service_id>` | Stop a service |
| `bp farewell <service_id> --evict` | Stop a Pouch for good: delete its fragments and gossip `evicting` |

## Networks

| Command | Description |
|---------|-------------|
| `bp network create <name>` | Create a network; you are its first member and its creator |
| `bp invite <network> "<ssh public key>"` | Submit the invitation to the network, wait until it is committed and print (on stdout) the token they join with. Requires the `invite` capability (R2+, or the creator while nobody is R2) |
| `bp members <network>` | Members with tier, voting weight, bytes they host (verified by the others last epoch) and store, devices and pending expulsions; expelled members |
| `bp expel <network> <fingerprint>` | Propose an expulsion (R3+, or the creator while nobody is R3) or approve one (R1+). It passes with > 2/3 of the other members' weight and at least half of them; the network key is then rotated without the expelled member |
| `bp invite <network> "<your own public key>"` | Token for another device of yours: log in there with the same SSH key and `bp join` it |
| `bp join <token>` | Join with a token (it only works for the key it was made for) and wait until the ledger is synced from a member |
| `bp leave <network_id>` | Leave a network; refuses while services run on it or you own files there, and prints how to stop them and save the files |
| `bp leave <network_id> --force` | Evict Pouches, stop other services, delete your files from the network, record the departure in the ledger (the key rotates without you) and forget the network |

## Files

| Command | Description |
|---------|-------------|
| `bp put <file> --network <id> [--ph <p>] [--q-target <q>] [--name <name>]` | Stream a file in 4 MiB chunks: encrypt, encode and distribute; prints its `file_id`. `--ph` = target recovery probability (default 0.999), `--q-target` = redundancy on top of `k` (default 1.0) |
| `bp get <file_id> <output> --network <id>` | Fetch, decode and decrypt one of your files or one shared with you, chunk by chunk |
| `bp rm <file_id> --network <id>` | Delete one of your files (every Pouch drops its fragments), or remove a shared file from your list |
| `bp ls [--network <id>]` | Your files and the ones shared with you, from your index (synced across your devices) |
| `bp unshare <file_id> <fingerprint> --network <id>` | Withdraw a member's access: Pouches refuse them the fragments and the file leaves their list |
| `bp share <file_id> <fingerprint> --network <id>` | Share one of your own files with a member: the file key is wrapped for their SSH key, a fetch grant is signed for them and both are delivered; it then shows up in their `bp ls` |
| `bp storage [--network <id>]` | Quota, used and available space per Pouch, plus upload totals |

## Connectivity

| Command | Description |
|---------|-------------|
| `bp bootstrap list` / `add <multiaddr>` / `remove <multiaddr>` | Manage `bootstrap.json` WAN peers (`/…/p2p/<PeerId>`); applied at next daemon start |
| `bp relay connect <multiaddr>` | Reserve a circuit-relay v2 slot so peers can reach you behind NAT |

## Environment

| Variable | Effect |
|----------|--------|
| `BP_PASSPHRASE` | Passphrase for an encrypted identity |
| `BP_LISTEN_PORT` | Fixed TCP listen port for the daemon (default: random) |
| `RUST_LOG` | Log filter, e.g. `bp_core=debug` |

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
| `bp farewell <service_id> --evict` | Stop a Pouch for good: delete its fragments, gossip `evicting`, reputation R0 for 30 days |

## Networks

| Command | Description |
|---------|-------------|
| `bp network create <name>` | Create a network; you are its first member and its creator |
| `bp invite <network> "<ssh public key>"` | Add someone to the registry and print (on stdout) the token they join with. Requires the `invite` capability: today only the creator has it |
| `bp join <token>` | Join with a token; it only works for the key it was made for |
| `bp leave <network_id>` | Leave a network; refuses while services are running on it and prints the commands to stop them |
| `bp leave <network_id> --force` | Evict Pouches and stop other services on the network, then leave. Files are not fetched first: `bp get` anything you need before |

## Files

| Command | Description |
|---------|-------------|
| `bp put <file> --network <id> [--ph <p>] [--q-target <q>] [--name <name>]` | Encrypt, encode and distribute a file; prints its `chunk_id`. `--ph` = target recovery probability (default 0.999), `--q-target` = redundancy on top of `k` (default 1.0) |
| `bp get <chunk_id> <output> --network <id>` | Fetch, decode and decrypt a file you stored |
| `bp ls [--network <id>]` | Files uploaded from this node (all networks by default) |
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

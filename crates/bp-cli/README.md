# bp-cli

Command-line interface for BillPouch — a P2P social distributed filesystem.

## Rationale

`bp-cli` is a **thin client binary** (`bp`). Its only responsibilities are:

1. Parse command-line arguments (via `clap`).
2. Auto-start the daemon in the background if it is not already running (`bp hatch`).
3. Forward every request to the daemon over a Unix socket using `ControlClient`.
4. Pretty-print the response to the terminal.

All business logic, networking and persistence live in `bp-core`. The CLI never imports
`libp2p` directly and never touches the filesystem beyond reading the socket path from
`bp_core::config`.

## Commands

| Command | Description |
|---|---|
| `bp login [--alias <name>]` | Generate an Ed25519 identity and store it locally. |
| `bp logout` | Remove your identity key from this machine. |
| `bp hatch <type> [--network <id>]` | Start a service (`bill`, `pouch`, or `post`). Launches the daemon automatically if needed. |
| `bp flock` | Display all known peers and a summary of joined networks. |
| `bp farewell <service_id>` | Stop a running service by its UUID. |
| `bp join <network_id>` | Subscribe to a new network's gossip topic. |

The hidden `bp --daemon` flag is used internally by `bp hatch` to spawn the daemon
process; it is not intended for direct use.

## Architecture

```
bp (CLI process)
  │
  │  Unix socket  (~/.local/share/billpouch/control.sock)
  ▼
bp --daemon (daemon process)
  ├── NetworkLoop    (libp2p swarm)
  ├── EvictionTask   (stale peer cleanup)
  └── ControlServer  (dispatches ControlRequests)
```

The CLI connects, sends **one** JSON request, reads **one** JSON response, then closes
the connection. The daemon runs indefinitely until all services are stopped.

## Error handling

All errors use `anyhow` for rich, context-annotated messages.  
`BpError` values from `bp-core` are converted automatically via `anyhow::Error`.

## Modules

| Module | Purpose |
|---|---|
| `client` | `ControlClient` — connects to the Unix socket, serialises requests, deserialises responses. |
| `commands::auth` | `login` and `logout` handlers. |
| `commands::hatch` | `hatch` handler; starts the daemon if needed. |
| `commands::flock` | `flock` handler; renders the peer/network table. |
| `commands::farewell` | `farewell` handler; stops a service. |
| `commands::join` | `join` handler; subscribes to a network. |

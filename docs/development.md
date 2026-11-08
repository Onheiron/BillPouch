# Development

## Build and check

Stable Rust (edition 2021). The same three commands run in CI on Linux and macOS:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Tests

| Where | What |
|-------|------|
| `#[cfg(test)]` in `bp-core/src` | Unit tests: GF(2⁸), RLNC, coding params, QoS, storage, encryption, identity, invites, ledger, consensus (simulated network), chain sync |
| `bp-core/tests/architecture_test.rs` | Identity on disk, gossip state, swarm build, control server over a real Unix socket, network loop |
| `bp-core/tests/integration_test.rs` | `put_file` / `get_file` through the control server |
| `bp-cli` | `Cli::command().debug_assert()` catches clap definition errors |
| `docker/smoke-test.sh` | 4 containers (pouch, bill, post, laptop) join by invite, discover each other and gossip; carlo shares a 9 MiB encrypted file (3 chunks) with marco, a second device of carlo (laptop) syncs carlo's index and reads the file, the laptop and marco open Pouches, carlo fills them and the network challenges and credits marco's, the laptop's new Pouch catches up on the existing files through recoding chains, carlo revokes marco and deletes a file, lucia (no Pouch, no credit) is refused, then pouch expels post and the others move to the rotated key |

Tests that change `HOME`/`XDG_DATA_HOME` hold a `static ENV_LOCK: Mutex<()>`.

## Docker

One image (`Dockerfile`, entrypoint `docker/entrypoint.sh`) serves both setups.
The entrypoint logs in and writes its public key to the shared `/shared` volume.
The `FOUNDER=true` node creates `$NETWORK` and writes a join token for every key
that shows up; the others wait for their token and join. Then each node hatches
`$SERVICES` and either waits or (`INTERACTIVE=true`) opens a shell.

```bash
# Smoke test (what CI runs)
docker compose -f docker-compose.smoke.yml up --build -d
./docker/smoke-test.sh
docker compose -f docker-compose.smoke.yml down -v

# Playground: carlo, marco, lucia, elena, paolo + you ("me")
./playground.sh up            # builds billpouch:local from this checkout
./playground.sh enter         # shell on your node; try bp status, bp put …
./playground.sh status        # bp status on every node
./playground.sh app           # web app of your node at https://127.0.0.1:3000
./playground.sh down
```

Every `./playground.sh up` starts from a clean network.

## CI and releases (`.github/workflows/`)

| Workflow | Trigger | Does |
|----------|---------|------|
| `ci.yml` | push to `main` | fmt, clippy, tests (Linux + macOS) |
| `smoke.yml` | push to `main` | Docker smoke test |
| `security.yml` | push to `main`, weekly | `cargo deny check` (advisories, licenses, sources) |
| `release.yml` | `vX.Y.Z` tag | Checks the tag matches `Cargo.toml`, builds `bp` for Linux x86_64 and macOS x86_64/aarch64, publishes the GitHub release with generated notes |
| `docker.yml` | `vX.Y.Z` tag, manual run | Builds and pushes `onheirongithub/billpouch` (`latest` + tag) |

Everything is committed straight to `main`; there are no pull requests. To
release, bump `version` in the root `Cargo.toml`, commit, then tag and push:

```bash
git tag v0.4.0
git push origin main v0.4.0
```

To publish an ad-hoc image run the Docker workflow manually from the Actions tab.

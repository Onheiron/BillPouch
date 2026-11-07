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
| `docker/smoke-test.sh` | 3 containers (pouch, bill, post) join by invite, discover each other and gossip; then pouch expels post and the other two move to the rotated key |

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
./playground.sh up            # pulls onheirongithub/billpouch:latest
./playground.sh enter         # shell on your node; try bp status, bp put …
./playground.sh status        # bp status on every node
./playground.sh down
```

Playground nodes keep their data in `playground/data/<node>/`.

## CI and releases (`.github/workflows/`)

| Workflow | Trigger | Does |
|----------|---------|------|
| `ci.yml` | push / PR to `main` | fmt, clippy, tests (Linux + macOS) |
| `smoke.yml` | push / PR to `main` | Docker smoke test |
| `commitlint.yml` | PR | Conventional commit messages (needed by release-please) |
| `security.yml` | push to `main`, weekly | `cargo deny check` (advisories, licenses, sources) |
| `release-please.yml` | push to `main` | Keeps a release PR open; merging it tags `billpouch-vX.Y.Z`, builds `bp` for Linux x86_64 and macOS x86_64/aarch64, publishes the GitHub release and the Docker image |
| `docker.yml` | called by release-please, manual run, or `v*`/`billpouch-v*` tag | Builds and pushes `onheirongithub/billpouch` (`latest` + tag) |

Versions are owned by release-please: it bumps `version.txt`,
`.release-please-manifest.json`, `Cargo.toml` and `CHANGELOG.md`. Don't bump
them by hand and don't push version tags. For an ad-hoc image (e.g. to refresh
the playground) run the Docker workflow manually from the Actions tab.

# BillPouch — instructions for coding agents

P2P social distributed filesystem in Rust. Read `README.md`, then `docs/`
(`architecture.md` for how it works, `api.md` for the control protocol).

## Layout

- `crates/bp-core` — library with all logic. No `println!`, no `process::exit`;
  errors are `BpError` / `BpResult` (`thiserror`); shared daemon state is
  `DaemonState` (`control/server.rs`).
- `crates/bp-cli` — `bp` binary; `anyhow`; talks to the daemon only through
  `client::ControlClient` (never libp2p directly).
- `crates/bp-api` — REST gateway + dashboard; same socket via `bp_core::control::client`.

## Rules

- Logging with `tracing` only. Comments, docs and identifiers in English.
- Keep it minimal: no speculative APIs, no code without a caller, no
  duplicated logic — extend the existing helper instead.
- A new `ControlRequest` needs: handler in `dispatch`, CLI command, REST route
  if useful, a row in `docs/api.md`, and a test.
- Update the relevant page in `docs/` in the same change; one topic, one place.
- Before pushing: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Conventional commits (`feat:`, `fix:`, `refactor:`, `docs:`, `chore:` …).
  Versions and tags belong to release-please: never bump them by hand.
- Tests that change `HOME` / `XDG_DATA_HOME` must hold `ENV_LOCK`.

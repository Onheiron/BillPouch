# ── Stage 1: chef — compute dependency recipe ───────────────────────────────
FROM rust:1.88-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

# ── Stage 2: planner — capture what deps to download ────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: builder — cache deps layer, then build app ─────────────────────
FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
# This layer is cached as long as Cargo.toml/Cargo.lock don't change.
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin bp

# ── Stage 4: runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates socat \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/bp /usr/local/bin/bp
COPY smoke/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENV RUST_LOG=bp_core=debug,bp_cli=debug

ENTRYPOINT ["/entrypoint.sh"]

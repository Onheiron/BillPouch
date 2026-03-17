//! # bp — BillPouch CLI
//!
//! ```text
//!  Usage:
//!    bp login  [--alias <name>]     Create / display your identity
//!    bp logout                      Remove your identity from this machine
//!    bp hatch  <type> [--network <id>]  Start a service (bill|pouch|post)
//!    bp flock                       Show peers and network summary
//!    bp farewell <service_id>       Stop a running service
//!    bp join <network_id>           Join a BillPouch network
//!    bp --daemon                    (internal) run the background daemon
//! ```

mod client;
mod commands;

use clap::{Parser, Subcommand};
use std::collections::HashMap;

const DEFAULT_NETWORK: &str = "public";

#[derive(Parser)]
#[command(
    name = "bp",
    about = "🦤 BillPouch — P2P social distributed filesystem",
    version,
    long_about = None,
)]
struct Cli {
    /// (Internal) Run as background daemon — not meant to be called directly.
    #[arg(long, hide = true)]
    daemon: bool,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create your identity (keypair) and log in to BillPouch.
    Login {
        /// Optional human-readable alias for this identity.
        #[arg(long, short)]
        alias: Option<String>,
    },

    /// Remove your identity from this machine.
    Logout,

    /// Hatch (start) a new service: bill | pouch | post.
    Hatch {
        /// Service type to start: bill | pouch | post.
        service_type: String,

        /// Network ID to join (default: "public").
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        /// For pouch: how many bytes to bid (e.g. 10737418240 for 10 GiB).
        #[arg(long)]
        storage_bytes: Option<u64>,

        /// For bill: local mount path for personal files.
        #[arg(long)]
        mount: Option<String>,
    },

    /// Show all known peers and network info.
    Flock,

    /// Kill a running service by its service ID.
    Farewell {
        /// The service ID returned by `bp hatch`.
        service_id: String,
    },

    /// Join an existing BillPouch network.
    Join {
        /// The network identifier (e.g. "my-team-net").
        network_id: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialise structured logging (RUST_LOG env var or default to info).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bp_core=info,bp_cli=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // ── Daemon mode ───────────────────────────────────────────────────────
    if cli.daemon {
        bp_core::daemon::run_daemon().await?;
        return Ok(());
    }

    // ── CLI commands ──────────────────────────────────────────────────────
    match cli.command {
        None => {
            println!("🦤 BillPouch (bp) — P2P social distributed filesystem");
            println!("   Run `bp --help` for usage.");
        }

        Some(Cmd::Login { alias }) => {
            commands::auth::login(alias).await?;
        }

        Some(Cmd::Logout) => {
            commands::auth::logout().await?;
        }

        Some(Cmd::Hatch {
            service_type,
            network,
            storage_bytes,
            mount,
        }) => {
            let svc_type: bp_core::service::ServiceType = service_type.parse()?;
            let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
            if let Some(bytes) = storage_bytes {
                metadata.insert("storage_bytes".into(), serde_json::Value::from(bytes));
            }
            if let Some(path) = mount {
                metadata.insert("mount_path".into(), serde_json::Value::from(path));
            }
            commands::hatch::hatch(svc_type, network, metadata).await?;
        }

        Some(Cmd::Flock) => {
            commands::flock::flock().await?;
        }

        Some(Cmd::Farewell { service_id }) => {
            commands::farewell::farewell(service_id).await?;
        }

        Some(Cmd::Join { network_id }) => {
            commands::join::join(network_id).await?;
        }
    }

    Ok(())
}

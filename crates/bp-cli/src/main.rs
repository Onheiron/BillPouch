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
//!    bp bootstrap list              Show configured WAN bootstrap nodes
//!    bp bootstrap add <addr>        Add a bootstrap node
//!    bp bootstrap remove <addr>     Remove a bootstrap node
//!    bp relay connect <addr>        Dial a relay node for NAT traversal
//!    bp offer <bytes> [--duration <secs>] [--price <tokens>]  Propose storage
//!    bp agree <offer_id>            Accept a received storage offer
//!    bp agreements [--network <id>] List local agreements
//!    bp offers [--network <id>]     List received storage offers
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

    /// Store a local file in the active Pouch (RLNC erasure-coded).
    Put {
        /// Path to the file to store.
        file: std::path::PathBuf,

        /// Network ID whose Pouch will store the fragments (default: "public").
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        /// Target recovery probability: probability of recovering the file at
        /// any given moment (default: 0.999 = 99.9%).
        #[arg(long, default_value_t = 0.999)]
        ph: f64,

        /// Redundancy overhead fraction on top of the recovery threshold
        /// (default: 1.0 = one extra "copy-equivalent" of fragments).
        #[arg(long, default_value_t = 1.0)]
        q_target: f64,
    },

    /// Retrieve and decode a stored chunk by its chunk_id.
    Get {
        /// The chunk_id returned by `bp put` (BLAKE3 prefix).
        chunk_id: String,

        /// Where to write the recovered file.
        output: std::path::PathBuf,

        /// Network ID to search (default: "public").
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,
    },

    /// Manage WAN bootstrap nodes (stored in bootstrap.json).
    ///
    /// Bootstrap nodes let the daemon discover peers on the public internet
    /// where mDNS multicast is not available.  Changes take effect on the
    /// next daemon restart.
    Bootstrap {
        #[command(subcommand)]
        action: BootstrapAction,
    },

    /// NAT traversal — connect to a relay node for circuit-relay v2.
    ///
    /// Dials the given relay peer and establishes a reservation so this
    /// node is reachable from the public internet even when behind NAT.
    Relay {
        #[command(subcommand)]
        action: RelayAction,
    },

    /// Broadcast a storage offer on the marketplace gossip topic.
    ///
    /// The offer is flooded to all peers on `network_id` via gossipsub.
    /// Other nodes can accept it with `bp agree <offer_id>`.
    Offer {
        /// Storage capacity to offer in bytes (e.g. 1073741824 = 1 GiB).
        bytes: u64,

        /// How long the offer is valid (default: 86400 = 24 h).
        #[arg(long, default_value_t = 86_400)]
        duration: u64,

        /// Optional price in protocol tokens (default: 0 = free).
        #[arg(long, default_value_t = 0)]
        price: u64,

        /// Network to broadcast the offer on.
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,
    },

    /// Accept a received storage offer.
    ///
    /// Creates a local `StorageAgreement` and broadcasts acceptance to the
    /// offerer via the marketplace gossip topic.
    Agree {
        /// The offer ID to accept (from `bp offers`).
        offer_id: String,
    },

    /// List local storage agreements (as offerer or requester).
    Agreements {
        /// Filter by network (default: "" = all networks).
        #[arg(long, short, default_value = "")]
        network: String,
    },

    /// List storage offers received from remote peers.
    Offers {
        /// Filter by network (default: "" = all networks).
        #[arg(long, short, default_value = "")]
        network: String,
    },
}

#[derive(Subcommand)]
enum BootstrapAction {
    /// List all configured bootstrap nodes.
    List,
    /// Add a bootstrap node (must include /p2p/`<PeerId>`).
    Add {
        /// Multiaddr of the bootstrap node, e.g.
        /// `/ip4/203.0.113.1/tcp/4001/p2p/12D3KooW...`
        addr: String,
    },
    /// Remove a bootstrap node by its multiaddr string.
    Remove {
        /// The exact multiaddr string to remove.
        addr: String,
    },
}

#[derive(Subcommand)]
enum RelayAction {
    /// Dial a relay node and establish a circuit-relay v2 reservation.
    ///
    /// Once the reservation is accepted, this node becomes reachable at
    /// `/p2p-circuit` addresses through the relay, enabling inbound
    /// connections from peers on the public internet.
    Connect {
        /// Full multiaddr of the relay node including its PeerId, e.g.
        /// `/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW...`
        addr: String,
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

        Some(Cmd::Put {
            file,
            network,
            ph,
            q_target,
        }) => {
            commands::put::put(file, network, ph, q_target).await?;
        }

        Some(Cmd::Get {
            chunk_id,
            output,
            network,
        }) => {
            commands::get::get(chunk_id, network, output).await?;
        }

        Some(Cmd::Bootstrap { action }) => match action {
            BootstrapAction::List => {
                commands::bootstrap::list()?;
            }
            BootstrapAction::Add { addr } => {
                commands::bootstrap::add(addr)?;
            }
            BootstrapAction::Remove { addr } => {
                commands::bootstrap::remove(&addr)?;
            }
        },

        Some(Cmd::Relay { action }) => match action {
            RelayAction::Connect { addr } => {
                commands::relay::connect(addr).await?;
            }
        },

        Some(Cmd::Offer {
            bytes,
            duration,
            price,
            network,
        }) => {
            commands::marketplace::propose(network, bytes, duration, price).await?;
        }

        Some(Cmd::Agree { offer_id }) => {
            commands::marketplace::accept(offer_id).await?;
        }

        Some(Cmd::Agreements { network }) => {
            commands::marketplace::list_agreements(network).await?;
        }

        Some(Cmd::Offers { network }) => {
            commands::marketplace::list_offers(network).await?;
        }
    }

    Ok(())
}

//! # bp — BillPouch CLI
//!
//! ```text
//!  Usage:
//!    bp login  [--alias <name>] [--passphrase <p>]  Create / display your identity
//!    bp logout                      Remove your identity from this machine
//!    bp export-identity --out <path>   Export identity to a portable file
//!    bp import-identity <path> [--force]  Import identity from a portable file
//!    bp hatch  <type> [--network <id>] [--tier T2]  Start a service (bill|pouch|post)
//!    bp flock                       Show peers and network summary
//!    bp farewell <service_id>       Stop a running service
//!    bp farewell <service_id> --evict  Permanently evict a Pouch from the network
//!    bp pause <service_id> --eta <m> Pause for maintenance (ETA in minutes)
//!    bp resume <service_id>         Resume a paused service
//!    bp invite create --network <id> --invite-password <pw>  Create invite token
//!    bp invite join <blob> --invite-password <pw>            Redeem invite token
//!    bp bootstrap list              Show configured WAN bootstrap nodes
//!    bp bootstrap add <addr>        Add a bootstrap node
//!    bp bootstrap remove <addr>     Remove a bootstrap node
//!    bp leave <network>              Leave a network (requires no active services)
//!    bp relay connect <addr>        Dial a relay node for NAT traversal
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

    /// Passphrase to unlock a passphrase-protected identity.
    /// Can also be set via the `BP_PASSPHRASE` environment variable.
    #[arg(long, global = true, env = "BP_PASSPHRASE")]
    passphrase: Option<String>,

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
        /// Encrypt the identity key with a passphrase (Argon2id + AES-256-GCM).
        /// Leave empty for a plaintext key (backwards-compatible).
        #[arg(long)]
        passphrase: Option<String>,
    },

    /// Remove your identity from this machine.
    Logout,

    /// Export your identity (keypair + profile) to a portable JSON file.
    ///
    /// Use this to move your identity to another machine.  On the target
    /// machine run `bp import-identity <file>`.
    ExportIdentity {
        /// File path to write the exported identity to.
        #[arg(long, short)]
        out: String,
    },

    /// Import an identity previously exported with `bp export-identity`.
    ///
    /// Installs the keypair and profile into the local XDG data directory.
    /// Fails if an identity already exists unless `--force` is specified.
    ImportIdentity {
        /// Path to the identity export file.
        path: String,

        /// Overwrite any existing identity without prompting.
        #[arg(long)]
        force: bool,
    },

    /// Hatch (start) a new service: bill | pouch | post.
    Hatch {
        /// Service type to start: bill | pouch | post.
        service_type: String,

        /// Network ID to join (default: "public").
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        /// For pouch: storage tier to contribute
        /// (T1=10 GiB, T2=100 GiB, T3=500 GiB, T4=1 TiB, T5=5 TiB).
        #[arg(long)]
        tier: Option<String>,

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
        /// Permanently evict a Pouch: purges all local fragment storage and
        /// announces to the network that this node is going offline for good.
        /// This is irreversible.
        #[arg(long)]
        evict: bool,
    },

    /// Pause a running service for planned maintenance.
    ///
    /// Announces to the network that this node will be temporarily offline.
    /// If the service does not resume before the ETA expires, fault-score
    /// increments will be applied by the quality monitor.
    Pause {
        /// The service ID to pause.
        service_id: String,
        /// Estimated downtime in minutes.
        #[arg(long, short)]
        eta: u64,
    },

    /// Resume a previously paused service.
    ///
    /// Re-announces the node as available and triggers an immediate
    /// Proof-of-Storage challenge to clear any pending fault-score penalties.
    Resume {
        /// The service ID to resume.
        service_id: String,
    },

    /// Subscribe to a network's gossip topic (used by scripts/automation).
    #[clap(hide = true)]
    Join {
        /// The network identifier to subscribe to.
        network_id: String,
    },

    /// Leave a network.
    ///
    /// Fails if any services are still active on the network.
    /// Stop Pouches with `bp farewell <id> --evict` and other services
    /// with `bp farewell <id>` before leaving.
    Leave {
        /// The network to leave.
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

    /// Manage network invite tokens.
    ///
    /// Networks are private by default.  Use `bp invite create` to generate
    /// a token for someone and `bp invite join` to accept one.
    Invite {
        #[command(subcommand)]
        action: InviteAction,
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

#[derive(Subcommand)]
enum InviteAction {
    /// Create a signed+encrypted invite token for a network.
    ///
    /// The token contains the network's secret key — share the blob AND
    /// the invite-password with the invitee out-of-band (Signal, phone, etc.).
    Create {
        /// Network to invite the recipient into.
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        /// Optional: fingerprint of the specific invitee. Omit for an open invite.
        #[arg(long)]
        for_fingerprint: Option<String>,

        /// Token validity in hours (default: 24).
        #[arg(long, default_value_t = 24)]
        ttl: u64,

        /// Password used to encrypt the token (share out-of-band).
        #[arg(long)]
        invite_password: String,
    },

    /// Redeem an invite token: decrypt, verify, save the network key and join.
    Join {
        /// The hex blob produced by `bp invite create`.
        blob: String,

        /// Password used to decrypt the token.
        #[arg(long)]
        invite_password: String,
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
        bp_core::daemon::run_daemon(cli.passphrase).await?;
        return Ok(());
    }

    // ── CLI commands ──────────────────────────────────────────────────────
    match cli.command {
        None => {
            println!("🦤 BillPouch (bp) — P2P social distributed filesystem");
            println!("   Run `bp --help` for usage.");
        }

        Some(Cmd::Login { alias, passphrase }) => {
            commands::auth::login(alias, passphrase.as_deref()).await?;
        }

        Some(Cmd::Logout) => {
            commands::auth::logout().await?;
        }

        Some(Cmd::ExportIdentity { out }) => {
            commands::auth::export_identity(&out).await?;
        }

        Some(Cmd::ImportIdentity { path, force }) => {
            commands::auth::import_identity(&path, force).await?;
        }

        Some(Cmd::Hatch {
            service_type,
            network,
            tier,
            mount,
        }) => {
            let svc_type: bp_core::service::ServiceType = service_type.parse()?;
            let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
            if let Some(tier_str) = tier {
                let storage_tier = bp_core::storage::StorageTier::parse(&tier_str)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                metadata.insert("tier".into(), serde_json::Value::from(tier_str));
                metadata.insert(
                    "storage_bytes".into(),
                    serde_json::Value::from(storage_tier.quota_bytes()),
                );
            }
            if let Some(path) = mount {
                metadata.insert("mount_path".into(), serde_json::Value::from(path));
            }
            commands::hatch::hatch(svc_type, network, metadata, cli.passphrase.clone()).await?;
        }

        Some(Cmd::Flock) => {
            commands::flock::flock().await?;
        }

        Some(Cmd::Farewell { service_id, evict }) => {
            if evict {
                commands::farewell::farewell_evict(service_id).await?;
            } else {
                commands::farewell::farewell(service_id).await?;
            }
        }

        Some(Cmd::Pause { service_id, eta }) => {
            commands::pause::pause(service_id, eta).await?;
        }

        Some(Cmd::Resume { service_id }) => {
            commands::pause::resume(service_id).await?;
        }

        Some(Cmd::Join { network_id }) => {
            commands::join::join(network_id).await?;
        }

        Some(Cmd::Leave { network_id }) => {
            commands::leave::leave(network_id).await?;
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

        Some(Cmd::Invite { action }) => match action {
            InviteAction::Create {
                network,
                for_fingerprint,
                ttl,
                invite_password,
            } => {
                commands::invite::create(network, for_fingerprint, ttl, invite_password).await?;
            }
            InviteAction::Join {
                blob,
                invite_password,
            } => {
                commands::invite::join(blob, invite_password).await?;
            }
        },
    }

    Ok(())
}

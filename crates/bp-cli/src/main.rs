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
    #[arg(help = "(Internal) Run as background daemon — not meant to be called directly")]
    #[arg(long, hide = true)]
    daemon: bool,

    #[arg(
        help = "Passphrase to unlock a passphrase-protected identity. Can also be set via the `BP_PASSPHRASE` environment variable"
    )]
    #[arg(long, global = true, env = "BP_PASSPHRASE")]
    passphrase: Option<String>,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "Create your identity (keypair) and log in to BillPouch")]
    Login {
        #[arg(help = "Optional human-readable alias for this identity")]
        #[arg(long, short)]
        alias: Option<String>,
        #[arg(
            help = "Encrypt the identity key with a passphrase (Argon2id + ChaCha20-Poly1305). Without it the key is stored unencrypted"
        )]
        #[arg(long)]
        passphrase: Option<String>,
    },

    #[command(about = "Remove your identity from this machine")]
    Logout,

    #[command(
        about = "Export your identity (keypair + profile) to a portable JSON file",
        long_about = "Export your identity (keypair + profile) to a portable JSON file.\n\nUse this to move your identity to another machine.  On the target machine run `bp import-identity <file>`."
    )]
    ExportIdentity {
        #[arg(help = "File path to write the exported identity to")]
        #[arg(long, short)]
        out: String,
    },

    #[command(
        about = "Import an identity previously exported with `bp export-identity`",
        long_about = "Import an identity previously exported with `bp export-identity`.\n\nInstalls the keypair and profile into the local XDG data directory. Fails if an identity already exists unless `--force` is specified."
    )]
    ImportIdentity {
        #[arg(help = "Path to the identity export file")]
        path: String,

        #[arg(help = "Overwrite any existing identity without prompting")]
        #[arg(long)]
        force: bool,
    },

    #[command(about = "Hatch (start) a new service: bill | pouch | post")]
    Hatch {
        #[arg(help = "Service type to start: bill | pouch | post")]
        service_type: String,

        #[arg(help = "Network ID to join (default: \"public\")")]
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        #[arg(
            help = "For pouch: storage tier to contribute (T1=10 GiB, T2=100 GiB, T3=500 GiB, T4=1 TiB, T5=5 TiB)"
        )]
        #[arg(long)]
        tier: Option<String>,
    },

    #[command(about = "Show all known peers and network info")]
    Flock,

    #[command(
        about = "Show daemon identity and active services (compact view)",
        long_about = "Show daemon identity and active services (compact view).\n\nFor the full peer list and network summary use `bp flock`."
    )]
    Status,

    #[command(about = "Kill a running service by its service ID")]
    Farewell {
        #[arg(help = "The service ID returned by `bp hatch`")]
        service_id: String,
        #[arg(
            help = "Permanently evict a Pouch: purges all local fragment storage and announces to the network that this node is going offline for good. This is irreversible"
        )]
        #[arg(long)]
        evict: bool,
    },

    #[command(
        about = "Pause a running service for planned maintenance",
        long_about = "Pause a running service for planned maintenance.\n\nAnnounces to the network that this node will be temporarily offline. If the service does not resume before the ETA expires, fault-score increments will be applied by the quality monitor."
    )]
    Pause {
        #[arg(help = "The service ID to pause")]
        service_id: String,
        #[arg(help = "Estimated downtime in minutes")]
        #[arg(long, short)]
        eta: u64,
    },

    #[command(
        about = "Resume a previously paused service",
        long_about = "Resume a previously paused service.\n\nRe-announces the node as available and triggers an immediate Proof-of-Storage challenge to clear any pending fault-score penalties."
    )]
    Resume {
        #[arg(help = "The service ID to resume")]
        service_id: String,
    },

    #[command(about = "Subscribe to a network's gossip topic (used by scripts/automation)")]
    #[clap(hide = true)]
    Join {
        #[arg(help = "The network identifier to subscribe to")]
        network_id: String,
    },

    #[command(
        about = "Leave a network",
        long_about = "Leave a network.\n\nWithout `--force`: fails if any services are still active on the network. Use `bp farewell <id> --evict` (Pouch) or `bp farewell <id>` (Bill/Post) first.\n\nWith `--force`: automatically evicts all active services on the network, then leaves.  Equivalent to running `bp farewell --evict` on each Pouch and `bp farewell` on each Bill/Post, then leaving."
    )]
    Leave {
        #[arg(help = "The network to leave")]
        network_id: String,
        #[arg(help = "Auto-evict all blocking services before leaving")]
        #[arg(long)]
        force: bool,
    },

    #[command(about = "Store a local file in the active Pouch (RLNC erasure-coded)")]
    Put {
        #[arg(help = "Path to the file to store")]
        file: std::path::PathBuf,

        #[arg(help = "Network ID whose Pouch will store the fragments (default: \"public\")")]
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        #[arg(
            help = "Target recovery probability: probability of recovering the file at any given moment (default: 0.999 = 99.9%)"
        )]
        #[arg(long, default_value_t = 0.999)]
        ph: f64,

        #[arg(
            help = "Redundancy overhead fraction on top of the recovery threshold (default: 1.0 = one extra \"copy-equivalent\" of fragments)"
        )]
        #[arg(long, default_value_t = 1.0)]
        q_target: f64,

        #[arg(
            help = "Human-readable name for the file (shown by `bp ls`). Defaults to the file's original filename"
        )]
        #[arg(long)]
        name: Option<String>,
    },

    #[command(about = "Retrieve and decode a stored chunk by its chunk_id")]
    Get {
        #[arg(help = "The chunk_id returned by `bp put` (BLAKE3 prefix)")]
        chunk_id: String,

        #[arg(help = "Where to write the recovered file")]
        output: std::path::PathBuf,

        #[arg(help = "Network ID to search (default: \"public\")")]
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,
    },

    #[command(
        about = "List files uploaded by this node",
        long_about = "List files uploaded by this node.\n\nShows all files recorded in the local file registry (populated on `bp put`).  Use `--network` to filter to a specific network."
    )]
    Ls {
        #[arg(help = "Filter to a specific network (default: show all networks)")]
        #[arg(long, short, default_value = "")]
        network: String,
    },

    #[command(
        about = "Show storage quota and uploaded-file statistics for this node",
        long_about = "Show storage quota and uploaded-file statistics for this node.\n\nDisplays per-Pouch bid / used / available bytes, aggregate totals, and counts for all files uploaded.  Use `--network` to filter."
    )]
    Storage {
        #[arg(help = "Filter to a specific network (default: show all networks)")]
        #[arg(long, short, default_value = "")]
        network: String,
    },

    #[command(
        about = "Manage WAN bootstrap nodes (stored in bootstrap.json)",
        long_about = "Manage WAN bootstrap nodes (stored in bootstrap.json).\n\nBootstrap nodes let the daemon discover peers on the public internet where mDNS multicast is not available.  Changes take effect on the next daemon restart."
    )]
    Bootstrap {
        #[command(subcommand)]
        action: BootstrapAction,
    },

    #[command(
        about = "NAT traversal — connect to a relay node for circuit-relay v2",
        long_about = "NAT traversal — connect to a relay node for circuit-relay v2.\n\nDials the given relay peer and establishes a reservation so this node is reachable from the public internet even when behind NAT."
    )]
    Relay {
        #[command(subcommand)]
        action: RelayAction,
    },

    #[command(
        about = "Manage network invite tokens",
        long_about = "Manage network invite tokens.\n\nNetworks are private by default.  Use `bp invite create` to generate a token for someone and `bp invite join` to accept one."
    )]
    Invite {
        #[command(subcommand)]
        action: InviteAction,
    },
}

#[derive(Subcommand)]
enum BootstrapAction {
    #[command(about = "List all configured bootstrap nodes")]
    List,
    #[command(about = "Add a bootstrap node (must include /p2p/`<PeerId>`)")]
    Add {
        #[arg(
            help = "Multiaddr of the bootstrap node, e.g. `/ip4/203.0.113.1/tcp/4001/p2p/12D3KooW...`"
        )]
        addr: String,
    },
    #[command(about = "Remove a bootstrap node by its multiaddr string")]
    Remove {
        #[arg(help = "The exact multiaddr string to remove")]
        addr: String,
    },
}

#[derive(Subcommand)]
enum RelayAction {
    #[command(
        about = "Dial a relay node and establish a circuit-relay v2 reservation",
        long_about = "Dial a relay node and establish a circuit-relay v2 reservation.\n\nOnce the reservation is accepted, this node becomes reachable at `/p2p-circuit` addresses through the relay, enabling inbound connections from peers on the public internet."
    )]
    Connect {
        #[arg(
            help = "Full multiaddr of the relay node including its PeerId, e.g. `/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW...`"
        )]
        addr: String,
    },
}

#[derive(Subcommand)]
enum InviteAction {
    #[command(
        about = "Create a signed+encrypted invite token for a network",
        long_about = "Create a signed+encrypted invite token for a network.\n\nThe token contains the network's secret key — share the blob AND the invite-password with the invitee out-of-band (Signal, phone, etc.)."
    )]
    Create {
        #[arg(help = "Network to invite the recipient into")]
        #[arg(long, short, default_value = DEFAULT_NETWORK)]
        network: String,

        #[arg(help = "Optional: fingerprint of the specific invitee. Omit for an open invite")]
        #[arg(long)]
        for_fingerprint: Option<String>,

        #[arg(help = "Token validity in hours (default: 24)")]
        #[arg(long, default_value_t = 24)]
        ttl: u64,

        #[arg(help = "Password used to encrypt the token (share out-of-band)")]
        #[arg(long)]
        invite_password: String,
    },

    #[command(about = "Redeem an invite token: decrypt, verify, save the network key and join")]
    Join {
        #[arg(help = "The hex blob produced by `bp invite create`")]
        blob: String,

        #[arg(help = "Password used to decrypt the token")]
        #[arg(long)]
        invite_password: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bp_core=info,bp_cli=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    if cli.daemon {
        bp_core::daemon::run_daemon(cli.passphrase).await?;
        return Ok(());
    }

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
        }) => {
            let svc_type: bp_core::service::ServiceType = service_type.parse()?;
            let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
            if svc_type == bp_core::service::ServiceType::Pouch && tier.is_none() {
                anyhow::bail!("a pouch needs a storage tier: --tier T1|T2|T3|T4|T5");
            }
            if let Some(tier_str) = tier {
                let storage_tier = bp_core::storage::StorageTier::parse(&tier_str)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                metadata.insert("tier".into(), serde_json::Value::from(tier_str));
                metadata.insert(
                    "storage_bytes".into(),
                    serde_json::Value::from(storage_tier.quota_bytes()),
                );
            }
            commands::hatch::hatch(svc_type, network, metadata, cli.passphrase.clone()).await?;
        }

        Some(Cmd::Flock) => {
            commands::flock::flock().await?;
        }

        Some(Cmd::Status) => {
            commands::status::status().await?;
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

        Some(Cmd::Leave { network_id, force }) => {
            commands::leave::leave(network_id, force).await?;
        }

        Some(Cmd::Put {
            file,
            network,
            ph,
            q_target,
            name,
        }) => {
            commands::put::put(file, network, ph, q_target, name).await?;
        }

        Some(Cmd::Get {
            chunk_id,
            output,
            network,
        }) => {
            commands::get::get(chunk_id, network, output).await?;
        }

        Some(Cmd::Ls { network }) => {
            commands::ls::ls(network).await?;
        }

        Some(Cmd::Storage { network }) => {
            commands::storage::storage_info(network).await?;
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        super::Cli::command().debug_assert();
    }
}

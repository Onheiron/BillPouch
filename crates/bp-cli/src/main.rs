mod client;
mod commands;

use clap::{Parser, Subcommand};
use std::collections::HashMap;

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
        help = "Passphrase of your SSH key (also used to encrypt a newly generated one). Can also be set via the `BP_PASSPHRASE` environment variable"
    )]
    #[arg(long, global = true, env = "BP_PASSPHRASE")]
    passphrase: Option<String>,

    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(
        about = "Log in on this device with your SSH identity (imported or newly generated)"
    )]
    Login {
        #[arg(
            help = "Use an existing OpenSSH ed25519 private key as your identity (default: generate a new one)"
        )]
        #[arg(long)]
        ssh_key: Option<std::path::PathBuf>,
        #[arg(help = "Optional human-readable alias for this identity")]
        #[arg(long, short)]
        alias: Option<String>,
    },

    #[command(about = "Remove your identity from this machine")]
    Logout,

    #[command(about = "Hatch (start) a new service: bill | pouch | post")]
    Hatch {
        #[arg(help = "Service type to start: bill | pouch | post")]
        service_type: String,

        #[arg(help = "Network the service belongs to")]
        #[arg(long, short)]
        network: String,

        #[arg(
            help = "For pouch: storage tier to contribute (T1=10 GiB, T2=100 GiB, T3=500 GiB, T4=1 TiB, T5=5 TiB)"
        )]
        #[arg(long)]
        tier: Option<String>,

        #[arg(
            help = "For pouch: offer less than the whole tier, e.g. 512MiB or 2GiB (at least 64MiB)"
        )]
        #[arg(long, value_parser = parse_size)]
        size: Option<u64>,
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

    #[command(about = "Create a network you will be the first member of")]
    Network {
        #[command(subcommand)]
        action: NetworkAction,
    },

    #[command(
        about = "Let someone join a network: prints a token for them",
        long_about = "Let someone join a network: prints a token for them.\n\nThey give you their SSH public key (the line `bp login` prints); you get a token to send back. The token is useless to anyone else: the network key inside is encrypted for their key only.\n\nWith your own public key it prints a token for another device of yours: log in there with the same SSH key and `bp join` it; your files follow."
    )]
    Invite {
        #[arg(help = "Network to invite them into")]
        network_id: String,
        #[arg(help = "Their SSH public key, e.g. \"ssh-ed25519 AAAA... marco@laptop\"")]
        member_key: String,
    },

    #[command(
        about = "Open the local dashboard with a session signed by your SSH key",
        long_about = "Open the local web app with a session signed by your SSH key.\n\nStarts bp-api on this machine if it is not running yet. The session lasts 12 hours; bp-api only accepts sessions signed by the key of this device's user."
    )]
    Dashboard {
        #[arg(help = "Port bp-api listens on")]
        #[arg(long, default_value_t = 3000)]
        port: u16,

        #[arg(help = "Address bp-api binds to when this command starts it")]
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        #[arg(help = "Only print the address, do not open a browser")]
        #[arg(long)]
        no_open: bool,
    },

    #[command(about = "Join a network with the token you were sent")]
    Join {
        #[arg(help = "The token printed by `bp invite`")]
        token: String,
    },

    #[command(about = "List the members of a network with their reputation and weight")]
    Members {
        #[arg(help = "The network")]
        network_id: String,
    },

    #[command(
        about = "Propose or approve expelling a member",
        long_about = "Propose or approve expelling a member.\n\nThe first call proposes (needs R3, or the creator while nobody is R3); later calls by other members approve (needs R1). The member is out when the approvals hold more than 2/3 of the other members' weight and at least half of them; the network key is then rotated without them."
    )]
    Expel {
        #[arg(help = "The network")]
        network_id: String,
        #[arg(help = "Fingerprint of the member, as `bp members` shows it (SHA256:...)")]
        member: String,
    },

    #[command(
        about = "Show or vote the parameters of a network",
        long_about = "Show or vote the parameters of a network.\n\nWithout flags it prints them. With flags it votes the current parameters changed by the flags given; they change when the members voting the same values hold more than 2/3 of the weight."
    )]
    Params {
        #[arg(help = "The network")]
        network_id: String,
        #[arg(help = "Default target recovery probability of a file, below 1")]
        #[arg(long)]
        ph: Option<f64>,
        #[arg(help = "Prior chance that a newcomer stays, the baseline of trust bets")]
        #[arg(long)]
        prior: Option<f64>,
        #[arg(help = "How much of their trust a member keeps at each settled bet")]
        #[arg(long)]
        decay: Option<f64>,
        #[arg(help = "Epochs a newcomer must keep a file to win a trust bet")]
        #[arg(long)]
        horizon: Option<u64>,
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

        #[arg(help = "Network that stores the file")]
        #[arg(long, short)]
        network: String,

        #[arg(
            help = "Target recovery probability: probability of recovering the file at any given moment (default: the network's, see `bp params`)"
        )]
        #[arg(long)]
        ph: Option<f64>,

        #[arg(
            help = "Human-readable name for the file (shown by `bp ls`). Defaults to the file's original filename"
        )]
        #[arg(long)]
        name: Option<String>,
    },

    #[command(about = "Fetch, decrypt and write one of your files (or one shared with you)")]
    Get {
        #[arg(help = "The file_id printed by `bp put` or `bp ls`")]
        file_id: String,

        #[arg(help = "Where to write the recovered file")]
        output: std::path::PathBuf,

        #[arg(help = "Network that stores the file")]
        #[arg(long, short)]
        network: String,
    },

    #[command(
        about = "List your files and the files shared with you",
        long_about = "List your files and the files shared with you.\n\nThe list is your personal index: it is encrypted with your SSH key, stored in the network and synced to every device you log in with the same key.  Use `--network` to filter to a specific network."
    )]
    Ls {
        #[arg(help = "Filter to a specific network (default: show all networks)")]
        #[arg(long, short, default_value = "")]
        network: String,
    },

    #[command(
        about = "Share one of your files with another member",
        long_about = "Share one of your files with another member.\n\nThe file key is encrypted for their SSH key and delivered to them; the file then shows up in their `bp ls`. Only the owner of a file can share it."
    )]
    Share {
        #[arg(help = "The file_id of the file (see `bp ls`)")]
        file_id: String,

        #[arg(help = "Fingerprint of the member, as `bp members` shows it (SHA256:...)")]
        member: String,

        #[arg(help = "Network that stores the file")]
        #[arg(long, short)]
        network: String,
    },

    #[command(
        about = "Stop sharing one of your files with a member",
        long_about = "Stop sharing one of your files with a member.\n\nTheir fetch permission is withdrawn and every Pouch refuses them the file's fragments from now on. A copy they already downloaded stays with them."
    )]
    Unshare {
        #[arg(help = "The file_id of the file (see `bp ls`)")]
        file_id: String,

        #[arg(help = "Fingerprint of the member (SHA256:...)")]
        member: String,

        #[arg(help = "Network that stores the file")]
        #[arg(long, short)]
        network: String,
    },

    #[command(
        about = "Delete a file",
        long_about = "Delete a file.\n\nYour own files are dropped from your index and every Pouch deletes their fragments. A file shared with you is only removed from your index."
    )]
    Rm {
        #[arg(help = "The file_id of the file (see `bp ls`)")]
        file_id: String,

        #[arg(help = "Network that stores the file")]
        #[arg(long, short)]
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
        long_about = "Manage WAN bootstrap nodes (stored in bootstrap.json).\n\nBootstrap nodes let the daemon reach peers it did not learn from an invite token or from the network; `bp status` prints a node's addresses. Changes take effect on the next daemon restart."
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
enum NetworkAction {
    #[command(about = "Create a new network")]
    Create {
        #[arg(help = "Name of the network, e.g. friends")]
        name: String,
    },
}

fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: u64 = number
        .parse()
        .map_err(|_| format!("'{text}' is not a size like 512MiB or 2GiB"))?;
    let shift = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 0,
        "k" | "kib" => 10,
        "m" | "mib" => 20,
        "g" | "gib" => 30,
        "t" | "tib" => 40,
        _ => return Err(format!("'{text}' is not a size like 512MiB or 2GiB")),
    };
    number
        .checked_mul(1 << shift)
        .ok_or_else(|| format!("'{text}' is too large"))
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

    let mut cli = Cli::parse();
    cli.passphrase = cli.passphrase.filter(|p| !p.is_empty());

    client::set_passphrase(cli.passphrase.clone());

    if cli.daemon {
        bp_core::daemon::run_daemon(cli.passphrase).await?;
        return Ok(());
    }

    match cli.command {
        None => {
            println!("🦤 BillPouch (bp) — P2P social distributed filesystem");
            println!("   Run `bp --help` for usage.");
        }

        Some(Cmd::Login { ssh_key, alias }) => {
            commands::auth::login(ssh_key.as_deref(), alias, cli.passphrase.as_deref()).await?;
        }

        Some(Cmd::Logout) => {
            commands::auth::logout().await?;
        }

        Some(Cmd::Hatch {
            service_type,
            network,
            tier,
            size,
        }) => {
            let svc_type: bp_core::service::ServiceType = service_type.parse()?;
            let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
            if let Some(tier) = tier {
                metadata.insert("tier".into(), serde_json::Value::from(tier));
            }
            if let Some(size) = size {
                metadata.insert("size_bytes".into(), serde_json::Value::from(size));
            }
            commands::hatch::hatch(svc_type, network, metadata).await?;
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

        Some(Cmd::Network {
            action: NetworkAction::Create { name },
        }) => {
            commands::network::create(name).await?;
        }

        Some(Cmd::Invite {
            network_id,
            member_key,
        }) => {
            commands::network::invite(network_id, member_key).await?;
        }

        Some(Cmd::Dashboard {
            port,
            host,
            no_open,
        }) => {
            commands::dashboard::dashboard(port, &host, no_open, cli.passphrase.as_deref()).await?;
        }

        Some(Cmd::Join { token }) => {
            commands::network::join(token).await?;
        }

        Some(Cmd::Members { network_id }) => {
            commands::network::members(network_id).await?;
        }

        Some(Cmd::Expel { network_id, member }) => {
            commands::network::expel(network_id, member).await?;
        }

        Some(Cmd::Params {
            network_id,
            ph,
            prior,
            decay,
            horizon,
        }) => {
            commands::network::params(network_id, ph, prior, decay, horizon).await?;
        }

        Some(Cmd::Leave { network_id, force }) => {
            commands::leave::leave(network_id, force).await?;
        }

        Some(Cmd::Put {
            file,
            network,
            ph,
            name,
        }) => {
            commands::put::put(file, network, ph, name).await?;
        }

        Some(Cmd::Get {
            file_id,
            output,
            network,
        }) => {
            commands::get::get(file_id, network, output).await?;
        }

        Some(Cmd::Ls { network }) => {
            commands::ls::ls(network).await?;
        }

        Some(Cmd::Share {
            file_id,
            member,
            network,
        }) => {
            commands::ls::share(network, file_id, member).await?;
        }

        Some(Cmd::Unshare {
            file_id,
            member,
            network,
        }) => {
            commands::ls::unshare(network, file_id, member).await?;
        }

        Some(Cmd::Rm { file_id, network }) => {
            commands::ls::rm(network, file_id).await?;
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
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn sizes_parse_with_binary_units() {
        assert_eq!(super::parse_size("512MiB"), Ok(512 << 20));
        assert_eq!(super::parse_size("2G"), Ok(2 << 30));
        assert_eq!(super::parse_size("4096"), Ok(4096));
        assert!(super::parse_size("big").is_err());
        assert!(super::parse_size("5XB").is_err());
    }

    #[test]
    fn cli_definition_is_valid() {
        super::Cli::command().debug_assert();
    }
}

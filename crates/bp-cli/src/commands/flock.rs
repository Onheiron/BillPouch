use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, FlockData};

pub async fn flock() -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client.request(ControlRequest::Flock).await?;

    match data {
        None => println!("(no data)"),
        Some(v) => {
            let flock: FlockData = serde_json::from_value(v)?;
            print_flock(&flock);
        }
    }
    Ok(())
}

fn print_flock(flock: &FlockData) {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║             🦤  BillPouch — Flock View               ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
    println!("📋 Local Services  ({})", flock.local_services.len());
    println!("─────────────────────────────────────────────────────");
    if flock.local_services.is_empty() {
        println!("   (none)");
    } else {
        for svc in &flock.local_services {
            let tier_info = if svc.service_type == bp_core::service::ServiceType::Pouch {
                match svc.metadata.get("tier").and_then(|v| v.as_str()) {
                    Some(t) => format!("  tier: {t}"),
                    None => String::new(),
                }
            } else {
                String::new()
            };
            println!(
                "   [{:>6}]  {}  │  net: {}  │  status: {}{}",
                svc.service_type,
                &svc.id[..8],
                svc.network_id,
                svc.status,
                tier_info,
            );
        }
    }

    println!();
    println!("🌐 Joined Networks  ({})", flock.networks.len());
    println!("─────────────────────────────────────────────────────");
    if flock.networks.is_empty() {
        println!("   (none — use `bp invite join <token>` to join a network)");
    } else {
        for net in &flock.networks {
            let peers_in_net = flock
                .known_peers
                .iter()
                .filter(|n| &n.network_id == net)
                .count();
            println!("   {}  │  {} known peer(s)", net, peers_in_net);
        }
    }

    println!();
    println!("🐦 Known Peers  ({})", flock.peer_count);
    println!("─────────────────────────────────────────────────────");
    if flock.known_peers.is_empty() {
        println!("   (none yet)");
    } else {
        for peer in &flock.known_peers {
            let alias = peer.user_alias.as_deref().unwrap_or("—");
            println!(
                "   {} │ {} │ {:>6} │ net: {}",
                &peer.peer_id[..12],
                peer.user_fingerprint,
                peer.service_type.to_string(),
                peer.network_id,
            );
            if alias != "—" {
                println!("   alias: {}", alias);
            }
        }
    }
    println!();
}

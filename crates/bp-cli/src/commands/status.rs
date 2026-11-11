use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, StatusData};
use std::io::Write as _;

pub async fn status() -> anyhow::Result<()> {
    {
        let mut client = ControlClient::connect().await?;

        let spinner = tokio::spawn(async {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i: usize = 0;
            loop {
                print!(
                    "\r{}  Refreshing network state...",
                    frames[i % frames.len()]
                );
                std::io::stdout().flush().ok();
                i += 1;
                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
            }
        });

        let _ = client.request(ControlRequest::AnnounceNow).await;

        spinner.abort();
        print!("\r\x1b[K");
        let _ = std::io::stdout().flush();
    }

    let mut client = ControlClient::connect().await?;
    let data = client.request(ControlRequest::Status).await?;

    match data {
        None => println!("(no data from daemon)"),
        Some(v) => {
            let s: StatusData = serde_json::from_value(v)?;
            print_status(&s);
        }
    }
    Ok(())
}

fn print_status(s: &StatusData) {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║           🦤  BillPouch — Daemon Status              ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    println!("🔑 Identity");
    println!("   fingerprint  : {}", s.fingerprint);
    if let Some(alias) = &s.alias {
        println!("   alias         : {alias}");
    }
    println!(
        "   peer_id       : {}…",
        if s.peer_id.len() > 24 {
            &s.peer_id[..24]
        } else {
            &s.peer_id
        }
    );
    println!("   daemon version : {}", s.version);
    println!();

    println!("🔌 Services  ({})", s.local_services.len());
    println!("─────────────────────────────────────────────────────");
    if s.local_services.is_empty() {
        println!("   (none — run `bp hatch <bill|pouch|post> --network <id>`)");
    } else {
        for svc in &s.local_services {
            let tier_info = match svc.service_type {
                bp_core::service::ServiceType::Pouch => {
                    match svc.metadata.get("tier").and_then(|v| v.as_str()) {
                        Some(t) => format!("  tier: {t}"),
                        None => String::new(),
                    }
                }
                _ => match bp_core::dlna::port(svc) {
                    Some(port) => format!("  📺 DLNA :{port}"),
                    None => String::new(),
                },
            };
            println!(
                "   [{:>6}]  {}  │  net: {}  │  {}{}",
                svc.service_type,
                &svc.id[..8],
                svc.network_id,
                svc.status,
                tier_info,
            );
        }
    }
    println!();

    println!("⭐ Reputation");
    println!("   tier   : {}", s.reputation_tier);
    println!();

    for ps in &s.pouch_stats {
        super::storage::print_pouch(ps);
        println!();
    }

    if s.networks.is_empty() {
        println!("🌐 Networks  (none — use `bp invite join <token>`)");
    } else {
        println!("🌐 Networks  {}", s.networks.join(", "));
    }
    println!("📡 Known peers  {}", s.known_peers);
    if s.addresses.is_empty() {
        println!("📍 Addresses    (not listening yet)");
    } else {
        println!(
            "📍 Addresses    (give one to `bp bootstrap add` on a node that cannot find this one)"
        );
        for addr in &s.addresses {
            println!("   {addr}");
        }
    }

    if let Some(qos) = &s.network_qos {
        println!();
        println!("📶 Network QoS  ({} observed peers)", qos.observed_peers);
        println!("   avg availability : {:.3}", qos.avg_availability);
        let mut tiers: Vec<(&String, &usize)> = qos.tier_counts.iter().collect();
        tiers.sort_by_key(|(k, _)| k.as_str());
        for (tier, count) in tiers {
            println!("   {tier:>4} : {count} peer(s)");
        }
    }
}

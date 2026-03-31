//! `bp status` — compact view of daemon identity and local services.

use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, StatusData};
use std::io::Write as _;

/// Query the daemon for identity + service status and pretty-print a compact
/// summary.  Triggers a gossip re-announce before reading so that `known_peers`
/// reflects the live network state.
pub async fn status() -> anyhow::Result<()> {
    // ── Phase 1: gossip refresh with spinner ─────────────────────────────
    {
        let mut client = ControlClient::connect().await?;

        // Spinner task — runs until the AnnounceNow response arrives.
        let spinner = tokio::spawn(async {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i: usize = 0;
            loop {
                let _ = print!("\r{}  Refreshing network state...", frames[i % frames.len()]);
                let _ = std::io::stdout().flush();
                i += 1;
                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
            }
        });

        // Ask the daemon to re-announce all services and wait 2 s.
        let _ = client.request(ControlRequest::AnnounceNow).await;

        spinner.abort();
        // Erase the spinner line.
        print!("\r\x1b[K");
        let _ = std::io::stdout().flush();
    }

    // ── Phase 2: fetch and display status ────────────────────────────────
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

    // ── Identity ──────────────────────────────────────────────────────────
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

    // ── Services ──────────────────────────────────────────────────────────
    println!("🔌 Services  ({})", s.local_services.len());
    println!("─────────────────────────────────────────────────────");
    if s.local_services.is_empty() {
        println!("   (none — run `bp hatch <bill|pouch|post> --network <id>`)");
    } else {
        for svc in &s.local_services {
            let tier_info = if svc.service_type == bp_core::service::ServiceType::Pouch {
                match svc.metadata.get("tier").and_then(|v| v.as_str()) {
                    Some(t) => format!("  tier: {t}"),
                    None => String::new(),
                }
            } else {
                String::new()
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

    // ── Reputation ────────────────────────────────────────────────────────
    println!("⭐ Reputation");
    println!(
        "   tier   : {}   score: {}",
        s.reputation_tier, s.reputation_score
    );
    println!();

    // ── Pouch storage stats ───────────────────────────────────────────────
    if !s.pouch_stats.is_empty() {
        println!("💾 Storage (Pouches)");
        println!("─────────────────────────────────────────────────────");
        for ps in &s.pouch_stats {
            let tier_label = ps.storage_tier.as_deref().unwrap_or("(no tier)");
            println!(
                "   [{}]  net: {}  tier: {}",
                &ps.service_id[..8],
                ps.network_id,
                tier_label,
            );
            println!(
                "      bid: {}   used: {}   avail: {}",
                fmt_bytes(ps.storage_bid_bytes),
                fmt_bytes(ps.storage_used_bytes),
                fmt_bytes(ps.available_bytes),
            );
        }
        println!();
    }

    // ── Networks & peers ──────────────────────────────────────────────────
    if s.networks.is_empty() {
        println!("🌐 Networks  (none — use `bp invite join <token>`)");
    } else {
        println!("🌐 Networks  {}", s.networks.join(", "));
    }
    println!("📡 Known peers  {}", s.known_peers);

    // ── Network QoS ───────────────────────────────────────────────────────
    if let Some(qos) = &s.network_qos {
        println!();
        println!("📶 Network QoS  ({} observed peers)", qos.observed_peers);
        println!("   avg stability : {:.3}", qos.avg_stability);
        let mut tiers: Vec<(&String, &usize)> = qos.tier_counts.iter().collect();
        tiers.sort_by_key(|(k, _)| k.as_str());
        for (tier, count) in tiers {
            println!("   {tier:>4} : {count} peer(s)");
        }
    }
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1_073_741_824 {
        format!("{:.1} GiB", b as f64 / 1_073_741_824.0)
    } else if b >= 1_048_576 {
        format!("{:.1} MiB", b as f64 / 1_048_576.0)
    } else if b >= 1024 {
        format!("{:.1} KiB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

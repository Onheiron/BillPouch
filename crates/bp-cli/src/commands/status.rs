//! `bp status` — compact view of daemon identity and local services.

use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, StatusData};

/// Query the daemon for identity + service status and pretty-print a compact
/// summary.  Unlike `bp flock`, this does **not** show the full peer list —
/// use `bp flock` when you need peer details.
pub async fn status() -> anyhow::Result<()> {
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
    println!(
        "   fingerprint  : {}",
        s.fingerprint
    );
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

    // ── Networks & peers ──────────────────────────────────────────────────
    if s.networks.is_empty() {
        println!("🌐 Networks  (none — use `bp invite join <token>`)");
    } else {
        println!("🌐 Networks  {}", s.networks.join(", "));
    }
    println!("📡 Known peers  {}", s.known_peers);
}

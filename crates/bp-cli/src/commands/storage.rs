//! `bp storage [--network <id>]` — show storage info for this node.

use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, StorageInfoData};

/// Query the daemon for storage info and print a human-readable summary.
pub async fn storage_info(network_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::StorageInfo {
            network_id: network_id.clone(),
        })
        .await?;

    match data {
        None => println!("(no data from daemon)"),
        Some(v) => {
            let d: StorageInfoData = serde_json::from_value(v)?;
            print_storage(&d);
        }
    }
    Ok(())
}

fn print_storage(d: &StorageInfoData) {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║              💾  BillPouch — Storage Info            ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    // ── Per-Pouch breakdown ───────────────────────────────────────────────
    if d.pouches.is_empty() {
        println!("💼 Pouches  (none — run `bp hatch pouch --network <id>`)");
    } else {
        println!("💼 Pouches  ({})", d.pouches.len());
        println!("─────────────────────────────────────────────────────");
        for ps in &d.pouches {
            let tier_label = ps.storage_tier.as_deref().unwrap_or("(no tier)");
            println!(
                "   [{}]  net: {}  tier: {}",
                &ps.service_id[..8],
                ps.network_id,
                tier_label,
            );
            let pct = if ps.storage_bid_bytes > 0 {
                ps.storage_used_bytes as f64 / ps.storage_bid_bytes as f64 * 100.0
            } else {
                0.0
            };
            println!(
                "      bid: {:>10}   used: {:>10} ({:4.1}%)   avail: {:>10}",
                fmt_bytes(ps.storage_bid_bytes),
                fmt_bytes(ps.storage_used_bytes),
                pct,
                fmt_bytes(ps.available_bytes),
            );
        }
        println!();
    }

    // ── Aggregate ─────────────────────────────────────────────────────────
    println!("📊 Aggregate");
    println!("─────────────────────────────────────────────────────");
    println!("   Total bid       : {}", fmt_bytes(d.total_bid_bytes));
    println!("   Total used      : {}", fmt_bytes(d.total_used_bytes));
    println!("   Total available : {}", fmt_bytes(d.total_available_bytes));
    println!();

    // ── Uploaded files ────────────────────────────────────────────────────
    println!("📤 Uploaded Files");
    println!("─────────────────────────────────────────────────────");
    println!("   Files   : {}", d.total_files_uploaded);
    println!("   Bytes   : {}", fmt_bytes(d.total_uploaded_bytes));
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

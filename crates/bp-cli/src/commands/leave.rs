//! `bp leave <network>` — leave a network safely.

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Leave a network.
///
/// Without `force`: fails with a clear error if any services are active.
/// With `force`: auto-evicts all blocking services, then leaves.
pub async fn leave(network_id: String, force: bool) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::Leave { network_id, force })
        .await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("🚪 {}", msg);
        }
        // Blocked (force=false, active services present).
        if let Some(arr) = v.get("blocking_services").and_then(|a| a.as_array()) {
            if !arr.is_empty() {
                println!("   Blocking services:");
                for svc in arr {
                    let id = svc.get("id").and_then(|s| s.as_str()).unwrap_or("?");
                    let stype = svc.get("type").and_then(|s| s.as_str()).unwrap_or("?");
                    let hint = svc.get("hint").and_then(|s| s.as_str()).unwrap_or("");
                    println!("   • {} ({})  → {}", id, stype, hint);
                }
                println!();
                println!(
                    "   Tip: use `bp leave {} --force` to auto-evict and leave.",
                    v.get("network_id")
                        .and_then(|n| n.as_str())
                        .unwrap_or("<network>")
                );
            }
        }
        // Auto-evicted services (force=true).
        if let Some(arr) = v.get("services_auto_evicted").and_then(|a| a.as_array()) {
            if !arr.is_empty() {
                println!("   Auto-evicted services:");
                for svc in arr {
                    let sid = svc
                        .get("service_id")
                        .and_then(|s| s.as_str())
                        .unwrap_or("?");
                    let stype = svc
                        .get("service_type")
                        .and_then(|s| s.as_str())
                        .unwrap_or("?");
                    let evicted = svc
                        .get("evicted")
                        .and_then(|e| e.as_bool())
                        .unwrap_or(false);
                    let label = if evicted { "evicted" } else { "stopped" };
                    println!("   • {} ({}) — {}", sid, stype, label);
                }
            }
        }
    }
    Ok(())
}

//! `bp leave <network>` — leave a network safely.

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Leave a network.
///
/// Fails with a clear error if any services (Bill, Pouch, Post) are still
/// active on the network. Stop or evict them first:
/// - Pouch: `bp farewell <id> --evict`
/// - Bill / Post: `bp farewell <id>`
pub async fn leave(network_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client.request(ControlRequest::Leave { network_id }).await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("🚪 {}", msg);
        }
        if let Some(arr) = v.get("blocking_services").and_then(|a| a.as_array()) {
            if !arr.is_empty() {
                println!("   Blocking services:");
                for svc in arr {
                    let id = svc.get("id").and_then(|s| s.as_str()).unwrap_or("?");
                    let stype = svc.get("type").and_then(|s| s.as_str()).unwrap_or("?");
                    let hint = svc.get("hint").and_then(|s| s.as_str()).unwrap_or("");
                    println!("   • {} ({})  → {}", id, stype, hint);
                }
            }
        }
    }
    Ok(())
}

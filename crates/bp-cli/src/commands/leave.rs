use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

pub async fn leave(network_id: String, force: bool) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::Leave { network_id, force })
        .await?;

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
        if v.get("blocked").and_then(|b| b.as_bool()) == Some(true) {
            println!();
            println!(
                "   Tip: use `bp leave {} --force` to leave anyway.",
                v.get("network_id")
                    .and_then(|n| n.as_str())
                    .unwrap_or("<network>")
            );
        }
        if let Some(arr) = v.get("own_files").and_then(|a| a.as_array()) {
            if !arr.is_empty() {
                println!("   Your files on this network (deleted by --force):");
                for f in arr {
                    let name = f.get("file_name").and_then(|s| s.as_str()).unwrap_or("?");
                    let hint = f.get("hint").and_then(|s| s.as_str()).unwrap_or("");
                    println!("   • {}  → save it with: {}", name, hint);
                }
            }
        }
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

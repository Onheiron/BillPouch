//! `bp farewell <service_id>` and `bp farewell <service_id> --evict`.

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Send a `Farewell` request to the daemon to stop the service identified by
/// `service_id` (the UUID returned by `bp hatch`).
///
/// Prints a confirmation message if the daemon responds with `Ok`.
pub async fn farewell(service_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::Farewell { service_id })
        .await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("👋 {}", msg);
        }
        if let Some(svc_id) = v.get("service_id").and_then(|i| i.as_str()) {
            println!("   Service {} has been stopped.", svc_id);
        }
    }
    Ok(())
}

/// Permanently evict a Pouch: announces to the network, purges local storage,
/// and removes the service from the daemon registry.
///
/// This is irreversible. The network will detect missing fragments via
/// Proof-of-Storage challenges and trigger preventive rebalancing.
pub async fn farewell_evict(service_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::FarewellEvict { service_id })
        .await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("🗑  {}", msg);
        }
        if let Some(chunks) = v.get("chunks_removed").and_then(|c| c.as_u64()) {
            let frags = v
                .get("fragments_removed")
                .and_then(|f| f.as_u64())
                .unwrap_or(0);
            let bytes = v
                .get("bytes_freed")
                .and_then(|b| b.as_u64())
                .unwrap_or(0);
            println!(
                "   Removed {} chunk(s), {} fragment(s), {} bytes.",
                chunks, frags, bytes
            );
        }
    }
    Ok(())
}

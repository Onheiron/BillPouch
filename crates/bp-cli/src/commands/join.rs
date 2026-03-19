//! `bp join <network_id>` — join an existing BillPouch network.

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Subscribe to the gossipsub topic for `network_id`.
///
/// Instructs the daemon to call `JoinNetwork` on the swarm, which subscribes
/// to `billpouch/v1/{network_id}/nodes`.  Peers on that network will begin
/// appearing in `bp flock` as their [`NodeInfo`] announcements arrive.
///
/// [`NodeInfo`]: bp_core::network::state::NodeInfo
pub async fn join(network_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client.request(ControlRequest::Join { network_id }).await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("🌐 {}", msg);
        }
        if let Some(net) = v.get("network_id").and_then(|n| n.as_str()) {
            println!("   Subscribed to gossip topic for network '{}'.", net);
            println!("   Peers in this network will appear in `bp flock` shortly.");
        }
    }
    Ok(())
}

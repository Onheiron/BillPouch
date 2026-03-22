//! `bp pause <service_id> --eta <minutes>` and `bp resume <service_id>`.

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Pause a running service for planned maintenance.
///
/// Announces to the network that this Pouch is temporarily unavailable
/// and will be back within `eta_minutes`.
pub async fn pause(service_id: String, eta_minutes: u64) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::Pause {
            service_id,
            eta_minutes,
        })
        .await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("⏸  {}", msg);
        }
    }
    Ok(())
}

/// Resume a previously paused service.
///
/// Re-announces this Pouch as available and triggers an immediate PoS
/// challenge so reputation is updated without penalty.
pub async fn resume(service_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::Resume { service_id })
        .await?;

    if let Some(v) = data {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            println!("▶  {}", msg);
        }
    }
    Ok(())
}

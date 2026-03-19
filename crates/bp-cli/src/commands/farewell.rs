//! `bp farewell <service_id>` — stop a running service.

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

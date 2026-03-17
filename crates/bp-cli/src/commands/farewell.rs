//! `bp farewell <service_id>` — kill a running service.

use bp_core::control::protocol::ControlRequest;
use crate::client::ControlClient;

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

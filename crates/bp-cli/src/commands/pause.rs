use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

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

use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, DevicesData, InviteData};

pub async fn add(network_id: String, key: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::AddDevice { network_id, key })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let added: InviteData = serde_json::from_value(data)?;
    eprintln!(
        "📱 Key {} is yours on '{}' and opens your files. On that device run: bp join <token>",
        added.member, added.network_id
    );
    println!("{}", added.token);
    Ok(())
}

pub async fn revoke(network_id: String, key: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::RevokeDevice { network_id, key })
        .await?;
    let message = data
        .as_ref()
        .and_then(|d| d.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("Revoked");
    println!(
        "🔒 {message}. Its devices are out and your family key changed for the files to come."
    );
    Ok(())
}

pub async fn list(network_id: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::Devices { network_id })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: DevicesData = serde_json::from_value(data)?;
    println!(
        "🔑 {} on '{}' — family key {}",
        d.user,
        d.network_id,
        if d.family { "set" } else { "not set yet" }
    );
    for key in &d.keys {
        let state = match (key.revoked, key.this_device) {
            (true, _) => "revoked",
            (false, true) => "this device",
            (false, false) => "active",
        };
        let name = if key.name.is_empty() {
            String::new()
        } else {
            format!(" ({})", key.name)
        };
        println!("   {}{name}  [{state}]", key.fingerprint);
        for device in &key.devices {
            println!("      └ {device}");
        }
    }
    Ok(())
}

use crate::client::ControlClient;
use anyhow::bail;
use bp_core::{
    control::protocol::{ControlRequest, DevicesData, InviteData},
    identity::{new_ssh_key, write_private},
};

async fn added(network_id: String, key: String, recovery: bool) -> anyhow::Result<InviteData> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::AddDevice {
            network_id,
            key,
            recovery,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    Ok(serde_json::from_value(data)?)
}

pub async fn add(network_id: String, key: String) -> anyhow::Result<()> {
    let added = added(network_id, key, false).await?;
    eprintln!(
        "📱 Key {} is yours on '{}' and opens your files. On that device run: bp join <token>",
        added.member, added.network_id
    );
    println!("{}", added.token);
    Ok(())
}

pub async fn recovery(
    network_id: String,
    out: std::path::PathBuf,
    passphrase: Option<&str>,
) -> anyhow::Result<()> {
    if out.exists() {
        bail!("{} already exists", out.display());
    }
    let (private, public) = new_ssh_key("recovery", passphrase)?;
    let added = added(network_id, public, true).await?;
    write_private(&out, private.as_bytes())?;
    eprintln!(
        "🗝  Recovery key {} on '{}' written to {}. Keep it and this token offline; with them, `bp login --ssh-key {}` and `bp join <token>` on any machine:",
        added.member,
        added.network_id,
        out.display(),
        out.display()
    );
    println!("{}", added.token);
    Ok(())
}

async fn report(req: ControlRequest, icon: &str) -> anyhow::Result<()> {
    let data = ControlClient::connect().await?.request(req).await?;
    let message = data
        .as_ref()
        .and_then(|d| d.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("Done");
    println!("{icon} {message}");
    Ok(())
}

pub async fn revoke(network_id: String, key: String) -> anyhow::Result<()> {
    report(ControlRequest::RevokeDevice { network_id, key }, "🔒").await
}

pub async fn cancel(network_id: String, key: String) -> anyhow::Result<()> {
    report(ControlRequest::CancelRevocation { network_id, key }, "↩️").await
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
        let mut state = match (key.revoked, key.this_device) {
            (true, _) => "revoked".to_string(),
            (false, true) => "this device".to_string(),
            (false, false) => "active".to_string(),
        };
        if key.recovery {
            state.push_str(", recovery");
        }
        if let Some(due) = key.revocation_due {
            let at = chrono::DateTime::<chrono::Utc>::from_timestamp(due as i64, 0)
                .map_or_else(|| due.to_string(), |t| t.to_rfc3339());
            state.push_str(&format!(", revoked at {at} unless cancelled"));
        }
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

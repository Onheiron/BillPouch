use crate::{client::ControlClient, commands::hatch::ensure_daemon};
use bp_core::control::protocol::{ControlRequest, InviteData};

pub async fn create(name: String, passphrase: Option<String>) -> anyhow::Result<()> {
    ensure_daemon(passphrase).await?;
    ControlClient::connect()
        .await?
        .request(ControlRequest::CreateNetwork { name: name.clone() })
        .await?;
    println!("🌐 Created network '{name}'. You are its first member.");
    println!("   Invite someone with: bp invite {name} \"<their ssh public key>\"");
    Ok(())
}

pub async fn invite(
    network_id: String,
    member_key: String,
    passphrase: Option<String>,
) -> anyhow::Result<()> {
    ensure_daemon(passphrase).await?;
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::Invite {
            network_id,
            member_key,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let invite: InviteData = serde_json::from_value(data)?;
    eprintln!(
        "✅ {} can now join '{}'. Send them this token; they run: bp join <token>",
        invite.member, invite.network_id
    );
    println!("{}", invite.token);
    Ok(())
}

pub async fn join(token: String, passphrase: Option<String>) -> anyhow::Result<()> {
    ensure_daemon(passphrase).await?;
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::JoinNetwork { token })
        .await?;
    let name = data
        .as_ref()
        .and_then(|d| d.get("network_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    println!("🌐 Joined network '{name}'.");
    Ok(())
}

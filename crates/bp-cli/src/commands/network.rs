use crate::{client::ControlClient, commands::hatch::ensure_daemon};
use bp_core::{
    chain::ExpelOutcome,
    control::protocol::{ControlRequest, ExpelData, InviteData, MembersData},
};

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
    if invite.for_device {
        eprintln!(
            "📱 Token for another device of yours on '{}'. Log in there with the same SSH key (bp login --ssh-key …), then run: bp join <token>",
            invite.network_id
        );
        println!("{}", invite.token);
        return Ok(());
    }
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

pub async fn members(network_id: String, passphrase: Option<String>) -> anyhow::Result<()> {
    ensure_daemon(passphrase).await?;
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::Members { network_id })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let m: MembersData = serde_json::from_value(data)?;
    println!(
        "🌐 {} — ledger height {}, epoch {}, total weight {}",
        m.network_id, m.height, m.epoch, m.total_weight
    );
    for member in &m.members {
        let expel = if member.expel_approvals > 0 {
            format!("   ⚠ expulsion: {} approval(s)", member.expel_approvals)
        } else {
            String::new()
        };
        println!(
            "   {}  {:<10} weight {}  hosts {:.1} MiB  stores {:.1} MiB  devices {}{}",
            member.fingerprint,
            member.tier,
            member.weight,
            member.credit_bytes as f64 / 1_048_576.0,
            member.used_bytes as f64 / 1_048_576.0,
            member.devices,
            expel
        );
    }
    for fingerprint in &m.expelled {
        println!("   {fingerprint}  expelled");
    }
    Ok(())
}

pub async fn expel(
    network_id: String,
    member: String,
    passphrase: Option<String>,
) -> anyhow::Result<()> {
    ensure_daemon(passphrase).await?;
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::Expel { network_id, member })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let e: ExpelData = serde_json::from_value(data)?;
    match e.outcome {
        ExpelOutcome::Expelled => println!(
            "🚫 {} is expelled from '{}'. The network key is being rotated.",
            e.member, e.network_id
        ),
        ExpelOutcome::Proposed => println!(
            "🗳  Proposed expelling {} from '{}'. Other members approve with: bp expel {} {}",
            e.member, e.network_id, e.network_id, e.member
        ),
        ExpelOutcome::Approved => println!(
            "🗳  Approved expelling {} from '{}'; more approvals are needed.",
            e.member, e.network_id
        ),
    }
    Ok(())
}

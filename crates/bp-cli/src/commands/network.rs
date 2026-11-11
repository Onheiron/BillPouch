use crate::client::ControlClient;
use bp_core::{
    chain::ExpelOutcome,
    control::protocol::{ChatData, ControlRequest, ExpelData, InviteData, MembersData, ParamsData},
    ledger::Params,
};

pub async fn create(name: String) -> anyhow::Result<()> {
    ControlClient::connect()
        .await?
        .request(ControlRequest::CreateNetwork { name: name.clone() })
        .await?;
    println!("🌐 Created network '{name}'. You are its first member.");
    println!("   Invite someone with: bp invite {name} \"<their ssh public key>\"");
    Ok(())
}

pub async fn invite(network_id: String, member_key: String) -> anyhow::Result<()> {
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

pub async fn join(token: String) -> anyhow::Result<()> {
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

pub async fn members(network_id: String) -> anyhow::Result<()> {
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
        let you = if member.fingerprint == m.me {
            "  ← you"
        } else {
            ""
        };
        println!(
            "   {:<12} {}  {:<10} weight {}  hosts {:.1} MiB  stores {:.1} MiB  devices {}  trust {:+.2} ({:.2} locked: {} open bet(s), {} invitation(s)){}{}",
            member.name,
            member.fingerprint,
            member.tier,
            member.weight,
            member.credit_bytes as f64 / 1_048_576.0,
            member.used_bytes as f64 / 1_048_576.0,
            member.devices,
            member.trust,
            member.locked_trust,
            member.open_bets,
            member.sponsoring,
            you,
            expel
        );
        let badges: Vec<&str> = member
            .achievements
            .iter()
            .filter(|a| a.earned)
            .map(|a| a.name.as_str())
            .collect();
        println!(
            "   {:<12} level {} {} · {} points · {}",
            "",
            member.level.number,
            member.level.name,
            member.level.points,
            if badges.is_empty() {
                "no badges yet".to_string()
            } else {
                badges.join(", ")
            }
        );
    }
    for fingerprint in &m.expelled {
        println!("   {fingerprint}  expelled");
    }
    Ok(())
}

pub async fn chat(network_id: String, message: String, last: usize) -> anyhow::Result<()> {
    let request = if message.trim().is_empty() {
        ControlRequest::Chat {
            network_id,
            known_revision: None,
        }
    } else {
        ControlRequest::ChatSend {
            network_id,
            text: message,
        }
    };
    let data = ControlClient::connect()
        .await?
        .request(request)
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let chat: ChatData = serde_json::from_value(data)?;
    let lines = chat.messages.unwrap_or_default();
    if lines.is_empty() {
        println!("💬 {} — nobody has written yet", chat.network_id);
    }
    for line in lines.iter().skip(lines.len().saturating_sub(last)) {
        let when = chrono::DateTime::from_timestamp(line.at as i64, 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%d %b %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        let who = if line.mine { "you" } else { line.name.as_str() };
        println!("{when}  {who}: {}", line.text);
    }
    Ok(())
}

fn ppm(value: f64) -> anyhow::Result<u32> {
    if !(0.0..=1.0).contains(&value) {
        anyhow::bail!("{value} is not between 0 and 1");
    }
    Ok((value * 1_000_000.0).round() as u32)
}

pub async fn params(
    network_id: String,
    ph: Option<f64>,
    decay: Option<f64>,
    horizon: Option<u64>,
    grace: Option<u64>,
) -> anyhow::Result<()> {
    let fetch = |propose: Option<Params>| {
        let network_id = network_id.clone();
        async move {
            let data = ControlClient::connect()
                .await?
                .request(ControlRequest::Params {
                    network_id,
                    propose,
                })
                .await?
                .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
            anyhow::Ok(serde_json::from_value::<ParamsData>(data)?)
        }
    };
    let mut data = fetch(None).await?;
    if ph.is_some() || decay.is_some() || horizon.is_some() || grace.is_some() {
        let mut wanted = data.params;
        if let Some(v) = ph {
            wanted.ph_ppm = ppm(v)?;
        }
        if let Some(v) = decay {
            wanted.decay_ppm = ppm(v)?;
        }
        if let Some(v) = horizon {
            wanted.horizon_epochs = v;
        }
        if let Some(v) = grace {
            wanted.grace_epochs = v;
        }
        data = fetch(Some(wanted)).await?;
    }
    let show = |p: &Params| {
        format!(
            "ph {:.6}  decay {:.3}  horizon {} epoch(s)  grace {} epoch(s)",
            p.ph(),
            f64::from(p.decay_ppm) / 1e6,
            p.horizon_epochs,
            p.grace_epochs
        )
    };
    println!("🌐 {}: {}", data.network_id, show(&data.params));
    if let Some(voted) = data.voted {
        println!("   your vote, waiting for a quorum: {}", show(&voted));
    }
    Ok(())
}

pub async fn expel(network_id: String, member: String) -> anyhow::Result<()> {
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

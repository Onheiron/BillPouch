use anyhow::{Context, Result};
use bp_core::control::protocol::{ControlRequest, ControlResponse, InviteData};

use crate::client::ControlClient;

pub async fn create(
    network_id: String,
    invitee_fingerprint: Option<String>,
    ttl_hours: u64,
    invite_password: String,
) -> Result<()> {
    let mut client = ControlClient::connect()
        .await
        .context("Cannot connect to BillPouch daemon — run `bp hatch post` first")?;

    let resp = client
        .send(ControlRequest::CreateInvite {
            network_id: network_id.clone(),
            invitee_fingerprint: invitee_fingerprint.clone(),
            invite_password,
            ttl_hours: Some(ttl_hours),
        })
        .await
        .context("Failed to send CreateInvite request")?;

    match resp {
        ControlResponse::Ok { data: Some(v) } => {
            let d: InviteData = serde_json::from_value(v).context("Bad InviteData response")?;
            println!("✅ Invite token created");
            println!("   Network:  {}", d.network_id);
            println!("   Inviter:  {}", d.inviter_fingerprint);
            if let Some(fp) = &invitee_fingerprint {
                println!("   Invitee:  {fp}");
            } else {
                println!("   Invitee:  (open — anyone with the password can join)");
            }
            println!("   Expires:  unix {}", d.expires_at);
            println!();
            println!("Share this blob AND the password out-of-band with the invitee:");
            println!();
            println!("{}", d.blob);
            println!();
            println!("Invitee runs:");
            println!(
                "  bp invite join <blob> --invite-password <password> --network {}",
                d.network_id
            );
        }
        ControlResponse::Ok { data: None } => {
            println!("Invite created (no data returned)");
        }
        ControlResponse::Error { message } => {
            anyhow::bail!("Daemon error: {message}");
        }
    }

    Ok(())
}

pub async fn join(blob: String, invite_password: String) -> Result<()> {
    let payload = bp_core::invite::redeem_invite(&blob, &invite_password)
        .context("Failed to redeem invite token")?;

    println!("✅ Invite verified");
    println!("   Network:  {}", payload.network_id);
    println!("   Inviter:  {} (fingerprint)", payload.inviter_fingerprint);

    bp_core::invite::save_invite_key(&payload).context("Failed to save network key from invite")?;
    println!("   NetworkMetaKey saved to local keystore ✓");

    let mut client = ControlClient::connect()
        .await
        .context("Cannot connect to BillPouch daemon — run `bp hatch post` first")?;

    let resp = client
        .send(ControlRequest::Join {
            network_id: payload.network_id.clone(),
        })
        .await
        .context("Failed to send Join request to daemon")?;

    match resp {
        ControlResponse::Ok { .. } => {
            println!("   Joined network '{}' ✓", payload.network_id);
        }
        ControlResponse::Error { message } => {
            println!("   Join response: {message}");
        }
    }

    println!();
    println!(
        "You are now a member of '{}'. Run `bp flock` to see peers.",
        payload.network_id
    );

    Ok(())
}

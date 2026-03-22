//! `bp join` — subscribe to a network's gossipsub topic.
//!
//! This command is used by scripts and automation to subscribe to the gossipsub
//! topic before hatching a service, ensuring peer connections stay alive.
//! It is hidden from the public help output; end users join networks via
//! `bp invite join`.

use crate::client::ControlClient;
use anyhow::Result;
use bp_core::control::protocol::ControlRequest;

pub async fn join(network_id: String) -> Result<()> {
    let mut client = ControlClient::connect().await?;
    let resp = client
        .send(ControlRequest::Join {
            network_id: network_id.clone(),
        })
        .await?;
    match resp {
        bp_core::control::protocol::ControlResponse::Ok { .. } => {
            println!("Joined network '{}'.", network_id);
        }
        bp_core::control::protocol::ControlResponse::Error { message } => {
            eprintln!("Error: {}", message);
        }
    }
    Ok(())
}

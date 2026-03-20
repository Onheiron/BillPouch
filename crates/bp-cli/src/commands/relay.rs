//! `bp relay connect` — dial a relay node for NAT traversal.
//!
//! Sends a [`ControlRequest::ConnectRelay`] to the daemon, which dials the
//! given relay node and establishes a circuit-relay v2 reservation.  Once
//! the reservation is accepted the daemon becomes reachable at
//! `/p2p-circuit` addresses routed through the relay, enabling connectivity
//! even from behind symmetric NAT.
//!
//! [`ControlRequest::ConnectRelay`]: bp_core::control::protocol::ControlRequest::ConnectRelay

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Dial `relay_addr` and request a relay reservation.
pub async fn connect(relay_addr: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let resp = client
        .call(ControlRequest::ConnectRelay {
            relay_addr: relay_addr.clone(),
        })
        .await?;
    println!("{}", serde_json::to_string_pretty(&resp)?);
    Ok(())
}

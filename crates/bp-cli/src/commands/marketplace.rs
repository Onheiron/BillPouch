//! `bp offer / bp agree / bp marketplace` — storage marketplace CLI commands.
//!
//! These commands interact with the storage marketplace layer, allowing Pouch
//! nodes to broadcast storage offers and Bill nodes to accept them.
//!
//! All commands communicate with the running daemon via the Unix control socket.

use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

/// Broadcast a storage offer on `network_id`.
pub async fn propose(
    network_id: String,
    bytes_offered: u64,
    duration_secs: u64,
    price_tokens: u64,
) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data: Option<serde_json::Value> = client
        .request(ControlRequest::ProposeStorage {
            network_id,
            bytes_offered,
            duration_secs,
            price_tokens,
        })
        .await?;
    if let Some(v) = data {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}

/// Accept a storage offer by its `offer_id`.
pub async fn accept(offer_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data: Option<serde_json::Value> = client
        .request(ControlRequest::AcceptStorage { offer_id })
        .await?;
    if let Some(v) = data {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}

/// List all local agreements for `network_id` (empty = all networks).
pub async fn list_agreements(network_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data: Option<serde_json::Value> = client
        .request(ControlRequest::ListAgreements { network_id })
        .await?;
    if let Some(v) = data {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}

/// List received storage offers for `network_id` (empty = all networks).
pub async fn list_offers(network_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data: Option<serde_json::Value> = client
        .request(ControlRequest::ListOffers { network_id })
        .await?;
    if let Some(v) = data {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}

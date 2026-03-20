//! `bp get <chunk_id>` — decode and retrieve a stored file chunk.

use bp_core::{control::protocol::ControlRequest, daemon, identity::Identity};
use std::path::PathBuf;

use crate::client::ControlClient;

/// Fetch and decode the chunk identified by `chunk_id` from the local Pouch,
/// writing the recovered bytes to `output`.
pub async fn get(chunk_id: String, network: String, output: PathBuf) -> anyhow::Result<()> {
    if !Identity::exists()? {
        anyhow::bail!("Not logged in. Run `bp login` first.");
    }

    if !daemon::is_running() {
        anyhow::bail!("Daemon is not running. Start a Pouch first with: bp hatch pouch");
    }

    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::GetFile {
            chunk_id: chunk_id.clone(),
            network_id: network.clone(),
        })
        .await?;

    let d: bp_core::control::protocol::GetFileData = match data {
        Some(v) => serde_json::from_value(v)?,
        None => anyhow::bail!("Daemon returned no data for chunk '{}'", chunk_id),
    };

    tokio::fs::write(&output, &d.data)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot write {:?}: {}", output, e))?;

    println!("✓ Decoded {} bytes → {:?}", d.data.len(), output);
    println!("   chunk_id        : {}", d.chunk_id);
    println!(
        "   fragments used  : {} ({} local, {} remote)",
        d.fragments_used,
        d.fragments_used - d.fragments_remote,
        d.fragments_remote
    );
    println!("   network         : {}", network);

    Ok(())
}

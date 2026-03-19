//! `bp put <file>` — encode and store a file in a local Pouch.

use bp_core::{control::protocol::ControlRequest, daemon, identity::Identity};
use std::path::PathBuf;

use crate::client::ControlClient;

/// Read `path` from disk, encode it with RLNC and store the fragments in the
/// local Pouch for `network_id`.
///
/// Default k / n: k = max(1, len/1024) source symbols, n = k * 2 redundant
/// fragments (capped at 255 per RLNC limit).
pub async fn put(
    path: PathBuf,
    network: String,
    k: Option<usize>,
    n: Option<usize>,
) -> anyhow::Result<()> {
    if !Identity::exists()? {
        anyhow::bail!("Not logged in. Run `bp login` first.");
    }

    if !daemon::is_running() {
        anyhow::bail!("Daemon is not running. Start a Pouch first with: bp hatch pouch");
    }

    let chunk_data = tokio::fs::read(&path)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot read {:?}: {}", path, e))?;

    if chunk_data.is_empty() {
        anyhow::bail!("File is empty: {:?}", path);
    }

    // Sensible defaults: ~1 KiB per symbol, 2× redundancy.
    let auto_k = ((chunk_data.len() / 1024) + 1).min(128);
    let k = k.unwrap_or(auto_k).max(1).min(255);
    let n = n.unwrap_or(k * 2).max(k).min(255);

    let file_bytes = chunk_data.len();

    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::PutFile {
            chunk_data,
            k,
            n,
            network_id: network.clone(),
        })
        .await?;

    if let Some(v) = data {
        let d: bp_core::control::protocol::PutFileData = serde_json::from_value(v)?;
        println!(
            "✓ Stored {} bytes from {:?}",
            file_bytes,
            path.file_name().unwrap_or_default()
        );
        println!("   chunk_id  : {}", d.chunk_id);
        println!("   fragments : {}/{} stored", d.fragments_stored, n);
        println!("   network   : {}", network);
    }

    Ok(())
}

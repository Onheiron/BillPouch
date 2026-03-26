//! `bp put <file>` — encode and store a file in a local Pouch.

use bp_core::{control::protocol::ControlRequest, daemon, identity::Identity};
use std::path::PathBuf;

use crate::client::ControlClient;

/// Read `path` from disk, encode it with RLNC and store the fragments in the
/// local Pouch for `network_id`.
///
/// `k` and `n` are computed by the daemon from live peer QoS data and the
/// `ph` target recovery probability.
pub async fn put(
    path: PathBuf,
    network: String,
    ph: f64,
    q_target: f64,
    name: Option<String>,
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

    let file_bytes = chunk_data.len();

    // If no name given, fall back to the file name from the path.
    let file_name = name.or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
    });

    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::PutFile {
            chunk_data,
            ph: Some(ph),
            q_target: Some(q_target),
            network_id: network.clone(),
            file_name,
        })
        .await?;

    if let Some(v) = data {
        let d: bp_core::control::protocol::PutFileData = serde_json::from_value(v)?;
        println!(
            "✓ Stored {} bytes from {:?}",
            file_bytes,
            path.file_name().unwrap_or_default()
        );
        println!("   chunk_id     : {}", d.chunk_id);
        println!("   k / n        : {} / {}  (q = {:.2})", d.k, d.n, d.q);
        println!("   Ph target    : {:.4}   Pe effective: {:.4}", d.ph, d.pe);
        println!(
            "   fragments    : {}/{} stored locally, {} distributed",
            d.fragments_stored, d.n, d.fragments_distributed
        );
        println!("   network      : {}", network);
    }

    Ok(())
}

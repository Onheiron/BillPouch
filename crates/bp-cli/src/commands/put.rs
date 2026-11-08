use bp_core::control::protocol::{ControlRequest, PutFileData};
use std::path::PathBuf;

use crate::client::ControlClient;

pub async fn put(
    path: PathBuf,
    network: String,
    ph: f64,
    name: Option<String>,
) -> anyhow::Result<()> {
    let path = std::fs::canonicalize(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read {:?}: {}", path, e))?;
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::PutFile {
            path,
            ph: Some(ph),
            network_id: network.clone(),
            file_name: name,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: PutFileData = serde_json::from_value(data)?;
    println!("✓ Stored {} bytes in {} chunk(s)", d.size_bytes, d.chunks);
    println!("   file_id      : {}", d.file_id);
    println!(
        "   k / n        : {} / {}  (one fragment per Pouch of the layer)",
        d.k, d.n
    );
    println!("   Ph target    : {:.4}   Pe effective: {:.4}", d.ph, d.pe);
    println!(
        "   fragments    : {} on this node, {} pushed to other Pouches",
        d.fragments_stored, d.fragments_distributed
    );
    println!("   network      : {}", network);
    Ok(())
}

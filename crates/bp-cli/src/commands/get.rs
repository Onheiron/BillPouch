use bp_core::{
    control::protocol::{ControlRequest, GetFileData},
    daemon,
    identity::Identity,
};
use std::path::PathBuf;

use crate::client::ControlClient;

pub async fn get(file_id: String, network: String, output: PathBuf) -> anyhow::Result<()> {
    if !Identity::exists()? {
        anyhow::bail!("Not logged in. Run `bp login` first.");
    }
    if !daemon::is_running() {
        anyhow::bail!("Daemon is not running. Start a Pouch first with: bp hatch pouch");
    }
    let output = std::path::absolute(&output)?;
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::GetFile {
            file_id,
            network_id: network,
            output,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: GetFileData = serde_json::from_value(data)?;
    println!(
        "✓ '{}': {} bytes in {} chunk(s) → {}",
        d.file_name, d.size_bytes, d.chunks, d.output
    );
    println!("   fragments from other Pouches: {}", d.fragments_remote);
    Ok(())
}

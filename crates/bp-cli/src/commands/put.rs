use bp_core::control::protocol::{ControlRequest, PutFileData};
use std::path::PathBuf;

use crate::client::ControlClient;
use crate::pipe::Fifo;

pub async fn put(
    path: PathBuf,
    network: String,
    ph: Option<f64>,
    name: Option<String>,
    tier: bp_core::storage::StorageTier,
    chunk_bytes: Option<u64>,
) -> anyhow::Result<()> {
    let from_stdin = path.as_os_str() == "-";
    let fifo = from_stdin.then(Fifo::new).transpose()?;
    let path = match &fifo {
        Some(fifo) => fifo.0.clone(),
        None => std::fs::canonicalize(&path)
            .map_err(|e| anyhow::anyhow!("Cannot read {:?}: {}", path, e))?,
    };
    let feeder = fifo.as_ref().map(|fifo| {
        let target = fifo.0.clone();
        tokio::spawn(async move {
            let mut out = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&target)
                .await?;
            tokio::io::copy(&mut tokio::io::stdin(), &mut out).await?;
            anyhow::Ok(())
        })
    });
    let name = name.or_else(|| from_stdin.then(|| "stdin".to_string()));
    let response = ControlClient::connect()
        .await?
        .request(ControlRequest::PutFile {
            path,
            ph,
            network_id: network.clone(),
            file_name: name,
            tier: Some(tier),
            chunk_bytes,
        })
        .await;
    if let Some(feeder) = feeder {
        feeder.abort();
    }
    let data = response?.ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: PutFileData = serde_json::from_value(data)?;
    println!("✓ Stored {} bytes in {} chunk(s)", d.size_bytes, d.chunks);
    println!("   file_id      : {}", d.file_id);
    if d.packed_bytes < d.size_bytes {
        println!(
            "   compressed   : {} bytes ({:.0}% of the original)",
            d.packed_bytes,
            d.packed_bytes as f64 * 100.0 / d.size_bytes.max(1) as f64
        );
    }
    println!(
        "   k / n        : {} / {}  (one fragment per holder, any k rebuild the file)",
        d.k, d.n
    );
    println!("   Ph target    : {:.4}   Pe effective: {:.4}", d.ph, d.pe);
    println!(
        "   fragments    : {} on this node, {} pushed to other Pouches",
        d.fragments_stored, d.fragments_distributed
    );
    println!("   network      : {}", network);
    println!("   folder       : {}", d.tier);
    if d.local {
        println!(
            "⚠  folder {} has fewer than 3 users with a Pouch: the file stays on your own Pouches, unprotected, and is spread over the folder as soon as it fills up",
            d.tier
        );
    }
    Ok(())
}

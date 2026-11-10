use bp_core::control::protocol::{ControlRequest, GetFileData};
use std::path::PathBuf;

use crate::client::ControlClient;
use crate::pipe::Fifo;

pub async fn get(
    file_id: String,
    network: String,
    output: PathBuf,
    range: Option<(u64, u64)>,
) -> anyhow::Result<()> {
    let to_stdout = output.as_os_str() == "-";
    let fifo = to_stdout.then(Fifo::new).transpose()?;
    let output = match &fifo {
        Some(fifo) => fifo.0.clone(),
        None => std::path::absolute(&output)?,
    };
    let reader = fifo.as_ref().map(|fifo| {
        let source = fifo.0.clone();
        tokio::spawn(async move {
            let mut input = tokio::fs::File::open(&source).await?;
            tokio::io::copy(&mut input, &mut tokio::io::stdout()).await?;
            anyhow::Ok(())
        })
    });
    let response = ControlClient::connect()
        .await?
        .request(ControlRequest::GetFile {
            file_id,
            network_id: network,
            output,
            range,
        })
        .await;
    let data = match response {
        Ok(Some(data)) => data,
        Ok(None) => anyhow::bail!("empty response from daemon"),
        Err(e) => {
            if let Some(reader) = reader {
                reader.abort();
            }
            return Err(e);
        }
    };
    if let Some(reader) = reader {
        reader.await??;
        return Ok(());
    }
    let d: GetFileData = serde_json::from_value(data)?;
    println!(
        "✓ '{}': {} of {} bytes in {} chunk(s) → {}",
        d.file_name, d.bytes, d.size_bytes, d.chunks, d.output
    );
    println!("   fragments from other Pouches: {}", d.fragments_remote);
    Ok(())
}

//! `bp ls [--network <id>]` — list files stored by this node.

use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, ListFilesData};
use chrono::{TimeZone, Utc};

/// Query the daemon for the list of files uploaded by this node and print
/// a human-readable table.
pub async fn ls(network_id: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::ListFiles {
            network_id: network_id.clone(),
        })
        .await?;

    match data {
        None => println!("(no data from daemon)"),
        Some(v) => {
            let d: ListFilesData = serde_json::from_value(v)?;
            print_ls(&d);
        }
    }
    Ok(())
}

fn print_ls(d: &ListFilesData) {
    let net_label = if d.network_id.is_empty() {
        "all networks".to_string()
    } else {
        format!("network: {}", d.network_id)
    };

    println!(
        "📂 Files ({}) — {} file(s), {}",
        net_label,
        d.total_files,
        fmt_bytes(d.total_bytes),
    );
    println!("─────────────────────────────────────────────────────────────────");

    if d.files.is_empty() {
        println!("   (no files — upload with `bp put <file>`)");
        return;
    }

    println!(
        "  {:<32}  {:>10}  {:<18}  Uploaded",
        "Name", "Size", "chunk_id"
    );
    println!("  {}", "─".repeat(78));

    for f in &d.files {
        let ts = Utc
            .timestamp_opt(f.uploaded_at as i64, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "?".to_string());

        let name = if f.file_name.len() > 32 {
            format!("{}…", &f.file_name[..31])
        } else {
            f.file_name.clone()
        };

        println!(
            "  {:<32}  {:>10}  {:<18}  {}",
            name,
            fmt_bytes(f.size_bytes),
            &f.chunk_id[..18.min(f.chunk_id.len())],
            ts
        );
    }
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1_073_741_824 {
        format!("{:.1} GiB", b as f64 / 1_073_741_824.0)
    } else if b >= 1_048_576 {
        format!("{:.1} MiB", b as f64 / 1_048_576.0)
    } else if b >= 1024 {
        format!("{:.1} KiB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

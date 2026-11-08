use crate::client::ControlClient;
use bp_core::control::protocol::{ControlRequest, ListFilesData, ShareData};
use chrono::{TimeZone, Utc};

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
        "  {:<32}  {:>10}  {:<18}  {:<16}  Uploaded",
        "Name", "Size", "file_id", "Owner"
    );
    println!("  {}", "─".repeat(96));

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

        let owner = if f.mine {
            match f.shared_with {
                0 => "you".to_string(),
                n => format!("you, shared ×{n}"),
            }
        } else {
            f.owner.chars().take(16).collect()
        };
        println!(
            "  {:<32}  {:>10}  {:<18}  {:<16}  {}",
            name,
            fmt_bytes(f.size_bytes),
            f.file_id,
            owner,
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

pub async fn share(network_id: String, file_id: String, member: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::Share {
            network_id,
            file_id,
            member,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: ShareData = serde_json::from_value(data)?;
    println!(
        "🔑 '{}' is now shared with {} on '{}'.",
        d.file_name, d.member, d.network_id
    );
    Ok(())
}

pub async fn unshare(network_id: String, file_id: String, member: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::Unshare {
            network_id,
            file_id,
            member,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let d: ShareData = serde_json::from_value(data)?;
    println!(
        "🔒 {} can no longer fetch '{}' on '{}'.",
        d.member, d.file_name, d.network_id
    );
    Ok(())
}

pub async fn rm(network_id: String, file_id: String) -> anyhow::Result<()> {
    let data = ControlClient::connect()
        .await?
        .request(ControlRequest::DeleteFile {
            file_id,
            network_id,
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("empty response from daemon"))?;
    let name = data["file_name"].as_str().unwrap_or("?");
    if data["removed_from_network"].as_bool().unwrap_or(false) {
        println!("🗑  '{name}' deleted: the Pouches drop its fragments.");
    } else {
        println!("🗑  '{name}' removed from your files (it stays with its owner).");
    }
    Ok(())
}

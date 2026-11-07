use bp_core::{
    control::protocol::{ControlRequest, HatchData},
    daemon,
    identity::Identity,
    service::ServiceType,
};
use std::collections::HashMap;
use tokio::process::Command;

use crate::client::ControlClient;

pub async fn hatch(
    service_type: ServiceType,
    network_id: String,
    metadata: HashMap<String, serde_json::Value>,
    passphrase: Option<String>,
) -> anyhow::Result<()> {
    if !Identity::exists()? {
        anyhow::bail!("Not logged in. Run `bp login` first.");
    }

    if !daemon::is_running() {
        start_daemon_background(passphrase).await?;
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }

    let mut client = ControlClient::connect().await?;
    let data = client
        .request(ControlRequest::Hatch {
            service_type,
            network_id,
            metadata,
        })
        .await?;

    if let Some(v) = data {
        let hatch: HatchData = serde_json::from_value(v)?;
        println!("{}", hatch.message);
        println!("   Service ID  : {}", hatch.service_id);
        println!("   Type        : {}", hatch.service_type);
        println!("   Network     : {}", hatch.network_id);
    }
    Ok(())
}

async fn start_daemon_background(passphrase: Option<String>) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--daemon");
    if let Some(pass) = passphrase {
        cmd.arg("--passphrase").arg(pass);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start daemon: {}", e))?;

    println!("🚀 Daemon started in background");
    Ok(())
}

use anyhow::{bail, Context};
use bp_core::{config, identity::Identity, session};
use std::time::{Duration, Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

fn listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300))
        .is_ok()
}

fn api_binary() -> anyhow::Result<std::path::PathBuf> {
    let sibling = std::env::current_exe()?.with_file_name("bp-api");
    if sibling.exists() {
        return Ok(sibling);
    }
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("bp-api"))
                .find(|p| p.exists())
        })
        .context("bp-api not found next to bp or in PATH: build it with `cargo build --release`")
}

async fn start_api(port: u16, host: &str) -> anyhow::Result<()> {
    let log_path = config::base_dir()?.join("api.log");
    let log = std::fs::File::create(&log_path)?;
    let mut child = tokio::process::Command::new(api_binary()?)
        .args(["--port", &port.to_string(), "--host", host])
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("cannot start bp-api")?;
    let started = Instant::now();
    while started.elapsed() < STARTUP_TIMEOUT {
        if listening(port) {
            println!(
                "🚀 bp-api started on port {port} (log: {})",
                log_path.display()
            );
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "bp-api exited ({status}):\n{}",
                std::fs::read_to_string(&log_path).unwrap_or_default()
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    bail!(
        "bp-api did not start listening on port {port}; see {}",
        log_path.display()
    )
}

pub async fn dashboard(
    port: u16,
    host: &str,
    no_open: bool,
    passphrase: Option<&str>,
) -> anyhow::Result<()> {
    let identity = Identity::load(passphrase)?;
    crate::client::ControlClient::connect().await?;
    if !listening(port) {
        start_api(port, host).await?;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let token = session::issue(&identity, now)?;
    let url = format!("https://127.0.0.1:{port}/#session={token}");
    println!(
        "🔐 BillPouch for {}, session valid 12 h:",
        identity.fingerprint
    );
    println!("   {url}");
    println!("   The certificate is self-signed: accept it once in the browser.");
    if !no_open {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener)
            .arg(&url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    Ok(())
}

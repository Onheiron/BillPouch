use anyhow::{bail, Context};
use bp_core::{
    config,
    control::{
        client,
        protocol::{ControlRequest, ControlResponse},
    },
    daemon,
    identity::Identity,
};
use std::{
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

static PASSPHRASE: OnceLock<Option<String>> = OnceLock::new();

pub fn set_passphrase(passphrase: Option<String>) {
    let _ = PASSPHRASE.set(passphrase);
}

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ControlClient {
    socket_path: PathBuf,
}

impl ControlClient {
    pub async fn connect() -> anyhow::Result<Self> {
        if !Identity::exists()? {
            bail!("Not logged in. Run `bp login` first.");
        }
        if !daemon::is_running() {
            start_daemon().await?;
        }
        Ok(Self {
            socket_path: config::socket_path()?,
        })
    }

    pub async fn send(&mut self, req: ControlRequest) -> anyhow::Result<ControlResponse> {
        client::send(&self.socket_path, &req)
            .await
            .context("Daemon request failed")
    }

    pub async fn request(
        &mut self,
        req: ControlRequest,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        match self.send(req).await? {
            ControlResponse::Ok { data } => Ok(data),
            ControlResponse::Error { message } => bail!("{}", message),
        }
    }
}

async fn start_daemon() -> anyhow::Result<()> {
    config::ensure_dirs()?;
    let log_path = config::log_path()?;
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("cannot create {}", log_path.display()))?;
    let mut cmd = tokio::process::Command::new(std::env::current_exe()?);
    cmd.arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if let Some(Some(pass)) = PASSPHRASE.get() {
        cmd.env("BP_PASSPHRASE", pass);
    }
    let mut child = cmd.spawn().context("cannot start the daemon")?;

    let started = Instant::now();
    while started.elapsed() < STARTUP_TIMEOUT {
        if daemon::is_running() {
            eprintln!("🚀 Daemon started (log: {})", log_path.display());
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "the daemon exited ({status}) before it was ready:\n{}",
                tail(&log_path)
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    bail!(
        "the daemon did not open its control socket within {}s:\n{}",
        STARTUP_TIMEOUT.as_secs(),
        tail(&log_path)
    )
}

fn tail(path: &std::path::Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

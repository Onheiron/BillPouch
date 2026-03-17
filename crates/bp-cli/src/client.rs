//! Thin async client that connects to the daemon control socket, sends one
//! JSON request and returns the parsed JSON response.

use anyhow::{bail, Context};
use bp_core::{
    config,
    control::protocol::{ControlRequest, ControlResponse},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

pub struct ControlClient {
    stream: UnixStream,
}

impl ControlClient {
    /// Open a connection to the daemon control socket.
    pub async fn connect() -> anyhow::Result<Self> {
        let path = config::socket_path()?;
        if !path.exists() {
            bail!("Daemon is not running. Start a service first with: bp hatch <type>");
        }
        let stream = UnixStream::connect(&path)
            .await
            .with_context(|| format!("Cannot connect to control socket at {:?}", path))?;
        Ok(Self { stream })
    }

    /// Send a request and receive a response.
    pub async fn send(&mut self, req: ControlRequest) -> anyhow::Result<ControlResponse> {
        let (read_half, mut write_half) = self.stream.split();

        let mut json = serde_json::to_string(&req)?;
        json.push('\n');
        write_half.write_all(json.as_bytes()).await?;

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let resp: ControlResponse = serde_json::from_str(line.trim())
            .with_context(|| format!("Invalid response from daemon: {}", line))?;
        Ok(resp)
    }

    /// Convenience: send and unwrap an Ok response (or bail on Error).
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

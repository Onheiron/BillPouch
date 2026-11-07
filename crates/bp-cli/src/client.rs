use anyhow::{bail, Context};
use bp_core::{
    config,
    control::{
        client,
        protocol::{ControlRequest, ControlResponse},
    },
};
use std::path::PathBuf;

pub struct ControlClient {
    socket_path: PathBuf,
}

impl ControlClient {
    pub async fn connect() -> anyhow::Result<Self> {
        let socket_path = config::socket_path()?;
        if !socket_path.exists() {
            bail!("Daemon is not running. Start a service first with: bp hatch <type>");
        }
        Ok(Self { socket_path })
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

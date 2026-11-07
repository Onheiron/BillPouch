//! Client side of the control protocol, shared by `bp` and `bp-api`.

use crate::{
    control::protocol::{ControlRequest, ControlResponse},
    error::{BpError, BpResult},
};
use std::path::Path;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

/// Send one request to the daemon listening on `socket_path` and return its
/// response.  `Err` means the daemon could not be reached or replied with
/// invalid JSON; daemon-side failures come back as [`ControlResponse::Error`].
pub async fn send(socket_path: &Path, req: &ControlRequest) -> BpResult<ControlResponse> {
    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(|e| BpError::Control(format!("cannot connect to {socket_path:?}: {e}")))?;
    let (read_half, mut write_half) = stream.split();

    let mut json = serde_json::to_string(req)?;
    json.push('\n');
    write_half.write_all(json.as_bytes()).await?;

    let mut line = String::new();
    BufReader::new(read_half).read_line(&mut line).await?;
    Ok(serde_json::from_str(line.trim())?)
}

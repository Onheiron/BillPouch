use crate::client::ControlClient;
use bp_core::control::protocol::ControlRequest;

pub async fn connect(relay_addr: String) -> anyhow::Result<()> {
    let mut client = ControlClient::connect().await?;
    let resp: Option<serde_json::Value> = client
        .request(ControlRequest::ConnectRelay {
            relay_addr: relay_addr.clone(),
        })
        .await?;
    if let Some(data) = resp {
        println!("{}", serde_json::to_string_pretty(&data)?);
    }
    Ok(())
}

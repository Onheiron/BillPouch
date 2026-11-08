use bp_core::{
    control::protocol::{ControlRequest, HatchData},
    service::ServiceType,
};
use std::collections::HashMap;

use crate::client::ControlClient;

pub async fn hatch(
    service_type: ServiceType,
    network_id: String,
    metadata: HashMap<String, serde_json::Value>,
) -> anyhow::Result<()> {
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

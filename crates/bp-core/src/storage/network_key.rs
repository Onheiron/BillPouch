use crate::{
    config,
    error::{BpError, BpResult},
};
use rand::RngCore;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMetaKey(pub [u8; 32]);

impl NetworkMetaKey {
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Self(key)
    }

    pub fn load(network_id: &str) -> BpResult<Option<Self>> {
        let Some(hex_key) = read_keys()?.remove(network_id) else {
            return Ok(None);
        };
        let bytes = hex::decode(&hex_key)
            .map_err(|e| BpError::Config(format!("Invalid network key for '{network_id}': {e}")))?;
        let arr: [u8; 32] = bytes.try_into().map_err(|b: Vec<u8>| {
            BpError::Config(format!(
                "Network key for '{network_id}' has wrong length: {}",
                b.len()
            ))
        })?;
        Ok(Some(Self(arr)))
    }

    pub fn save(&self, network_id: &str) -> BpResult<()> {
        config::ensure_dirs()?;
        let mut map = read_keys()?;
        map.insert(network_id.to_string(), hex::encode(self.0));
        std::fs::write(
            config::network_keys_path()?,
            serde_json::to_string_pretty(&map)?,
        )?;
        Ok(())
    }

    pub fn load_or_create(network_id: &str) -> BpResult<Self> {
        if let Some(key) = Self::load(network_id)? {
            return Ok(key);
        }
        let key = Self::generate();
        key.save(network_id)?;
        tracing::info!(network = %network_id, "Generated new NetworkMetaKey");
        Ok(key)
    }
}

fn read_keys() -> BpResult<HashMap<String, String>> {
    let path = config::network_keys_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

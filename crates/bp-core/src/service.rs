use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    Pouch,
    Bill,
    Post,
}

impl std::fmt::Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceType::Pouch => write!(f, "pouch"),
            ServiceType::Bill => write!(f, "bill"),
            ServiceType::Post => write!(f, "post"),
        }
    }
}

impl FromStr for ServiceType {
    type Err = BpError;
    fn from_str(s: &str) -> BpResult<Self> {
        match s.to_lowercase().as_str() {
            "pouch" => Ok(ServiceType::Pouch),
            "bill" => Ok(ServiceType::Bill),
            "post" => Ok(ServiceType::Post),
            other => Err(BpError::Service(format!(
                "Unknown service type '{}'. Use: bill | pouch | post",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Running,
    Paused { eta_minutes: u64, paused_at: u64 },
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Running => write!(f, "running"),
            ServiceStatus::Paused { eta_minutes, .. } => {
                write!(f, "paused (ETA {} min)", eta_minutes)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: String,
    pub service_type: ServiceType,
    pub network_id: String,
    pub status: ServiceStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ServiceInfo {
    pub fn new(
        service_type: ServiceType,
        network_id: String,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            service_type,
            network_id,
            status: ServiceStatus::Running,
            started_at: chrono::Utc::now(),
            metadata,
        }
    }
}

#[derive(Default)]
pub struct ServiceRegistry {
    services: HashMap<String, ServiceInfo>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, info: ServiceInfo) {
        self.services.insert(info.id.clone(), info);
    }

    pub fn get(&self, id: &str) -> Option<&ServiceInfo> {
        self.services.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ServiceInfo> {
        self.services.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<ServiceInfo> {
        self.services.remove(id)
    }

    pub fn all(&self) -> Vec<&ServiceInfo> {
        self.services.values().collect()
    }
}

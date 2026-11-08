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
    path: Option<std::path::PathBuf>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load() -> Self {
        let path = crate::config::services_path().ok();
        let services = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<ServiceInfo>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        Self { services, path }
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let all: Vec<&ServiceInfo> = self.services.values().collect();
        let written = serde_json::to_vec_pretty(&all)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(path, json));
        if let Err(e) = written {
            tracing::warn!("cannot save services: {e}");
        }
    }

    pub fn register(&mut self, info: ServiceInfo) {
        self.services.insert(info.id.clone(), info);
        self.save();
    }

    pub fn get(&self, id: &str) -> Option<&ServiceInfo> {
        self.services.get(id)
    }

    pub fn update<R>(&mut self, id: &str, change: impl FnOnce(&mut ServiceInfo) -> R) -> Option<R> {
        let result = change(self.services.get_mut(id)?);
        self.save();
        Some(result)
    }

    pub fn remove(&mut self, id: &str) -> Option<ServiceInfo> {
        let removed = self.services.remove(id);
        self.save();
        removed
    }

    pub fn all(&self) -> Vec<&ServiceInfo> {
        self.services.values().collect()
    }
}

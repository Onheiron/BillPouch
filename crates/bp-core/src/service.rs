//! Service definitions for BillPouch.
//!
//! Three service types, each maps to a different role in the pelican metaphor:
//!
//! - `Pouch`  — bids local storage into the network (the pelican's pouch)
//! - `Bill`   — personal I/O interface to the user's files (the pelican's bill)
//! - `Post`   — pure routing / relay node; contributes only CPU + RAM

use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

/// The role a service plays in the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    /// Bids local storage into the network.
    Pouch,
    /// Personal file I/O interface.
    Bill,
    /// Routing / relay node (CPU + RAM only).
    Post,
}

impl std::fmt::Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceType::Pouch => write!(f, "pouch"),
            ServiceType::Bill  => write!(f, "bill"),
            ServiceType::Post  => write!(f, "post"),
        }
    }
}

impl FromStr for ServiceType {
    type Err = BpError;
    fn from_str(s: &str) -> BpResult<Self> {
        match s.to_lowercase().as_str() {
            "pouch" => Ok(ServiceType::Pouch),
            "bill"  => Ok(ServiceType::Bill),
            "post"  => Ok(ServiceType::Post),
            other   => Err(BpError::Service(format!(
                "Unknown service type '{}'. Use: bill | pouch | post",
                other
            ))),
        }
    }
}

/// Runtime status of a local service instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error(String),
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Starting     => write!(f, "starting"),
            ServiceStatus::Running      => write!(f, "running"),
            ServiceStatus::Stopping     => write!(f, "stopping"),
            ServiceStatus::Stopped      => write!(f, "stopped"),
            ServiceStatus::Error(msg)   => write!(f, "error: {}", msg),
        }
    }
}

/// Metadata for a *local* service instance (daemon-side).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Unique ID for this service instance (UUID v4).
    pub id: String,
    pub service_type: ServiceType,
    pub network_id: String,
    pub status: ServiceStatus,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Extensible metadata (e.g. storage_bytes for Pouch, mount_path for Bill).
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
            status: ServiceStatus::Starting,
            started_at: chrono::Utc::now(),
            metadata,
        }
    }
}

/// In-process registry of running services (held inside the daemon).
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

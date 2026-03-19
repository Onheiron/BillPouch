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

/// Runtime status of a local service instance.
///
/// Transitions: `Starting` → `Running` → `Stopping` → `Stopped`.
/// Any state can transition to `Error` if an unrecoverable fault occurs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    /// Service is initialising (e.g. subscribing to gossip topics).
    Starting,
    /// Service is fully operational and announcing itself to the network.
    Running,
    /// Graceful shutdown has been requested but not yet completed.
    Stopping,
    /// Service has shut down cleanly.
    Stopped,
    /// Service encountered an unrecoverable error; payload is a description.
    Error(String),
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceStatus::Starting => write!(f, "starting"),
            ServiceStatus::Running => write!(f, "running"),
            ServiceStatus::Stopping => write!(f, "stopping"),
            ServiceStatus::Stopped => write!(f, "stopped"),
            ServiceStatus::Error(msg) => write!(f, "error: {}", msg),
        }
    }
}

/// Metadata for a *local* service instance (daemon-side).
///
/// Created by [`ServiceInfo::new`] when a `Hatch` request is dispatched.
/// Stored in the [`ServiceRegistry`] for the lifetime of the running service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    /// Unique ID for this service instance (UUID v4).
    pub id: String,
    /// Whether this node offers storage, file I/O, or pure relay.
    pub service_type: ServiceType,
    /// Identifier of the BillPouch network this service participates in.
    pub network_id: String,
    /// Current lifecycle state of the service.
    pub status: ServiceStatus,
    /// UTC timestamp when the service was started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Extensible metadata (e.g. `storage_bytes` for Pouch, `mount_path` for Bill).
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ServiceInfo {
    /// Construct a new `ServiceInfo` in the [`ServiceStatus::Starting`] state.
    ///
    /// Generates a fresh UUID v4 `service_id` and records the current UTC time
    /// as `started_at`.
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

/// In-process registry of all running service instances (held inside `DaemonState`).
///
/// Keyed by `service_id` (UUID v4 string).  All mutations go through `DaemonState`'s
/// `RwLock<ServiceRegistry>` so that concurrent control requests are safe.
#[derive(Default)]
pub struct ServiceRegistry {
    services: HashMap<String, ServiceInfo>,
}

impl ServiceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a service entry.  The entry's `id` is used as the key.
    pub fn register(&mut self, info: ServiceInfo) {
        self.services.insert(info.id.clone(), info);
    }

    /// Look up a service by its UUID.  Returns `None` if not found.
    pub fn get(&self, id: &str) -> Option<&ServiceInfo> {
        self.services.get(id)
    }

    /// Look up a service mutably (e.g. to update its [`ServiceStatus`]).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ServiceInfo> {
        self.services.get_mut(id)
    }

    /// Remove and return a service by UUID.  Returns `None` if not found.
    pub fn remove(&mut self, id: &str) -> Option<ServiceInfo> {
        self.services.remove(id)
    }

    /// Return references to all registered services (order is unspecified).
    pub fn all(&self) -> Vec<&ServiceInfo> {
        self.services.values().collect()
    }
}

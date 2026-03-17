//! JSON-RPC–style control protocol between the CLI and the daemon.
//!
//! Messages are newline-delimited JSON frames exchanged over a Unix socket.

use crate::{
    network::state::NodeInfo,
    service::{ServiceInfo, ServiceType},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request sent from CLI → daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Spawn a new service of `service_type` in `network_id`.
    Hatch {
        service_type: ServiceType,
        network_id: String,
        /// Optional extra config (e.g. `{"storage_bytes": 10737418240}` for Pouch).
        metadata: HashMap<String, serde_json::Value>,
    },
    /// Return all known peers and network summary.
    Flock,
    /// Kill a running service by ID.
    Farewell { service_id: String },
    /// Join an existing network (subscribe to gossip topics).
    Join { network_id: String },
    /// Leave a network.
    Leave { network_id: String },
    /// Return info about this daemon (identity, services, networks).
    Status,
    /// Ping — used to check if daemon is alive.
    Ping,
}

/// Response sent from daemon → CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ControlResponse {
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    },
    Error {
        message: String,
    },
}

impl ControlResponse {
    pub fn ok(data: impl Serialize) -> Self {
        let v = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
        ControlResponse::Ok { data: Some(v) }
    }

    pub fn ok_empty() -> Self {
        ControlResponse::Ok { data: None }
    }

    pub fn err(msg: impl ToString) -> Self {
        ControlResponse::Error { message: msg.to_string() }
    }
}

// ── Typed data payloads returned in ControlResponse::Ok.data ─────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HatchData {
    pub service_id: String,
    pub service_type: ServiceType,
    pub network_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlockData {
    pub local_services: Vec<ServiceInfo>,
    pub known_peers: Vec<NodeInfo>,
    pub networks: Vec<String>,
    pub peer_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub peer_id: String,
    pub fingerprint: String,
    pub alias: Option<String>,
    pub local_services: Vec<ServiceInfo>,
    pub networks: Vec<String>,
    pub known_peers: usize,
    pub version: String,
}

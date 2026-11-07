use crate::{
    network::state::NodeInfo,
    service::{ServiceInfo, ServiceType},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlRequest {
    Hatch {
        service_type: ServiceType,
        network_id: String,
        metadata: HashMap<String, serde_json::Value>,
    },
    Flock,
    Farewell {
        service_id: String,
    },
    FarewellEvict {
        service_id: String,
    },
    Pause {
        service_id: String,
        eta_minutes: u64,
    },
    Resume {
        service_id: String,
    },
    CreateNetwork {
        name: String,
    },
    Invite {
        network_id: String,
        member_key: String,
    },
    JoinNetwork {
        token: String,
    },
    Leave {
        network_id: String,
        force: bool,
    },
    Status,
    AnnounceNow,
    Ping,
    PutFile {
        chunk_data: Vec<u8>,
        ph: Option<f64>,
        q_target: Option<f64>,
        network_id: String,
        file_name: Option<String>,
    },
    GetFile {
        chunk_id: String,
        network_id: String,
    },
    ConnectRelay {
        relay_addr: String,
    },
    StorageInfo {
        network_id: String,
    },
    ListFiles {
        network_id: String,
    },
}

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

    pub fn err(msg: impl ToString) -> Self {
        ControlResponse::Error {
            message: msg.to_string(),
        }
    }
}

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
    pub reputation_tier: String,
    pub pouch_stats: Vec<PouchStat>,
    pub network_qos: Option<NetworkQosSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutFileData {
    pub chunk_id: String,
    pub k: usize,
    pub n: usize,
    pub q: f64,
    pub ph: f64,
    pub pe: f64,
    pub fragments_stored: usize,
    pub fragments_distributed: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetFileData {
    pub chunk_id: String,
    pub data: Vec<u8>,
    pub fragments_used: usize,
    pub fragments_remote: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteData {
    pub token: String,
    pub network_id: String,
    pub member: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PouchStat {
    pub service_id: String,
    pub network_id: String,
    pub storage_tier: Option<String>,
    pub storage_bid_bytes: u64,
    pub storage_used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkQosSummary {
    pub observed_peers: usize,
    pub avg_stability: f64,
    pub tier_counts: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfoData {
    pub pouches: Vec<PouchStat>,
    pub total_bid_bytes: u64,
    pub total_used_bytes: u64,
    pub total_available_bytes: u64,
    pub total_files_uploaded: usize,
    pub total_uploaded_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub file_name: String,
    pub size_bytes: u64,
    pub chunk_id: String,
    pub network_id: String,
    pub uploaded_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesData {
    pub files: Vec<FileEntry>,
    pub network_id: String,
    pub total_files: usize,
    pub total_bytes: u64,
}

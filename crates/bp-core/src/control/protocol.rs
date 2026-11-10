use crate::{
    network::{qos::PeerQos, state::NodeInfo},
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
    Members {
        network_id: String,
    },
    Share {
        network_id: String,
        file_id: String,
        member: String,
    },
    Expel {
        network_id: String,
        member: String,
    },
    Params {
        network_id: String,
        propose: Option<crate::ledger::Params>,
    },
    Leave {
        network_id: String,
        force: bool,
    },
    Status,
    AnnounceNow,
    Ping,
    PutFile {
        path: std::path::PathBuf,
        ph: Option<f64>,
        network_id: String,
        file_name: Option<String>,
        tier: Option<crate::storage::StorageTier>,
    },
    GetFile {
        file_id: String,
        network_id: String,
        output: std::path::PathBuf,
    },
    DeleteFile {
        file_id: String,
        network_id: String,
    },
    Unshare {
        network_id: String,
        file_id: String,
        member: String,
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
    pub qos: Vec<PeerQos>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub peer_id: String,
    pub addresses: Vec<String>,
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
    pub file_id: String,
    pub tier: String,
    pub local: bool,
    pub chunks: usize,
    pub size_bytes: u64,
    pub k: usize,
    pub n: usize,
    pub ph: f64,
    pub pe: f64,
    pub fragments_stored: usize,
    pub fragments_distributed: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetFileData {
    pub file_id: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub chunks: usize,
    pub fragments_remote: usize,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteData {
    pub token: String,
    pub network_id: String,
    pub member: String,
    pub for_device: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub fingerprint: String,
    pub name: String,
    pub tier: String,
    pub weight: u64,
    pub credit_bytes: u64,
    pub used_bytes: u64,
    pub devices: usize,
    pub joined_at: u64,
    pub expel_approvals: usize,
    pub trust: f64,
    pub locked_trust: f64,
    pub open_bets: usize,
    pub sponsoring: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembersData {
    pub network_id: String,
    pub me: String,
    pub height: u64,
    pub epoch: u64,
    pub total_weight: u64,
    pub members: Vec<MemberInfo>,
    pub expelled: Vec<String>,
    pub params: crate::ledger::Params,
    pub availability: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamsData {
    pub network_id: String,
    pub params: crate::ledger::Params,
    pub voted: Option<crate::ledger::Params>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpelData {
    pub network_id: String,
    pub member: String,
    pub outcome: crate::chain::ExpelOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PouchStat {
    pub service_id: String,
    pub network_id: String,
    pub tier: String,
    pub quota_bytes: u64,
    pub fragment_bytes: u64,
    pub filler_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditStat {
    pub network_id: String,
    pub offered_bytes: u64,
    pub credit_bytes: u64,
    pub used_bytes: u64,
    pub rate: Option<(u64, u64)>,
    pub quota_bytes: u64,
    pub quota_verified_bytes: u64,
    pub quota_used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkQosSummary {
    pub observed_peers: usize,
    pub avg_availability: f64,
    pub tier_counts: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfoData {
    pub pouches: Vec<PouchStat>,
    pub credits: Vec<CreditStat>,
    pub folders: Vec<FolderInfo>,
    pub files_uploaded: usize,
    pub uploaded_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderInfo {
    pub network_id: String,
    pub tier: String,
    pub users: usize,
    pub pouches: usize,
    pub active: bool,
    pub k: usize,
    pub n: usize,
    pub pe: f64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub file_name: String,
    pub size_bytes: u64,
    pub file_id: String,
    pub chunks: usize,
    pub network_id: String,
    pub uploaded_at: u64,
    pub owner: String,
    pub mine: bool,
    pub shared_with: usize,
    pub tier: Option<String>,
    pub local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareData {
    pub network_id: String,
    pub file_id: String,
    pub file_name: String,
    pub member: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListFilesData {
    pub files: Vec<FileEntry>,
    pub network_id: String,
    pub total_files: usize,
    pub total_bytes: u64,
}

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
    /// Encode `chunk_data` with RLNC and store fragments in the local Pouch.
    ///
    /// `k` and `n` are derived automatically by the daemon from live QoS data
    /// and the `ph` target recovery probability.
    /// Returns a `chunk_id` (BLAKE3 hash prefix) that can be used with `GetFile`.
    PutFile {
        /// Raw bytes to encode and store.
        chunk_data: Vec<u8>,
        /// Target recovery probability (default: 0.999).
        /// The daemon derives k/n from live peer QoS and this target.
        ph: Option<f64>,
        /// Redundancy overhead fraction (default: 1.0 = k extra fragments).
        q_target: Option<f64>,
        /// Network ID whose Pouch should hold the fragments.
        network_id: String,
    },
    /// Retrieve and decode a stored file chunk by its `chunk_id`.
    GetFile {
        /// BLAKE3 chunk hash prefix (16 hex chars) returned by `PutFile`.
        chunk_id: String,
        /// Network ID to search for the chunk.
        network_id: String,
    },
    /// Dial a relay node to enable NAT traversal via circuit relay v2.
    ///
    /// The daemon dials `relay_addr`, establishes a reservation, and
    /// subsequently becomes reachable through the relay at
    /// `/p2p-circuit` addresses.
    ConnectRelay {
        /// Full multiaddr of the relay node, e.g.
        /// `/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW...`
        relay_addr: String,
    },
    /// Generate a signed + password-encrypted invite token for `network_id`.
    ///
    /// The token contains the `NetworkMetaKey` for the network, signed with
    /// the daemon's Ed25519 key and encrypted with `invite_password`.  Share
    /// the blob and the password with the invitee out-of-band.
    CreateInvite {
        /// Network to invite the recipient into.
        network_id: String,
        /// Optional: fingerprint of the specific invitee.  `None` = open invite.
        invitee_fingerprint: Option<String>,
        /// Password used to encrypt the token — shared out-of-band.
        invite_password: String,
        /// Token validity in hours (default: 24).
        ttl_hours: Option<u64>,
    },
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
        ControlResponse::Error {
            message: msg.to_string(),
        }
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

/// Returned by [`ControlRequest::PutFile`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutFileData {
    /// BLAKE3 chunk hash prefix identifying this chunk.
    pub chunk_id: String,
    /// Recovery threshold: minimum fragments needed to reconstruct.
    pub k: usize,
    /// Total fragments generated per chunk.
    pub n: usize,
    /// Effective redundancy overhead: `(n − k) / k`.
    pub q: f64,
    /// Target recovery probability declared at upload time.
    pub ph: f64,
    /// Rolling effective recovery probability at upload time.
    pub pe: f64,
    /// How many fragments were stored locally.
    pub fragments_stored: usize,
    /// How many fragments were pushed to remote Pouches.
    pub fragments_distributed: usize,
    /// Human-readable summary.
    pub message: String,
}

/// Returned by [`ControlRequest::GetFile`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetFileData {
    /// BLAKE3 chunk hash prefix of the retrieved chunk.
    pub chunk_id: String,
    /// Recovered raw bytes.
    pub data: Vec<u8>,
    /// How many fragments were used for decoding.
    pub fragments_used: usize,
    /// How many of those fragments came from remote Pouches.
    pub fragments_remote: usize,
}

/// Returned by [`ControlRequest::CreateInvite`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteData {
    /// Hex-encoded invite blob to share with the invitee.
    pub blob: String,
    /// Network this invite grants access to.
    pub network_id: String,
    /// Unix timestamp when the token expires.
    pub expires_at: u64,
    /// Fingerprint of the inviter (for display).
    pub inviter_fingerprint: String,
}

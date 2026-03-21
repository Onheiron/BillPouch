//! Storage marketplace — offers and agreements between BillPouch users.
//!
//! ## Workflow
//!
//! 1. A Pouch owner calls `bp offer <bytes> --duration <secs>` (or the REST
//!    endpoint) to broadcast a [`StorageOffer`] on the gossip topic
//!    `billpouch/v1/{network_id}/offers`.
//! 2. Other nodes receive the offer and store it in [`ReceivedOffers`].
//! 3. A Bill user calls `bp agree <offer_id>` to accept an offer.
//!    This creates a local [`StorageAgreement`] and broadcasts an
//!    acceptance back on the same gossip topic.
//! 4. Both sides track the agreement in their local [`AgreementStore`].

use crate::error::{BpError, BpResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── StorageOffer ──────────────────────────────────────────────────────────────

/// A storage offer broadcast via gossip by a Pouch node.
///
/// Serialised as JSON and published on
/// `billpouch/v1/{network_id}/offers`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageOffer {
    /// Unique identifier for this offer (UUID v4).
    pub id: String,
    /// Fingerprint of the node making the offer.
    pub offerer_fingerprint: String,
    /// Human-readable alias of the offerer.
    pub offerer_alias: String,
    /// Network for which storage is being offered.
    pub network_id: String,
    /// Storage capacity offered in bytes.
    pub bytes_offered: u64,
    /// Duration for which the offer is valid (seconds).
    pub duration_secs: u64,
    /// Optional price in protocol tokens (0 = free).
    pub price_tokens: u64,
    /// Unix timestamp when the offer was announced.
    pub announced_at: u64,
}

impl StorageOffer {
    /// Gossipsub topic for storage offers in `network_id`.
    pub fn topic_name(network_id: &str) -> String {
        format!("billpouch/v1/{}/offers", network_id)
    }
}

// ── StorageAgreement ──────────────────────────────────────────────────────────

/// Status of a storage agreement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgreementStatus {
    /// Acceptance sent; waiting for offerer confirmation.
    Pending,
    /// Both parties have confirmed; storage is in use.
    Active,
    /// Agreement ran to completion.
    Completed,
    /// One party cancelled before or during the agreement.
    Cancelled,
}

/// A bilateral storage agreement between a Pouch offerer and a Bill requester.
///
/// Created locally when the requester calls `AcceptStorage`.
/// Persisted to [`AgreementStore`] and broadcast on the offers gossip topic
/// so the offerer can record the acceptance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageAgreement {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// ID of the originating [`StorageOffer`].
    pub offer_id: String,
    /// Fingerprint of the node providing storage.
    pub offerer_fingerprint: String,
    /// Fingerprint of the node requesting storage.
    pub requester_fingerprint: String,
    /// Network this agreement is scoped to.
    pub network_id: String,
    /// Agreed storage capacity in bytes.
    pub bytes_agreed: u64,
    /// Agreed duration in seconds.
    pub duration_secs: u64,
    /// Agreed price in tokens.
    pub price_tokens: u64,
    /// Current lifecycle status.
    pub status: AgreementStatus,
    /// Unix timestamp when the agreement was created.
    pub created_at: u64,
    /// Unix timestamp when the agreement was completed/cancelled (if applicable).
    pub completed_at: Option<u64>,
}

// ── AgreementAcceptance ───────────────────────────────────────────────────────

/// Broadcast by a requester to accept a [`StorageOffer`].
///
/// Published on the same `billpouch/v1/{network_id}/offers` gossip topic
/// so the offerer receives it and can activate the agreement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgreementAcceptance {
    /// `"acceptance"` tag so the offerer can distinguish from a fresh offer.
    pub kind: String,
    /// Agreement ID created by the requester.
    pub agreement_id: String,
    /// Originating offer ID.
    pub offer_id: String,
    /// Fingerprint of the requester.
    pub requester_fingerprint: String,
    /// Network scoped to.
    pub network_id: String,
    /// Unix timestamp.
    pub accepted_at: u64,
}

// ── AgreementStore ────────────────────────────────────────────────────────────

/// In-memory store for local agreements and received remote offers.
///
/// Persisted to disk as `agreements.json` inside the BillPouch data directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgreementStore {
    /// Agreements this node is a party to (either as offerer or requester).
    pub agreements: Vec<StorageAgreement>,
    /// Storage offers received from remote peers via gossip.
    pub received_offers: Vec<StorageOffer>,
}

impl AgreementStore {
    /// Load from disk; returns an empty store if the file is missing or corrupt.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to disk as pretty JSON.
    pub fn save(&self, path: &Path) -> BpResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).map_err(BpError::Io)
    }

    /// Record a new agreement (created locally when accepting an offer).
    pub fn add_agreement(&mut self, agreement: StorageAgreement) {
        // Deduplicate by id.
        if !self.agreements.iter().any(|a| a.id == agreement.id) {
            self.agreements.push(agreement);
        }
    }

    /// Upsert a received remote offer (deduplicated by id).
    pub fn upsert_offer(&mut self, offer: StorageOffer) {
        if let Some(pos) = self.received_offers.iter().position(|o| o.id == offer.id) {
            self.received_offers[pos] = offer;
        } else {
            self.received_offers.push(offer);
        }
    }

    /// All agreements for a given network.
    pub fn agreements_for_network(&self, network_id: &str) -> Vec<&StorageAgreement> {
        self.agreements
            .iter()
            .filter(|a| a.network_id == network_id)
            .collect()
    }

    /// All received offers for a given network.
    pub fn offers_for_network(&self, network_id: &str) -> Vec<&StorageOffer> {
        self.received_offers
            .iter()
            .filter(|o| o.network_id == network_id)
            .collect()
    }

    /// Mark an agreement as `Active` (called when offerer confirms acceptance).
    pub fn activate(&mut self, agreement_id: &str) {
        if let Some(a) = self.agreements.iter_mut().find(|a| a.id == agreement_id) {
            a.status = AgreementStatus::Active;
        }
    }

    /// Mark an agreement as `Cancelled`.
    pub fn cancel(&mut self, agreement_id: &str) {
        let now = chrono::Utc::now().timestamp() as u64;
        if let Some(a) = self.agreements.iter_mut().find(|a| a.id == agreement_id) {
            a.status = AgreementStatus::Cancelled;
            a.completed_at = Some(now);
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_offer(id: &str, network_id: &str) -> StorageOffer {
        StorageOffer {
            id: id.to_string(),
            offerer_fingerprint: "abc12345".to_string(),
            offerer_alias: "alice".to_string(),
            network_id: network_id.to_string(),
            bytes_offered: 1_073_741_824,
            duration_secs: 86_400,
            price_tokens: 0,
            announced_at: 1_710_000_000,
        }
    }

    fn sample_agreement(id: &str, offer_id: &str, network_id: &str) -> StorageAgreement {
        StorageAgreement {
            id: id.to_string(),
            offer_id: offer_id.to_string(),
            offerer_fingerprint: "abc12345".to_string(),
            requester_fingerprint: "def67890".to_string(),
            network_id: network_id.to_string(),
            bytes_agreed: 1_073_741_824,
            duration_secs: 86_400,
            price_tokens: 0,
            status: AgreementStatus::Pending,
            created_at: 1_710_000_100,
            completed_at: None,
        }
    }

    #[test]
    fn offer_topic_name() {
        assert_eq!(
            StorageOffer::topic_name("amici"),
            "billpouch/v1/amici/offers"
        );
    }

    #[test]
    fn upsert_offer_dedup() {
        let mut store = AgreementStore::default();
        store.upsert_offer(sample_offer("o1", "public"));
        store.upsert_offer(sample_offer("o1", "public")); // duplicate
        assert_eq!(store.received_offers.len(), 1);
    }

    #[test]
    fn add_agreement_dedup() {
        let mut store = AgreementStore::default();
        store.add_agreement(sample_agreement("a1", "o1", "public"));
        store.add_agreement(sample_agreement("a1", "o1", "public")); // duplicate
        assert_eq!(store.agreements.len(), 1);
    }

    #[test]
    fn filter_by_network() {
        let mut store = AgreementStore::default();
        store.upsert_offer(sample_offer("o1", "amici"));
        store.upsert_offer(sample_offer("o2", "lavoro"));
        assert_eq!(store.offers_for_network("amici").len(), 1);
        assert_eq!(store.offers_for_network("lavoro").len(), 1);
        assert_eq!(store.offers_for_network("public").len(), 0);
    }

    #[test]
    fn activate_and_cancel() {
        let mut store = AgreementStore::default();
        store.add_agreement(sample_agreement("a1", "o1", "public"));
        store.activate("a1");
        assert_eq!(store.agreements[0].status, AgreementStatus::Active);
        store.cancel("a1");
        assert_eq!(store.agreements[0].status, AgreementStatus::Cancelled);
        assert!(store.agreements[0].completed_at.is_some());
    }

    #[test]
    fn roundtrip_json() {
        let mut store = AgreementStore::default();
        store.upsert_offer(sample_offer("o1", "amici"));
        store.add_agreement(sample_agreement("a1", "o1", "amici"));

        let path = std::env::temp_dir().join(format!(
            "bp_agreement_test_{}.json",
            uuid::Uuid::new_v4()
        ));
        store.save(&path).unwrap();

        let loaded = AgreementStore::load(&path);
        assert_eq!(loaded.agreements.len(), 1);
        assert_eq!(loaded.received_offers.len(), 1);
        let _ = std::fs::remove_file(&path);
    }
}

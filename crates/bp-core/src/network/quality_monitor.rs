//! Network quality monitor — Ping and Proof-of-Storage challenge loops.
//!
//! ## What it does
//!
//! The monitor runs as a background Tokio task spawned by the daemon.
//!
//! **Ping loop (every 60 s)**:
//! 1. Reads the live [`NetworkState`] to enumerate all known Pouch peers.
//! 2. Sends a [`NetworkCommand::Ping`] for each Pouch (fire-and-forget per peer).
//! 3. Each Ping waits up to 5 seconds for a Pong from the network loop.
//! 4. On success: records the RTT into [`QosRegistry`] via
//!    [`crate::network::qos::PeerQos::record_ping`] and
//!    [`crate::network::qos::PeerQos::record_challenge`].
//! 5. On timeout: records a timeout via
//!    [`crate::network::qos::PeerQos::record_ping_timeout`] and a failed challenge.
//!
//! **Proof-of-Storage loop (every 300 s)**:
//! 1. For each Pouch peer that has entries in [`OutgoingAssignments`]:
//!    picks one random fragment ID assigned to that peer.
//! 2. Sends a [`NetworkCommand::ProofOfStorage`] challenge.
//! 3. Waits up to 10 s for a `bool` proof result.
//! 4. On success: calls [`crate::network::qos::PeerQos::record_pos_success`].
//! 5. On failure/timeout: calls
//!    [`crate::network::qos::PeerQos::record_pos_failure`] — increments
//!    `fault_score` toward the `degraded`/`suspected`/`blacklisted` thresholds.
//!
//! The `QosRegistry` populated here is consumed by `PutFile` in the control
//! server to compute adaptive `k`/`n` coding parameters.
//!
//! ## Concurrency model
//!
//! All per-peer tasks within a round are spawned concurrently so a slow or
//! dead peer does not delay the others.

use crate::{
    network::{qos::QosRegistry, state::NetworkState, NetworkCommand, OutgoingAssignments},
    service::ServiceType,
};
use libp2p::PeerId;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

// ── Configuration constants ───────────────────────────────────────────────────

/// How often (in seconds) a full Ping round is run against all known Pouches.
const PING_INTERVAL_SECS: u64 = 60;

/// Maximum time (in seconds) to wait for a Pong before declaring a timeout.
const PING_TIMEOUT_SECS: u64 = 5;

/// How often (in seconds) a Proof-of-Storage round is run.
const POS_INTERVAL_SECS: u64 = 300;

/// Maximum time (in seconds) to wait for a PoS proof response.
const POS_TIMEOUT_SECS: u64 = 10;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the quality monitor loop indefinitely.
///
/// Spawned by [`crate::daemon::run_daemon`] as a background Tokio task.
///
/// # Arguments
///
/// - `network_state`        — shared view of all known peers (read-only here).
/// - `qos`                  — shared QoS registry updated after every challenge.
/// - `net_tx`               — channel to submit network commands.
/// - `outgoing_assignments` — map of `peer_id → fragments pushed to that peer`.
pub async fn run_quality_monitor(
    network_state: Arc<RwLock<NetworkState>>,
    qos: Arc<RwLock<QosRegistry>>,
    net_tx: mpsc::Sender<NetworkCommand>,
    outgoing_assignments: OutgoingAssignments,
) {
    let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
    // Skip the first (immediate) tick so the monitor waits a full interval
    // after daemon startup before the first round.
    ping_interval.reset();

    let mut pos_interval = tokio::time::interval(Duration::from_secs(POS_INTERVAL_SECS));
    pos_interval.reset();

    tracing::info!(
        ping_interval_secs = PING_INTERVAL_SECS,
        pos_interval_secs = POS_INTERVAL_SECS,
        "Quality monitor started"
    );

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                run_ping_round(&network_state, &qos, &net_tx).await;
            }
            _ = pos_interval.tick() => {
                run_pos_round(&outgoing_assignments, &qos, &net_tx).await;
            }
        }
    }
}

// ── Ping round ────────────────────────────────────────────────────────────────

async fn run_ping_round(
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    // Collect Pouch peer IDs from the current NetworkState snapshot.
    let pouch_peers: Vec<(String, PeerId)> = {
        let ns = match network_state.read() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("quality_monitor: NetworkState lock poisoned: {e}");
                return;
            }
        };
        ns.all()
            .into_iter()
            .filter(|n| n.service_type == ServiceType::Pouch)
            .filter_map(|n| {
                n.peer_id
                    .parse::<PeerId>()
                    .ok()
                    .map(|pid| (n.peer_id.clone(), pid))
            })
            .collect()
    };

    if pouch_peers.is_empty() {
        tracing::debug!("quality_monitor: no Pouch peers known, skipping Ping round");
        return;
    }

    tracing::debug!(peers = pouch_peers.len(), "quality_monitor: starting Ping round");

    let mut handles = Vec::with_capacity(pouch_peers.len());
    for (peer_id_str, peer_id) in pouch_peers {
        let qos = Arc::clone(qos);
        let net_tx = net_tx.clone();
        let handle = tokio::spawn(async move {
            ping_one_peer(peer_id_str, peer_id, qos, net_tx).await;
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }

    tracing::debug!("quality_monitor: Ping round complete");
}

// ── PoS round ─────────────────────────────────────────────────────────────────

async fn run_pos_round(
    outgoing_assignments: &OutgoingAssignments,
    qos: &Arc<RwLock<QosRegistry>>,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    // Snapshot the current assignments (peer_id → random fragment to challenge).
    let challenges: Vec<(String, PeerId, String, String)> = {
        let assignments = match outgoing_assignments.read() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("quality_monitor: outgoing_assignments lock poisoned: {e}");
                return;
            }
        };
        assignments
            .iter()
            .filter_map(|(peer_id_str, fragments)| {
                if fragments.is_empty() {
                    return None;
                }
                // Pick a random fragment index using a simple modular approach.
                let idx = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as usize
                    % fragments.len();
                let frag = &fragments[idx];
                peer_id_str
                    .parse::<PeerId>()
                    .ok()
                    .map(|pid| (peer_id_str.clone(), pid, frag.chunk_id.clone(), frag.fragment_id.clone()))
            })
            .collect()
    };

    if challenges.is_empty() {
        tracing::debug!("quality_monitor: no outgoing fragments, skipping PoS round");
        return;
    }

    tracing::debug!(
        peers = challenges.len(),
        "quality_monitor: starting PoS round"
    );

    let mut handles = Vec::with_capacity(challenges.len());
    for (peer_id_str, peer_id, chunk_id, fragment_id) in challenges {
        let qos = Arc::clone(qos);
        let net_tx = net_tx.clone();
        let handle = tokio::spawn(async move {
            pos_one_peer(peer_id_str, peer_id, chunk_id, fragment_id, qos, net_tx).await;
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }

    tracing::debug!("quality_monitor: PoS round complete");
}

// ── Per-peer PoS helper ───────────────────────────────────────────────────────

async fn pos_one_peer(
    peer_id_str: String,
    peer_id: PeerId,
    chunk_id: String,
    fragment_id: String,
    qos: Arc<RwLock<QosRegistry>>,
    net_tx: mpsc::Sender<NetworkCommand>,
) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<bool>();

    if net_tx
        .send(NetworkCommand::ProofOfStorage {
            peer_id,
            chunk_id: chunk_id.clone(),
            fragment_id: fragment_id.clone(),
            nonce,
            resp_tx,
        })
        .await
        .is_err()
    {
        tracing::warn!(peer = %peer_id_str, "quality_monitor: net_tx closed (PoS)");
        return;
    }

    match tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), resp_rx).await {
        Ok(Ok(true)) => {
            tracing::debug!(
                peer = %peer_id_str,
                chunk = %chunk_id,
                frag = %fragment_id,
                "PoS challenge: proof received"
            );
            if let Ok(mut qos_guard) = qos.write() {
                qos_guard.entry(peer_id_str).record_pos_success();
            }
        }
        Ok(Ok(false)) | Ok(Err(_)) => {
            tracing::debug!(
                peer = %peer_id_str,
                chunk = %chunk_id,
                frag = %fragment_id,
                "PoS challenge: proof failed or fragment not found"
            );
            record_pos_failure(&peer_id_str, &qos);
        }
        Err(_) => {
            tracing::debug!(
                peer = %peer_id_str,
                "PoS challenge: timeout (>{POS_TIMEOUT_SECS}s)"
            );
            record_pos_failure(&peer_id_str, &qos);
        }
    }
}

fn record_pos_failure(peer_id_str: &str, qos: &Arc<RwLock<QosRegistry>>) {
    if let Ok(mut qos_guard) = qos.write() {
        qos_guard.entry(peer_id_str).record_pos_failure();
    }
}

async fn ping_one_peer(
    peer_id_str: String,
    peer_id: PeerId,
    qos: Arc<RwLock<QosRegistry>>,
    net_tx: mpsc::Sender<NetworkCommand>,
) {
    let sent_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<u64>();

    // Send the Ping command to the network loop.
    if net_tx
        .send(NetworkCommand::Ping {
            peer_id,
            sent_at_ms,
            resp_tx,
        })
        .await
        .is_err()
    {
        tracing::warn!(peer = %peer_id_str, "quality_monitor: net_tx closed");
        return;
    }

    // Wait for the Pong (RTT in ms) or time out.
    match tokio::time::timeout(Duration::from_secs(PING_TIMEOUT_SECS), resp_rx).await {
        Ok(Ok(rtt_ms)) => {
            tracing::debug!(peer = %peer_id_str, rtt_ms = %rtt_ms, "Ping OK");
            if let Ok(mut qos_guard) = qos.write() {
                let entry = qos_guard.entry(peer_id_str.clone());
                entry.record_ping(rtt_ms as f64);
                entry.record_challenge(true);
            }
        }
        Ok(Err(_)) => {
            // Oneshot dropped by network loop (peer unreachable / send failed).
            tracing::debug!(peer = %peer_id_str, "Ping: no response (channel dropped)");
            record_timeout(&peer_id_str, &qos);
        }
        Err(_) => {
            tracing::debug!(peer = %peer_id_str, "Ping: timeout (>{PING_TIMEOUT_SECS}s)");
            record_timeout(&peer_id_str, &qos);
        }
    }
}

fn record_timeout(peer_id_str: &str, qos: &Arc<RwLock<QosRegistry>>) {
    if let Ok(mut qos_guard) = qos.write() {
        let entry = qos_guard.entry(peer_id_str);
        entry.record_ping_timeout();
        entry.record_challenge(false);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::OutgoingFragment;

    #[test]
    fn record_timeout_degrades_score() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        // Prime with a good ping.
        {
            let mut g = qos.write().unwrap();
            let e = g.entry("p1");
            e.record_ping(10.0);
            e.record_challenge(true);
        }
        let score_before = qos.read().unwrap().stability_score("p1");

        // Record several timeouts.
        for _ in 0..5 {
            record_timeout("p1", &qos);
        }
        let score_after = qos.read().unwrap().stability_score("p1");
        assert!(score_after < score_before, "timeout should degrade score");
    }

    #[test]
    fn new_peer_gets_timeout_entry() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        record_timeout("new-peer", &qos);
        let score = qos.read().unwrap().stability_score("new-peer");
        // After one timeout: challenge_success < 1.0, latency still 0.0
        // → stability < 0.4 (pure reliability component capped by β=0.10)
        assert!(
            score < 0.95,
            "new peer after one timeout should have degraded score"
        );
    }

    #[test]
    fn pos_failure_increments_fault_score() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        // Initial fault score is 0.
        assert_eq!(qos.read().unwrap().fault_score("p1"), 0);

        for _ in 0..3 {
            record_pos_failure("p1", &qos);
        }
        let fault = qos.read().unwrap().fault_score("p1");
        assert!(fault > 0, "fault score should increase after PoS failures");
    }

    #[test]
    fn pos_success_decays_fault_score() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        // Prime with some failures.
        for _ in 0..4 {
            record_pos_failure("p1", &qos);
        }
        let fault_after_failures = qos.read().unwrap().fault_score("p1");

        // A success should decay the fault score.
        {
            let mut g = qos.write().unwrap();
            g.entry("p1").record_pos_success();
        }
        let fault_after_success = qos.read().unwrap().fault_score("p1");
        assert!(
            fault_after_success < fault_after_failures,
            "PoS success should decay fault score"
        );
    }

    /// Ensure `run_quality_monitor` can be started and does not panic
    /// immediately when there are no peers and no assignments.
    #[tokio::test]
    async fn monitor_noop_with_no_peers() {
        let ns = Arc::new(RwLock::new(NetworkState::new()));
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        let (net_tx, _net_rx) = mpsc::channel(8);
        let outgoing = Arc::new(RwLock::new(std::collections::HashMap::new()));

        // Run the monitor for a very short time — it should not panic.
        tokio::time::timeout(
            Duration::from_millis(200),
            run_quality_monitor(ns, qos, net_tx, outgoing),
        )
        .await
        .ok(); // timeout is expected
    }

    #[test]
    fn outgoing_assignment_structure() {
        // Verify we can build OutgoingAssignments and look up entries.
        let outgoing: OutgoingAssignments =
            Arc::new(RwLock::new(std::collections::HashMap::new()));
        {
            let mut g = outgoing.write().unwrap();
            g.entry("peer-1".into()).or_default().push(OutgoingFragment {
                chunk_id: "chunk-a".into(),
                fragment_id: "frag-1".into(),
            });
        }
        let g = outgoing.read().unwrap();
        assert_eq!(g.get("peer-1").unwrap().len(), 1);
        assert_eq!(g.get("peer-1").unwrap()[0].chunk_id, "chunk-a");
    }
}

//! Network quality monitor — periodic Ping challenge loop.
//!
//! ## What it does
//!
//! The monitor runs as a background Tokio task spawned by the daemon.
//! Every 60 seconds it:
//!
//! 1. Reads the live [`NetworkState`] to enumerate all known Pouch peers.
//! 2. Sends a [`NetworkCommand::Ping`] for each Pouch (fire-and-forget per peer).
//! 3. Each Ping waits up to 5 seconds for a Pong from the network loop.
//! 4. On success: records the RTT into [`QosRegistry`] via
//!    [`crate::network::qos::PeerQos::record_ping`] and
//!    [`crate::network::qos::PeerQos::record_challenge`].
//! 5. On timeout: records a timeout via
//!    [`crate::network::qos::PeerQos::record_ping_timeout`] and a failed challenge.
//!
//! The `QosRegistry` populated here is consumed by `PutFile` in the control
//! server to compute adaptive `k`/`n` coding parameters.
//!
//! ## Concurrency model
//!
//! All Ping requests for a given round are spawned concurrently as individual
//! Tokio tasks, so a slow or dead peer does not delay the others.
//! The next round timer starts only after the previous one has fully completed.

use crate::{
    network::{qos::QosRegistry, state::NetworkState, NetworkCommand},
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

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the quality monitor loop indefinitely.
///
/// Spawned by [`crate::daemon::run_daemon`] as a background Tokio task.
///
/// # Arguments
///
/// - `network_state` — shared view of all known peers (read-only here).
/// - `qos`           — shared QoS registry updated after every Ping.
/// - `net_tx`        — channel to submit `NetworkCommand::Ping` requests.
pub async fn run_quality_monitor(
    network_state: Arc<RwLock<NetworkState>>,
    qos: Arc<RwLock<QosRegistry>>,
    net_tx: mpsc::Sender<NetworkCommand>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
    // Skip the first (immediate) tick so the monitor waits a full interval
    // after daemon startup before the first round.
    interval.reset();

    tracing::info!(
        interval_secs = PING_INTERVAL_SECS,
        timeout_secs = PING_TIMEOUT_SECS,
        "Quality monitor started"
    );

    loop {
        interval.tick().await;

        // Collect Pouch peer IDs from the current NetworkState snapshot.
        let pouch_peers: Vec<(String, PeerId)> = {
            let ns = match network_state.read() {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("quality_monitor: NetworkState lock poisoned: {e}");
                    continue;
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
            tracing::debug!("quality_monitor: no Pouch peers known, skipping round");
            continue;
        }

        tracing::debug!(
            peers = pouch_peers.len(),
            "quality_monitor: starting Ping round"
        );

        // Spawn one task per peer so timeouts are independent.
        let mut handles = Vec::with_capacity(pouch_peers.len());
        for (peer_id_str, peer_id) in pouch_peers {
            let qos = Arc::clone(&qos);
            let net_tx = net_tx.clone();
            let handle = tokio::spawn(async move {
                ping_one_peer(peer_id_str, peer_id, qos, net_tx).await;
            });
            handles.push(handle);
        }

        // Await all so the next round starts cleanly.
        for h in handles {
            let _ = h.await;
        }

        tracing::debug!("quality_monitor: Ping round complete");
    }
}

// ── Per-peer Ping helper ──────────────────────────────────────────────────────

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

    /// Ensure `run_quality_monitor` can be started and does not panic
    /// immediately when there are no peers.
    #[tokio::test]
    async fn monitor_noop_with_no_peers() {
        let ns = Arc::new(RwLock::new(NetworkState::new()));
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        let (net_tx, _net_rx) = mpsc::channel(8);

        // Run the monitor for a very short time — it should not panic.
        tokio::time::timeout(
            Duration::from_millis(200),
            run_quality_monitor(ns, qos, net_tx),
        )
        .await
        .ok(); // timeout is expected
    }
}

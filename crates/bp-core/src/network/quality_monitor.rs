use crate::{
    chain::{observe, ObservationLog},
    coding::rlnc,
    coding::rlnc::EncodedFragment,
    network::{
        qos::QosRegistry, state::NetworkState, FragmentResponse, NetworkCommand,
        OutgoingAssignments, FAULT_DEGRADED, FAULT_SUSPECTED,
    },
    service::ServiceType,
};
use libp2p::PeerId;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const PING_INTERVAL_SECS: u64 = 60;

const PING_TIMEOUT_SECS: u64 = 5;

const POS_INTERVAL_SECS: u64 = 300;

const POS_TIMEOUT_SECS: u64 = 10;

pub async fn run_quality_monitor(
    network_state: Arc<RwLock<NetworkState>>,
    qos: Arc<RwLock<QosRegistry>>,
    observations: ObservationLog,
    net_tx: mpsc::Sender<NetworkCommand>,
    outgoing_assignments: OutgoingAssignments,
) {
    let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
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
                run_pos_round(&outgoing_assignments, &network_state, &qos, &observations, &net_tx).await;
            }
        }
    }
}

async fn run_ping_round(
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
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

    tracing::debug!(
        peers = pouch_peers.len(),
        "quality_monitor: starting Ping round"
    );

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

async fn run_pos_round(
    outgoing_assignments: &OutgoingAssignments,
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    observations: &ObservationLog,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
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
                let idx = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as usize
                    % fragments.len();
                let frag = &fragments[idx];
                peer_id_str.parse::<PeerId>().ok().map(|pid| {
                    (
                        peer_id_str.clone(),
                        pid,
                        frag.chunk_id.clone(),
                        frag.fragment_id.clone(),
                    )
                })
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
    for (peer_id_str, peer_id, chunk_id, fragment_id) in challenges.iter().cloned() {
        let qos = Arc::clone(qos);
        let observations = Arc::clone(observations);
        let net_tx = net_tx.clone();
        let handle = tokio::spawn(async move {
            let passed = pos_one_peer(
                peer_id_str.clone(),
                peer_id,
                chunk_id,
                fragment_id,
                &qos,
                net_tx,
            )
            .await;
            if let Some(passed) = passed {
                observe(&observations, &peer_id_str, passed, crate::chain::now_ms());
            }
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }

    tracing::debug!("quality_monitor: PoS round complete");

    let suspected: Vec<(String, PeerId)> = challenges
        .iter()
        .filter(|(peer_id_str, _, _, _)| {
            qos.read()
                .map(|g| g.fault_score(peer_id_str) >= FAULT_SUSPECTED)
                .unwrap_or(false)
        })
        .map(|(peer_id_str, peer_id, _, _)| (peer_id_str.clone(), *peer_id))
        .fold(Vec::<(String, PeerId)>::new(), |mut acc, (s, p)| {
            if !acc.iter().any(|(x, _)| x == &s) {
                acc.push((s, p));
            }
            acc
        });

    for (peer_id_str, peer_id) in suspected {
        reroute_suspected_peer(
            &peer_id_str,
            peer_id,
            outgoing_assignments,
            network_state,
            qos,
            net_tx,
        )
        .await;
    }
}

async fn reroute_suspected_peer(
    suspected_peer_str: &str,
    suspected_peer: PeerId,
    outgoing: &OutgoingAssignments,
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let chunks: Vec<(String, crate::files::Grant)> = {
        let guard = match outgoing.read() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard
            .get(suspected_peer_str)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|f| (f.chunk_id.clone(), f.grant.clone()))
            .collect::<std::collections::HashMap<_, _>>()
            .into_iter()
            .collect()
    };

    if chunks.is_empty() {
        tracing::debug!(
            peer = %suspected_peer_str,
            "reroute: no assigned chunks, nothing to reroute"
        );
        return;
    }

    let healthy_peer: Option<(String, PeerId)> = {
        let ns = match network_state.read() {
            Ok(g) => g,
            Err(_) => return,
        };
        let qos_g = match qos.read() {
            Ok(g) => g,
            Err(_) => return,
        };
        ns.all()
            .into_iter()
            .filter(|n| n.service_type == ServiceType::Pouch)
            .filter(|n| n.peer_id != suspected_peer_str)
            .filter(|n| qos_g.fault_score(&n.peer_id) < FAULT_DEGRADED)
            .filter_map(|n| {
                n.peer_id
                    .parse::<PeerId>()
                    .ok()
                    .map(|pid| (n.peer_id.clone(), pid))
            })
            .next()
    };

    let (healthy_str, healthy_pid) = match healthy_peer {
        Some(p) => p,
        None => {
            tracing::warn!(
                peer = %suspected_peer_str,
                "reroute: no healthy peer available, aborting rerouting"
            );
            return;
        }
    };

    tracing::info!(
        suspected = %suspected_peer_str,
        healthy   = %healthy_str,
        chunks    = chunks.len(),
        "reroute: starting preventive fragment rerouting"
    );

    for (chunk_id, grant) in chunks {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if net_tx
            .send(NetworkCommand::FetchChunkFragments {
                peer_id: suspected_peer,
                chunk_id: chunk_id.clone(),
                grant: grant.clone(),
                resp_tx: tx,
            })
            .await
            .is_err()
        {
            tracing::debug!(chunk = %chunk_id, "reroute: net_tx closed");
            continue;
        }

        match tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), rx).await {
            Ok(Ok(FragmentResponse::FoundMany { fragments })) if !fragments.is_empty() => {
                let encoded: Vec<EncodedFragment> = fragments
                    .iter()
                    .filter_map(|(fid, bytes)| {
                        EncodedFragment::from_bytes(fid.clone(), chunk_id.clone(), bytes).ok()
                    })
                    .collect();

                if encoded.is_empty() {
                    tracing::debug!(chunk = %chunk_id, "reroute: all fragments unparseable");
                    continue;
                }

                match rlnc::recode(&encoded, 1) {
                    Ok(mut recoded) => {
                        if let Some(new_frag) = recoded.pop() {
                            let frag_id = new_frag.id.clone();
                            let _ = net_tx
                                .send(NetworkCommand::PushFragment {
                                    peer_id: healthy_pid,
                                    chunk_id: chunk_id.clone(),
                                    fragment_id: frag_id.clone(),
                                    data: new_frag.to_bytes(),
                                    grant: grant.clone(),
                                })
                                .await;

                            tracing::info!(
                                chunk   = %chunk_id,
                                healthy = %healthy_str,
                                "reroute: recoded fragment pushed to healthy peer"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            chunk = %chunk_id,
                            err   = %e,
                            "reroute: recode failed"
                        );
                    }
                }
            }
            _ => {
                tracing::debug!(
                    chunk = %chunk_id,
                    peer  = %suspected_peer_str,
                    "reroute: fragment fetch failed or timed out"
                );
            }
        }
    }
}

async fn pos_one_peer(
    peer_id_str: String,
    peer_id: PeerId,
    chunk_id: String,
    fragment_id: String,
    qos: &Arc<RwLock<QosRegistry>>,
    net_tx: mpsc::Sender<NetworkCommand>,
) -> Option<bool> {
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
        return None;
    }

    let passed = matches!(
        tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), resp_rx).await,
        Ok(Ok(true))
    );
    tracing::debug!(
        peer = %peer_id_str,
        chunk = %chunk_id,
        frag = %fragment_id,
        passed,
        "PoS challenge result"
    );
    if passed {
        if let Ok(mut qos_guard) = qos.write() {
            qos_guard.entry(peer_id_str).record_pos_success();
        }
    } else {
        record_pos_failure(&peer_id_str, qos);
    }
    Some(passed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_timeout_degrades_score() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        {
            let mut g = qos.write().unwrap();
            let e = g.entry("p1");
            e.record_ping(10.0);
            e.record_challenge(true);
        }
        let score_before = qos.read().unwrap().stability_score("p1");

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
        assert!(
            score < 0.95,
            "new peer after one timeout should have degraded score"
        );
    }

    #[test]
    fn pos_failure_increments_fault_score() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
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
        for _ in 0..4 {
            record_pos_failure("p1", &qos);
        }
        let fault_after_failures = qos.read().unwrap().fault_score("p1");

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

    #[tokio::test]
    async fn monitor_noop_with_no_peers() {
        let ns = Arc::new(RwLock::new(NetworkState::new()));
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        let (net_tx, _net_rx) = mpsc::channel(8);
        let outgoing = Arc::new(RwLock::new(std::collections::HashMap::new()));

        tokio::time::timeout(
            Duration::from_millis(200),
            run_quality_monitor(ns, qos, Default::default(), net_tx, outgoing),
        )
        .await
        .ok();
    }
}

use crate::{
    chain::{observe, ObservationLog},
    network::{
        qos::QosRegistry, state::NetworkState, Fillers, NetworkCommand, OutgoingAssignments,
        StorageManagerMap,
    },
    service::ServiceType,
    storage::{
        filler::{Sponsored, BLOB_BLOCKS, BLOB_BYTES},
        merkle::{self, Proof},
        PosTarget, StorageTier,
    },
};
use libp2p::PeerId;
use rand::{seq::SliceRandom, Rng};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const PING_INTERVAL_SECS: u64 = 60;

const PING_TIMEOUT_SECS: u64 = 5;

const POS_INTERVAL_SECS: u64 = 300;

const POS_TIMEOUT_SECS: u64 = 10;

const FILL_INTERVAL_SECS: u64 = 60;

const MAX_BLOBS_PER_ROUND: u64 = 256;

const CATCH_UP_INTERVAL_SECS: u64 = 60;

const MAX_REPAIRS_PER_ROUND: usize = 64;

#[allow(clippy::too_many_arguments)]
pub async fn run_quality_monitor(
    me: String,
    network_state: Arc<RwLock<NetworkState>>,
    qos: Arc<RwLock<QosRegistry>>,
    observations: ObservationLog,
    net_tx: mpsc::Sender<NetworkCommand>,
    outgoing_assignments: OutgoingAssignments,
    storage_managers: StorageManagerMap,
    fillers: Fillers,
) {
    let mut fill_interval = tokio::time::interval(Duration::from_secs(FILL_INTERVAL_SECS));
    fill_interval.reset();

    let mut ping_interval = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
    ping_interval.reset();

    let mut pos_interval = tokio::time::interval(Duration::from_secs(POS_INTERVAL_SECS));
    pos_interval.reset();

    let mut catch_up_interval = tokio::time::interval(Duration::from_secs(CATCH_UP_INTERVAL_SECS));
    catch_up_interval.reset();

    tracing::info!(
        ping_interval_secs = PING_INTERVAL_SECS,
        pos_interval_secs = POS_INTERVAL_SECS,
        "Quality monitor started"
    );

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                run_ping_round(&network_state, &qos, &net_tx).await;
                crate::network::save_outgoing(&outgoing_assignments);
                if let Ok(q) = qos.read() {
                    q.save();
                }
            }
            _ = pos_interval.tick() => {
                run_pos_round(&outgoing_assignments, &fillers, &qos, &observations, &net_tx).await;
            }
            _ = fill_interval.tick() => {
                run_fill_round(&me, &network_state, &storage_managers, &fillers, &net_tx).await;
            }
            _ = catch_up_interval.tick() => {
                run_catch_up(&me, &network_state, &storage_managers, &net_tx).await;
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

enum Expect {
    Merkle { root: [u8; 32], blocks: usize },
    Filler(Box<Sponsored>),
}

struct Challenge {
    peer: String,
    peer_id: PeerId,
    target: PosTarget,
    index: usize,
    expect: Expect,
}

impl Challenge {
    fn passes(&self, proof: &Proof) -> bool {
        proof.index == self.index
            && match &self.expect {
                Expect::Merkle { root, blocks } => merkle::verify(root, *blocks, proof),
                Expect::Filler(sponsored) => sponsored.verify(proof),
            }
    }
}

fn challenges(outgoing: &OutgoingAssignments, fillers: &Fillers) -> Vec<Challenge> {
    let mut rng = rand::thread_rng();
    let mut out = Vec::new();
    if let Ok(assignments) = outgoing.read() {
        for (peer, fragments) in assignments.iter() {
            let (Some(f), Ok(peer_id)) = (fragments.choose(&mut rng), peer.parse()) else {
                continue;
            };
            out.push(Challenge {
                peer: peer.clone(),
                peer_id,
                target: PosTarget::Fragment {
                    chunk_id: f.chunk_id.clone(),
                    fragment_id: f.fragment_id.clone(),
                },
                index: rng.gen_range(0..f.blocks),
                expect: Expect::Merkle {
                    root: f.root,
                    blocks: f.blocks,
                },
            });
        }
    }
    if let Ok(book) = fillers.read() {
        for (peer, blobs) in &book.by_pouch {
            let (Some(blob), Ok(peer_id)) = (blobs.choose(&mut rng), peer.parse()) else {
                continue;
            };
            out.push(Challenge {
                peer: peer.clone(),
                peer_id,
                target: PosTarget::Filler {
                    blob_id: blob.blob_id.clone(),
                },
                index: rng.gen_range(0..BLOB_BLOCKS),
                expect: Expect::Filler(Box::new(blob.clone())),
            });
        }
    }
    out
}

fn placed_bytes(outgoing: &OutgoingAssignments, fillers: &Fillers, peer: &str) -> u64 {
    let fragments: u64 = outgoing
        .read()
        .map(|o| o.get(peer).map_or(0, |f| f.iter().map(|f| f.size).sum()))
        .unwrap_or(0);
    let filler = fillers.read().map_or(0, |b| b.bytes_for(peer));
    fragments + filler
}

async fn run_pos_round(
    outgoing_assignments: &OutgoingAssignments,
    fillers: &Fillers,
    qos: &Arc<RwLock<QosRegistry>>,
    observations: &ObservationLog,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let mut handles = Vec::new();
    for challenge in challenges(outgoing_assignments, fillers) {
        let net_tx = net_tx.clone();
        handles.push(tokio::spawn(async move {
            let passed = prove_one(&challenge, net_tx).await;
            (challenge.peer, passed)
        }));
    }
    for handle in handles {
        let Ok((peer, Some(passed))) = handle.await else {
            continue;
        };
        tracing::debug!(peer = %peer, passed, "PoS challenge result");
        if let Ok(mut q) = qos.write() {
            q.entry(peer.clone()).record_challenge(passed);
        }
        let placed = placed_bytes(outgoing_assignments, fillers, &peer);
        observe(observations, &peer, passed, placed, crate::chain::now_ms());
    }
}

fn layer_peers(
    network_state: &Arc<RwLock<NetworkState>>,
    network: &str,
    layer: StorageTier,
    me: &str,
) -> Vec<String> {
    network_state
        .read()
        .map(|ns| {
            ns.in_network(network)
                .into_iter()
                .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != me)
                .filter(|n| n.metadata.get("evicting").and_then(|v| v.as_bool()) != Some(true))
                .filter(|n| {
                    n.metadata
                        .get("tier")
                        .and_then(|t| t.as_str())
                        .and_then(|t| StorageTier::parse(t).ok())
                        .is_some_and(|t| t >= layer)
                })
                .map(|n| n.peer_id.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub fn repair_route(peers: &[String], k: usize, me: &str) -> Option<Vec<String>> {
    if peers.len() < k {
        return None;
    }
    let mut route: Vec<String> = peers
        .choose_multiple(&mut rand::thread_rng(), k)
        .cloned()
        .collect();
    route.push(me.to_string());
    Some(route)
}

async fn run_catch_up(
    me: &str,
    network_state: &Arc<RwLock<NetworkState>>,
    storage_managers: &StorageManagerMap,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let pouches: Vec<(
        String,
        StorageTier,
        Arc<RwLock<crate::storage::StorageManager>>,
    )> = storage_managers
        .read()
        .map(|m| {
            m.values()
                .map(|sm| {
                    let meta = &sm.read().unwrap().meta;
                    (meta.network_id.clone(), meta.tier, Arc::clone(sm))
                })
                .collect()
        })
        .unwrap_or_default();
    for (network, tier, sm) in pouches {
        for layer in tier.layers() {
            let peers = layer_peers(network_state, &network, layer, me);
            let Some(source) = peers
                .choose(&mut rand::thread_rng())
                .and_then(|p| p.parse::<PeerId>().ok())
            else {
                continue;
            };
            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            let sent = net_tx
                .send(NetworkCommand::Catalog {
                    peer_id: source,
                    network: network.clone(),
                    layer,
                    resp_tx,
                })
                .await;
            if sent.is_err() {
                continue;
            }
            let Ok(Ok(catalog)) =
                tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), resp_rx).await
            else {
                continue;
            };
            let missing: Vec<_> = {
                let sm = sm.read().unwrap();
                catalog
                    .into_iter()
                    .filter(|entry| !sm.has_chunk(&entry.chunk_id))
                    .take(MAX_REPAIRS_PER_ROUND)
                    .collect()
            };
            let mut started = 0;
            for entry in missing {
                let Some(route) = repair_route(&peers, entry.k, me) else {
                    continue;
                };
                let Ok(first) = route[0].parse::<PeerId>() else {
                    continue;
                };
                let _ = net_tx
                    .send(NetworkCommand::Recode {
                        peer_id: first,
                        chunk_id: entry.chunk_id,
                        owner: entry.owner,
                        route: route[1..].to_vec(),
                    })
                    .await;
                started += 1;
            }
            if started > 0 {
                tracing::info!(network = %network, layer = ?layer, chunks = started, "catching up: recoding missing fragments");
            }
        }
    }
}

async fn prove_one(challenge: &Challenge, net_tx: mpsc::Sender<NetworkCommand>) -> Option<bool> {
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    net_tx
        .send(NetworkCommand::ProofOfStorage {
            peer_id: challenge.peer_id,
            target: challenge.target.clone(),
            index: challenge.index,
            resp_tx,
        })
        .await
        .ok()?;
    let proof = tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), resp_rx)
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten();
    Some(proof.is_some_and(|p| challenge.passes(&p)))
}

async fn run_fill_round(
    me: &str,
    network_state: &Arc<RwLock<NetworkState>>,
    storage_managers: &StorageManagerMap,
    fillers: &Fillers,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let networks: Vec<String> = storage_managers
        .read()
        .map(|m| {
            m.values()
                .map(|sm| sm.read().unwrap().meta.network_id.clone())
                .collect()
        })
        .unwrap_or_default();
    for network in networks {
        let pouches: Vec<(String, u64)> = network_state
            .read()
            .map(|ns| {
                ns.in_network(&network)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != me)
                    .filter_map(|n| {
                        let free = n.metadata.get("free_bytes")?.as_u64()?;
                        Some((n.peer_id.clone(), free))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let sponsors = pouches.len() as u64;
        for (pouch, free) in pouches {
            let Ok(peer_id) = pouch.parse::<PeerId>() else {
                continue;
            };
            let blobs = (free / sponsors / BLOB_BYTES as u64).min(MAX_BLOBS_PER_ROUND);
            let mut filled = 0;
            for _ in 0..blobs {
                let (sponsored, data) = Sponsored::new(&network);
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                let sent = net_tx
                    .send(NetworkCommand::Fill {
                        peer_id,
                        network: network.clone(),
                        blob_id: sponsored.blob_id.clone(),
                        data,
                        resp_tx,
                    })
                    .await;
                let stored = sent.is_ok()
                    && matches!(
                        tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), resp_rx).await,
                        Ok(Ok(true))
                    );
                if !stored {
                    break;
                }
                if let Ok(mut book) = fillers.write() {
                    book.add(&pouch, sponsored);
                }
                filled += 1;
            }
            if filled > 0 {
                tracing::info!(pouch = %pouch, network = %network, blobs = filled, "filled free space");
            }
        }
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
    fn challenges_accept_only_the_right_block() {
        let data: Vec<u8> = (0..3 * merkle::BLOCK).map(|i| i as u8).collect();
        let fragment = Challenge {
            peer: "p".into(),
            peer_id: PeerId::random(),
            target: PosTarget::Fragment {
                chunk_id: "c".into(),
                fragment_id: "f".into(),
            },
            index: 1,
            expect: Expect::Merkle {
                root: merkle::root(&data),
                blocks: 3,
            },
        };
        assert!(fragment.passes(&merkle::prove(&data, 1).unwrap()));
        assert!(!fragment.passes(&merkle::prove(&data, 2).unwrap()));

        let (sponsored, blob) = Sponsored::new("amici");
        let (_, other) = Sponsored::new("amici");
        let filler = Challenge {
            peer: "p".into(),
            peer_id: PeerId::random(),
            target: PosTarget::Filler {
                blob_id: sponsored.blob_id.clone(),
            },
            index: 7,
            expect: Expect::Filler(Box::new(sponsored)),
        };
        assert!(filler.passes(&merkle::prove(&blob, 7).unwrap()));
        assert!(!filler.passes(&merkle::prove(&other, 7).unwrap()));
    }

    #[test]
    fn a_repair_route_crosses_k_distinct_holders_and_ends_here() {
        let peers: Vec<String> = (0..6).map(|i| format!("p{i}")).collect();
        let route = repair_route(&peers, 4, "me").unwrap();
        assert_eq!(route.len(), 5);
        assert_eq!(route.last().map(String::as_str), Some("me"));
        let hops: std::collections::HashSet<_> = route[..4].iter().collect();
        assert_eq!(hops.len(), 4);
        assert!(repair_route(&peers, 7, "me").is_none());
    }

    #[tokio::test]
    async fn monitor_noop_with_no_peers() {
        let ns = Arc::new(RwLock::new(NetworkState::new()));
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        let (net_tx, _net_rx) = mpsc::channel(8);
        let outgoing = Arc::new(RwLock::new(std::collections::HashMap::new()));

        tokio::time::timeout(
            Duration::from_millis(200),
            run_quality_monitor(
                "me".into(),
                ns,
                qos,
                Default::default(),
                net_tx,
                outgoing,
                Default::default(),
                Default::default(),
            ),
        )
        .await
        .ok();
    }
}

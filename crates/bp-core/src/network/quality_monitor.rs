use crate::{
    chain::{observe, ObservationLog},
    coding::rlnc,
    coding::rlnc::EncodedFragment,
    network::{
        qos::QosRegistry, state::NetworkState, Fillers, FragmentResponse, NetworkCommand,
        OutgoingAssignments, StorageManagerMap, FAULT_DEGRADED, FAULT_SUSPECTED,
    },
    service::ServiceType,
    storage::{
        filler::{Sponsored, BLOB_BLOCKS, BLOB_BYTES},
        merkle::{self, Proof},
        PosTarget,
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

    tracing::info!(
        ping_interval_secs = PING_INTERVAL_SECS,
        pos_interval_secs = POS_INTERVAL_SECS,
        "Quality monitor started"
    );

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                run_ping_round(&network_state, &qos, &net_tx).await;
                repair_evicting(&me, &outgoing_assignments, &network_state, &qos, &storage_managers, &net_tx).await;
                crate::network::save_outgoing(&outgoing_assignments);
                if let Ok(q) = qos.read() {
                    q.save();
                }
            }
            _ = pos_interval.tick() => {
                run_pos_round(&outgoing_assignments, &fillers, &network_state, &qos, &observations, &storage_managers, &net_tx, &me).await;
            }
            _ = fill_interval.tick() => {
                run_fill_round(&me, &network_state, &storage_managers, &fillers, &net_tx).await;
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

#[allow(clippy::too_many_arguments)]
async fn run_pos_round(
    outgoing_assignments: &OutgoingAssignments,
    fillers: &Fillers,
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    observations: &ObservationLog,
    storage_managers: &StorageManagerMap,
    net_tx: &mpsc::Sender<NetworkCommand>,
    me: &str,
) {
    let challenges = challenges(outgoing_assignments, fillers);
    if challenges.is_empty() {
        return;
    }
    let mut suspected: Vec<String> = Vec::new();
    let mut handles = Vec::new();
    for challenge in challenges {
        let net_tx = net_tx.clone();
        handles.push(tokio::spawn(async move {
            let passed = prove_one(&challenge, net_tx).await;
            (challenge.peer, challenge.peer_id, passed)
        }));
    }
    for handle in handles {
        let Ok((peer, _, passed)) = handle.await else {
            continue;
        };
        let Some(passed) = passed else { continue };
        tracing::debug!(peer = %peer, passed, "PoS challenge result");
        if let Ok(mut q) = qos.write() {
            if passed {
                q.entry(peer.clone()).record_pos_success();
            } else {
                q.entry(peer.clone()).record_pos_failure();
            }
        }
        let placed = placed_bytes(outgoing_assignments, fillers, &peer);
        observe(observations, &peer, passed, placed, crate::chain::now_ms());
        let is_suspected = qos
            .read()
            .is_ok_and(|q| q.fault_score(&peer) >= FAULT_SUSPECTED);
        if is_suspected && !suspected.contains(&peer) {
            suspected.push(peer);
        }
    }
    for peer in suspected {
        repair_peer(
            me,
            &peer,
            outgoing_assignments,
            network_state,
            qos,
            storage_managers,
            net_tx,
        )
        .await;
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

struct LostChunk {
    chunk_id: String,
    grant: crate::files::Grant,
    layer: crate::storage::StorageTier,
    lost: usize,
    holders: Vec<PeerId>,
}

fn lost_chunks(outgoing: &OutgoingAssignments, lost_peer: &str) -> Vec<LostChunk> {
    let Ok(guard) = outgoing.read() else {
        return Vec::new();
    };
    let Some(on_lost) = guard.get(lost_peer) else {
        return Vec::new();
    };
    let mut chunks: Vec<LostChunk> = Vec::new();
    for f in on_lost {
        match chunks.iter_mut().find(|c| c.chunk_id == f.chunk_id) {
            Some(c) => c.lost += 1,
            None => chunks.push(LostChunk {
                chunk_id: f.chunk_id.clone(),
                grant: f.grant.clone(),
                layer: f.layer,
                lost: 1,
                holders: Vec::new(),
            }),
        }
    }
    for chunk in &mut chunks {
        chunk.holders = guard
            .iter()
            .filter(|(peer, frags)| {
                peer.as_str() != lost_peer && frags.iter().any(|f| f.chunk_id == chunk.chunk_id)
            })
            .filter_map(|(peer, _)| peer.parse().ok())
            .collect();
    }
    chunks
}

async fn gather(
    chunk: &LostChunk,
    storage_managers: &StorageManagerMap,
    net_tx: &mpsc::Sender<NetworkCommand>,
) -> Vec<EncodedFragment> {
    let mut fragments: Vec<EncodedFragment> = Vec::new();
    if let Ok(managers) = storage_managers.read() {
        for sm in managers.values() {
            let sm = sm.read().unwrap();
            for meta in sm.index.fragments_for_chunk(&chunk.chunk_id) {
                if let Ok(f) = sm.load_fragment(&chunk.chunk_id, &meta.fragment_id) {
                    fragments.push(f);
                }
            }
        }
    }
    for holder in &chunk.holders {
        if fragments.first().is_some_and(|f| fragments.len() >= f.k) {
            break;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let sent = net_tx
            .send(NetworkCommand::FetchChunkFragments {
                peer_id: *holder,
                chunk_id: chunk.chunk_id.clone(),
                grant: chunk.grant.clone(),
                resp_tx: tx,
            })
            .await;
        if sent.is_err() {
            continue;
        }
        if let Ok(Ok(FragmentResponse::FoundMany { fragments: found })) =
            tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), rx).await
        {
            fragments.extend(found.into_iter().filter_map(|(id, bytes)| {
                EncodedFragment::from_bytes(id, chunk.chunk_id.clone(), &bytes).ok()
            }));
        }
    }
    fragments
}

#[allow(clippy::too_many_arguments)]
pub async fn repair_peer(
    me: &str,
    lost_peer: &str,
    outgoing: &OutgoingAssignments,
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    storage_managers: &StorageManagerMap,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let chunks = lost_chunks(outgoing, lost_peer);
    if chunks.is_empty() {
        return;
    }
    let mut pouches: Vec<String> = {
        let (Ok(ns), Ok(q)) = (network_state.read(), qos.read()) else {
            return;
        };
        ns.all()
            .into_iter()
            .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != lost_peer)
            .filter(|n| n.metadata.get("evicting").and_then(|v| v.as_bool()) != Some(true))
            .filter(|n| q.fault_score(&n.peer_id) < FAULT_DEGRADED)
            .map(|n| n.peer_id.clone())
            .collect()
    };
    if storage_managers.read().is_ok_and(|m| !m.is_empty()) {
        pouches.push(me.to_string());
    }
    let mut repaired = 0;
    for chunk in chunks {
        let fragments = gather(&chunk, storage_managers, net_tx).await;
        let Some(k) = fragments.first().map(|f| f.k) else {
            tracing::warn!(chunk = %chunk.chunk_id, "repair: no fragment reachable");
            continue;
        };
        if fragments.len() < k {
            tracing::warn!(chunk = %chunk.chunk_id, have = fragments.len(), k, "repair: not enough fragments");
            continue;
        }
        let holders: Vec<String> = chunk.holders.iter().map(|p| p.to_string()).collect();
        let mut targets: Vec<String> = pouches
            .iter()
            .filter(|p| !holders.contains(p))
            .cloned()
            .collect();
        crate::control::files::rank(&chunk.chunk_id, &mut targets);
        if targets.is_empty() {
            targets = holders.clone();
        }
        let Some(target) = targets.first().cloned() else {
            tracing::warn!(chunk = %chunk.chunk_id, "repair: no Pouch to move fragments to");
            continue;
        };
        let Ok(recoded) = rlnc::recode(&fragments, chunk.lost) else {
            continue;
        };
        if target == me {
            let owner = crate::storage::ChunkOwner {
                owner_key: chunk.grant.owner_key.clone(),
                file_id: chunk.grant.file_id.clone(),
                layer: chunk.layer,
            };
            let local = storage_managers
                .read()
                .ok()
                .and_then(|m| m.values().next().cloned());
            if let Some(sm) = local {
                for fragment in &recoded {
                    if let Err(e) = sm.write().unwrap().store_fragment(fragment, &owner) {
                        tracing::warn!(chunk = %chunk.chunk_id, "repair: local store failed: {e}");
                    }
                }
            }
        }
        if let Some(peer) = target.parse::<PeerId>().ok().filter(|_| target != me) {
            for fragment in recoded {
                let _ = net_tx
                    .send(NetworkCommand::PushFragment {
                        peer_id: peer,
                        chunk_id: chunk.chunk_id.clone(),
                        fragment_id: fragment.id.clone(),
                        data: fragment.to_bytes(),
                        grant: chunk.grant.clone(),
                        layer: chunk.layer,
                    })
                    .await;
            }
        }
        if let Ok(mut o) = outgoing.write() {
            if let Some(frags) = o.get_mut(lost_peer) {
                frags.retain(|f| f.chunk_id != chunk.chunk_id);
            }
            o.retain(|_, frags| !frags.is_empty());
        }
        repaired += 1;
    }
    if repaired > 0 {
        tracing::info!(lost = %lost_peer, chunks = repaired, "repaired fragments away from a lost Pouch");
    }
}

async fn repair_evicting(
    me: &str,
    outgoing: &OutgoingAssignments,
    network_state: &Arc<RwLock<NetworkState>>,
    qos: &Arc<RwLock<QosRegistry>>,
    storage_managers: &StorageManagerMap,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let evicting: Vec<String> = network_state
        .read()
        .map(|ns| {
            ns.all()
                .into_iter()
                .filter(|n| n.metadata.get("evicting").and_then(|v| v.as_bool()) == Some(true))
                .map(|n| n.peer_id.clone())
                .collect()
        })
        .unwrap_or_default();
    for peer in evicting {
        repair_peer(
            me,
            &peer,
            outgoing,
            network_state,
            qos,
            storage_managers,
            net_tx,
        )
        .await;
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
            qos.write().unwrap().entry("p1").record_pos_failure();
        }
        let fault = qos.read().unwrap().fault_score("p1");
        assert!(fault > 0, "fault score should increase after PoS failures");
    }

    #[test]
    fn pos_success_decays_fault_score() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        for _ in 0..4 {
            qos.write().unwrap().entry("p1").record_pos_failure();
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
    fn a_lost_pouch_lists_its_chunks_and_who_else_holds_them() {
        let carlo = crate::identity::Identity::in_memory(None).unwrap();
        let grant = crate::files::Grant::sign(&carlo, "f", &carlo.fingerprint).unwrap();
        let frag = |chunk: &str, id: &str| crate::network::OutgoingFragment {
            chunk_id: chunk.into(),
            fragment_id: id.into(),
            grant: grant.clone(),
            layer: crate::storage::StorageTier::T1,
            root: [0; 32],
            blocks: 1,
            size: 10,
        };
        let (lost, other) = (PeerId::random().to_string(), PeerId::random());
        let outgoing: OutgoingAssignments = Arc::new(RwLock::new(
            [
                (
                    lost.clone(),
                    vec![frag("a", "1"), frag("a", "2"), frag("b", "3")],
                ),
                (other.to_string(), vec![frag("a", "4")]),
            ]
            .into_iter()
            .collect(),
        ));
        let mut chunks = lost_chunks(&outgoing, &lost);
        chunks.sort_by(|x, y| x.chunk_id.cmp(&y.chunk_id));
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            (chunks[0].lost, chunks[0].holders.clone()),
            (2, vec![other])
        );
        assert_eq!((chunks[1].lost, chunks[1].holders.len()), (1, 0));
        assert!(lost_chunks(&outgoing, "nobody").is_empty());
        assert_eq!(placed_bytes(&outgoing, &Default::default(), &lost), 30);
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

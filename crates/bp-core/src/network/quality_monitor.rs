use crate::{
    chain::{observe, Chains, ObservationLog},
    identity::Identity,
    network::{
        qos::QosRegistry, state::NetworkState, valid_commitment, NetworkCommand, Proven,
        StorageManagerMap,
    },
    service::ServiceType,
    storage::{
        commitment::{Commitment, Held},
        filler::{blob, BLOB_BYTES},
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

const POS_INTERVAL_SECS: u64 = 60;

const POS_TIMEOUT_SECS: u64 = 10;

const FILL_INTERVAL_SECS: u64 = 60;

const MAX_BLOBS_PER_ROUND: u64 = 256;

const CATCH_UP_INTERVAL_SECS: u64 = 60;

const MAX_REPAIRS_PER_ROUND: usize = 64;

const MAX_CHALLENGES_PER_ROUND: usize = 8;

#[allow(clippy::too_many_arguments)]
pub async fn run_quality_monitor(
    identity: Identity,
    network_state: Arc<RwLock<NetworkState>>,
    qos: Arc<RwLock<QosRegistry>>,
    observations: ObservationLog,
    net_tx: mpsc::Sender<NetworkCommand>,
    storage_managers: StorageManagerMap,
    chains: Chains,
) {
    let me = identity.peer_id.to_string();
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
                if let Ok(q) = qos.read() {
                    q.save();
                }
            }
            _ = pos_interval.tick() => {
                run_pos_round(&identity, &network_state, &storage_managers, &chains, &qos, &observations, &net_tx).await;
            }
            _ = fill_interval.tick() => {
                run_fill_round(&identity, &network_state, &storage_managers, &net_tx).await;
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

pub struct Challenge {
    pub peer: String,
    pub peer_user: String,
    pub target: PosTarget,
    pub seed: u64,
    pub stored_bytes: u64,
}

impl Challenge {
    pub fn passes(&self, proven: &Proven, signer_user: Option<&str>, valid: bool) -> bool {
        let (proof, commitment) = proven;
        let held_ok = match (&self.target, &commitment.held) {
            (PosTarget::Fragment { chunk_id }, Held::Fragment { chunk_id: c, .. }) => c == chunk_id,
            (PosTarget::Filler { .. }, Held::Filler { .. }) => {
                signer_user.is_some_and(|u| u != self.peer_user)
            }
            _ => false,
        };
        valid && held_ok && commitment.holder == self.peer && commitment.proves(self.seed, proof)
    }
}

fn challenges(
    identity: &Identity,
    network_state: &Arc<RwLock<NetworkState>>,
    storage_managers: &StorageManagerMap,
) -> Vec<Challenge> {
    let me = identity.peer_id.to_string();
    let pouches: Vec<(String, StorageTier, Vec<String>)> = storage_managers
        .read()
        .map(|m| {
            m.values()
                .map(|sm| {
                    let sm = sm.read().unwrap();
                    let chunks = sm
                        .meta
                        .tier
                        .layers()
                        .flat_map(|l| sm.catalog(l))
                        .filter(|e| e.owner.owner_key != identity.user_key)
                        .map(|e| e.chunk_id)
                        .collect();
                    (sm.meta.network_id.clone(), sm.meta.tier, chunks)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut rng = rand::thread_rng();
    let mut out = Vec::new();
    let Ok(ns) = network_state.read() else {
        return out;
    };
    for (network, my_tier, my_chunks) in pouches {
        let mut peers: Vec<_> = ns
            .in_network(&network)
            .into_iter()
            .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != me)
            .filter(|n| n.user_fingerprint != identity.fingerprint)
            .collect();
        peers.shuffle(&mut rng);
        for node in peers.into_iter().take(MAX_CHALLENGES_PER_ROUND) {
            let meta = |key: &str| node.metadata.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
            let tier = node
                .metadata
                .get("tier")
                .and_then(|t| t.as_str())
                .and_then(|t| StorageTier::parse(t).ok())
                .unwrap_or(StorageTier::T1);
            let fillers = meta("fillers");
            let shared = tier.min(my_tier) >= StorageTier::T1 && !my_chunks.is_empty();
            let target = match (shared, fillers > 0) {
                (true, true) if rng.gen_bool(0.5) => PosTarget::Filler {
                    nth: rng.gen_range(0..fillers),
                },
                (true, _) => PosTarget::Fragment {
                    chunk_id: my_chunks.choose(&mut rng).cloned().unwrap_or_default(),
                },
                (false, true) => PosTarget::Filler {
                    nth: rng.gen_range(0..fillers),
                },
                (false, false) => continue,
            };
            out.push(Challenge {
                peer: node.peer_id.clone(),
                peer_user: node.user_fingerprint.clone(),
                target,
                seed: rng.gen(),
                stored_bytes: meta("stored_bytes"),
            });
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn run_pos_round(
    identity: &Identity,
    network_state: &Arc<RwLock<NetworkState>>,
    storage_managers: &StorageManagerMap,
    chains: &Chains,
    qos: &Arc<RwLock<QosRegistry>>,
    observations: &ObservationLog,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let mut handles = Vec::new();
    for challenge in challenges(identity, network_state, storage_managers) {
        let net_tx = net_tx.clone();
        handles.push(tokio::spawn(async move {
            let proven = prove_one(&challenge, net_tx).await;
            (challenge, proven)
        }));
    }
    for handle in handles {
        let Ok((challenge, Some(proven))) = handle.await else {
            continue;
        };
        let passed = proven.as_ref().is_some_and(|p| {
            let signer_user = chains.read().ok().and_then(|all| {
                all.values()
                    .find_map(|c| c.ledger().device_owner(&p.1.signer).map(str::to_string))
            });
            let valid = valid_commitment(chains, &p.1, &challenge.peer);
            challenge.passes(p, signer_user.as_deref(), valid)
        });
        tracing::debug!(peer = %challenge.peer, passed, "PoS challenge result");
        let success = qos.write().map_or(0.0, |mut q| {
            let entry = q.entry(challenge.peer.clone());
            entry.record_challenge(passed);
            entry.challenge_success_ewma
        });
        let verified = (challenge.stored_bytes as f64 * success) as u64;
        observe(
            observations,
            &challenge.peer,
            passed,
            verified,
            crate::chain::now_ms(),
        );
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

async fn prove_one(
    challenge: &Challenge,
    net_tx: mpsc::Sender<NetworkCommand>,
) -> Option<Option<Proven>> {
    let peer_id = challenge.peer.parse::<PeerId>().ok()?;
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    net_tx
        .send(NetworkCommand::ProofOfStorage {
            peer_id,
            target: challenge.target.clone(),
            seed: challenge.seed,
            resp_tx,
        })
        .await
        .ok()?;
    Some(
        tokio::time::timeout(Duration::from_secs(POS_TIMEOUT_SECS), resp_rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten(),
    )
}

async fn run_fill_round(
    identity: &Identity,
    network_state: &Arc<RwLock<NetworkState>>,
    storage_managers: &StorageManagerMap,
    net_tx: &mpsc::Sender<NetworkCommand>,
) {
    let me = identity.peer_id.to_string();
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
                    .filter(|n| n.user_fingerprint != identity.fingerprint)
                    .filter_map(|n| {
                        let free = n.metadata.get("free_bytes")?.as_u64()?;
                        Some((n.peer_id.clone(), free))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let sponsors = pouches.len() as u64 + 1;
        for (pouch, free) in pouches {
            let Ok(peer_id) = pouch.parse::<PeerId>() else {
                continue;
            };
            let blobs = (free / sponsors / BLOB_BYTES as u64).min(MAX_BLOBS_PER_ROUND);
            let mut filled = 0;
            for _ in 0..blobs {
                let (blob_id, data) = blob();
                let held = Held::Filler {
                    blob_id: blob_id.clone(),
                };
                let Ok(commitment) = Commitment::sign(&identity.device, &me, held, &pouch, &data)
                else {
                    break;
                };
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                let sent = net_tx
                    .send(NetworkCommand::Fill {
                        peer_id,
                        network: network.clone(),
                        blob_id,
                        data,
                        commitment,
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
    fn a_challenge_passes_only_with_the_right_commitment_and_block() {
        let sponsor = libp2p::identity::Keypair::generate_ed25519();
        let data: Vec<u8> = (0..3 * crate::storage::merkle::BLOCK)
            .map(|i| i as u8)
            .collect();
        let commit = |held: Held, holder: &str| {
            Commitment::sign(&sponsor, "sponsor-device", held, holder, &data).unwrap()
        };
        let fragment = Held::Fragment {
            chunk_id: "c".into(),
            fragment_id: "f".into(),
        };
        let challenge = Challenge {
            peer: "pouch".into(),
            peer_user: "lucia".into(),
            target: PosTarget::Fragment {
                chunk_id: "c".into(),
            },
            seed: 4,
            stored_bytes: 0,
        };
        let c = commit(fragment.clone(), "pouch");
        let proof = |c: &Commitment, seed: u64| {
            crate::storage::merkle::prove(&data, c.index_for(seed)).unwrap()
        };
        let right = (proof(&c, 4), c.clone());
        assert!(challenge.passes(&right, Some("carlo"), true));
        assert!(!challenge.passes(&right, Some("carlo"), false));
        assert!(!challenge.passes(&(proof(&c, 5), c.clone()), Some("carlo"), true));
        let elsewhere = commit(fragment, "another pouch");
        assert!(!challenge.passes(&(proof(&elsewhere, 4), elsewhere), Some("carlo"), true));

        let filler = Challenge {
            target: PosTarget::Filler { nth: 0 },
            ..challenge
        };
        let blob = commit(
            Held::Filler {
                blob_id: "b".into(),
            },
            "pouch",
        );
        let proven = (proof(&blob, 4), blob);
        assert!(filler.passes(&proven, Some("carlo"), true));
        assert!(
            !filler.passes(&proven, Some("lucia"), true),
            "self-sponsored filler"
        );
        assert!(!filler.passes(&right, Some("carlo"), true));
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

        tokio::time::timeout(
            Duration::from_millis(200),
            run_quality_monitor(
                Identity::in_memory(None).unwrap(),
                ns,
                qos,
                Default::default(),
                net_tx,
                Default::default(),
                Default::default(),
            ),
        )
        .await
        .ok();
    }
}

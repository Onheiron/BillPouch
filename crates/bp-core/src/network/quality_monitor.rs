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
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const PING_INTERVAL_SECS: u64 = 60;

const PING_TIMEOUT_SECS: u64 = 5;

const POS_INTERVAL_SECS: u64 = 60;

const POS_TIMEOUT_SECS: u64 = 10;

const FILL_INTERVAL_SECS: u64 = 60;

const MAX_BLOBS_PER_ROUND: u64 = 256;

const CATCH_UP_INTERVAL_SECS: u64 = 60;

const MAX_REPAIRS_PER_ROUND: usize = 64;
const MAX_VOLUNTEERS_PER_ROUND: usize = 2;

const DEFAULT_TRUST_AT: f64 = 0.8;

const CLAIM_RETRY_SECS: u64 = 600;

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

    let mut claimed = Claims::default();
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
                run_catch_up(&me, &network_state, &storage_managers, &chains, &net_tx, &mut claimed).await;
                run_trust_round(&identity, &storage_managers, &chains, &mut claimed);
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

pub fn repairs_fall_to_me(
    key: &str,
    planned: usize,
    holding: &[String],
    layer: &[String],
    me: &str,
) -> bool {
    let lacking = planned.saturating_sub(holding.len());
    let mut candidates: Vec<&str> = layer
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(me))
        .filter(|p| !holding.iter().any(|h| h == p))
        .collect();
    candidates.sort_by_key(|p| blake3::hash(format!("{key}/{p}").as_bytes()).to_hex());
    candidates.dedup();
    candidates
        .iter()
        .position(|p| *p == me)
        .is_some_and(|rank| rank < lacking)
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
    chains: &Chains,
    net_tx: &mpsc::Sender<NetworkCommand>,
    claimed: &mut Claims,
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
        shed_extra(me, &network, &sm, chains, claimed);
        let pouches_by_tier = layer_tiers(network_state, &network, me);
        let mut recodes = Vec::new();
        let mut complete = Vec::new();
        {
            let all = chains.read().unwrap();
            let Some(chain) = all.get(&network) else {
                continue;
            };
            let ledger = chain.ledger();
            let volunteered = volunteers(ledger, &sm.read().unwrap(), tier, me);
            for (file_id, placement) in &ledger.placements {
                if tier < placement.layer
                    || placement.holders.contains(me)
                    || placement.extra.contains(me)
                {
                    continue;
                }
                let live: Vec<String> = ledger
                    .live_holders(placement)
                    .into_iter()
                    .cloned()
                    .collect();
                let layer: Vec<String> = pouches_by_tier
                    .iter()
                    .filter(|(_, t)| *t >= placement.layer)
                    .map(|(p, _)| p.clone())
                    .collect();
                if !volunteered.contains(file_id)
                    && !repairs_fall_to_me(file_id, placement.planned, &live, &layer, me)
                {
                    continue;
                }
                let Some(owner_key) = ledger.members.get(&placement.owner).map(|m| m.key.clone())
                else {
                    continue;
                };
                let owner = crate::storage::ChunkOwner {
                    owner_key,
                    file_id: file_id.clone(),
                    layer: placement.layer,
                };
                let missing: Vec<&String> = {
                    let sm = sm.read().unwrap();
                    placement
                        .chunks
                        .iter()
                        .filter(|c| !sm.has_chunk(c))
                        .collect()
                };
                if missing.is_empty() {
                    complete.push(file_id.clone());
                }
                for chunk in missing {
                    recodes.push((chunk.clone(), owner.clone(), live.clone(), placement.k));
                }
            }
        }
        for file_id in complete {
            if !claimed.take(&file_id) {
                continue;
            }
            let hold = crate::ledger::Op::Hold {
                file_id: file_id.clone(),
                device: me.to_string(),
            };
            if let Some(chain) = chains.write().unwrap().get_mut(&network) {
                match chain.propose(hold, crate::chain::now_ms()) {
                    Ok(()) => tracing::info!(file_id = %file_id, "now holds a file"),
                    Err(e) => tracing::warn!(file_id = %file_id, "cannot register as holder: {e}"),
                }
            }
        }
        let mut started = 0;
        for (chunk_id, owner, live, k) in recodes.into_iter().take(MAX_REPAIRS_PER_ROUND) {
            let Some(route) = repair_route(&live, k, me) else {
                continue;
            };
            let Ok(first) = route[0].parse::<PeerId>() else {
                continue;
            };
            let _ = net_tx
                .send(NetworkCommand::Recode {
                    peer_id: first,
                    chunk_id,
                    owner,
                    route: route[1..].to_vec(),
                })
                .await;
            started += 1;
        }
        if started > 0 {
            tracing::info!(network = %network, chunks = started, "repairing files below their planned holders");
        }
    }
}

fn shed_extra(
    me: &str,
    network: &str,
    sm: &Arc<RwLock<crate::storage::StorageManager>>,
    chains: &Chains,
    claimed: &mut Claims,
) {
    let tight = {
        let sm = sm.read().unwrap();
        sm.meta.available_bytes() + sm.filler_bytes() < sm.meta.storage_bytes_bid / 4
    };
    if !tight {
        return;
    }
    let mut all = chains.write().unwrap();
    let Some(chain) = all.get_mut(network) else {
        return;
    };
    let ledger = chain.ledger();
    let Some((file_id, owner_key)) = ledger.placements.iter().find_map(|(file_id, p)| {
        let owner = ledger.members.get(&p.owner)?;
        (p.extra.contains(me) && !claimed.pending(&format!("release/{file_id}")))
            .then(|| (file_id.clone(), owner.key.clone()))
    }) else {
        return;
    };
    claimed.take(&format!("release/{file_id}"));
    let release = crate::ledger::Op::Release {
        file_id: file_id.clone(),
        device: me.to_string(),
    };
    if let Err(e) = chain.propose(release, crate::chain::now_ms()) {
        return tracing::warn!(file_id = %file_id, "cannot release an extra file: {e}");
    }
    drop(all);
    match sm.write().unwrap().delete_file(&file_id, &owner_key) {
        Ok(freed) => {
            tracing::info!(file_id = %file_id, freed, "gave up an extra file to make room")
        }
        Err(e) => tracing::warn!(file_id = %file_id, "cannot delete an extra file: {e}"),
    }
}

#[derive(Default)]
struct Claims(std::collections::HashMap<String, Instant>);

impl Claims {
    fn pending(&self, key: &str) -> bool {
        self.0
            .get(key)
            .is_some_and(|at| at.elapsed() < Duration::from_secs(CLAIM_RETRY_SECS))
    }

    fn take(&mut self, key: &str) -> bool {
        if self.pending(key) {
            return false;
        }
        self.0.insert(key.to_string(), Instant::now());
        true
    }
}

fn trust_threshold() -> f64 {
    std::env::var("BP_TRUST_AT")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| (0.0..=1.0).contains(v))
        .unwrap_or(DEFAULT_TRUST_AT)
}

fn run_trust_round(
    identity: &Identity,
    storage_managers: &StorageManagerMap,
    chains: &Chains,
    claimed: &mut Claims,
) {
    let me = identity.peer_id.to_string();
    let user = identity.fingerprint.as_str();
    let mut released = Vec::new();
    let mut offers = Vec::new();
    {
        let all = chains.read().unwrap();
        for (network, chain) in all.iter() {
            let ledger = chain.ledger();
            for bet in ledger.bets.iter().filter(|b| b.released == me) {
                let Some(p) = ledger.placements.get(&bet.file_id) else {
                    continue;
                };
                if p.holders.contains(&me) || p.extra.contains(&me) {
                    continue;
                }
                if let Some(owner) = ledger.members.get(&p.owner) {
                    released.push((network.clone(), bet.file_id.clone(), owner.key.clone()));
                }
            }
            for (file_id, p) in &ledger.placements {
                if !p.holders.contains(&me) || claimed.pending(&format!("trust/{file_id}")) {
                    continue;
                }
                for newcomer in &p.extra {
                    if ledger.may_trust(user, file_id, &me, newcomer).is_ok() {
                        offers.push((
                            network.clone(),
                            file_id.clone(),
                            newcomer.clone(),
                            ledger.survival(newcomer),
                        ));
                    }
                }
            }
        }
    }
    if let Ok(managers) = storage_managers.read() {
        for (network, file_id, owner_key) in &released {
            for sm in managers.values() {
                let mut sm = sm.write().unwrap();
                if sm.meta.network_id == *network {
                    match sm.delete_file(file_id, owner_key) {
                        Ok(0) => {}
                        Ok(freed) => {
                            tracing::info!(file_id = %file_id, freed, "dropped a file entrusted to a newcomer")
                        }
                        Err(e) => {
                            tracing::warn!(file_id = %file_id, "cannot drop an entrusted file: {e}")
                        }
                    }
                }
            }
        }
    }
    let threshold = trust_threshold();
    let mut chosen: std::collections::BTreeMap<String, (f64, String, String)> =
        std::collections::BTreeMap::new();
    for (network, file_id, newcomer, confidence) in offers {
        let better = chosen
            .get(&network)
            .is_none_or(|(best, _, _)| confidence > *best);
        if confidence >= threshold && better {
            chosen.insert(network, (confidence, file_id, newcomer));
        }
    }
    let mut all = chains.write().unwrap();
    for (network, (confidence, file_id, newcomer)) in chosen {
        let Some(chain) = all.get_mut(&network) else {
            continue;
        };
        claimed.take(&format!("trust/{file_id}"));
        let op = crate::ledger::Op::Trust {
            file_id: file_id.clone(),
            released: me.clone(),
            newcomer: newcomer.clone(),
            confidence_ppm: (confidence * 1_000_000.0) as u32,
        };
        match chain.propose(op, crate::chain::now_ms()) {
            Ok(()) => {
                tracing::info!(file_id = %file_id, newcomer = %newcomer, confidence, "trusted a newcomer with a file")
            }
            Err(e) => tracing::warn!(file_id = %file_id, "cannot trust the newcomer: {e}"),
        }
    }
}

fn volunteers(
    ledger: &crate::ledger::Ledger,
    sm: &crate::storage::StorageManager,
    tier: StorageTier,
    me: &str,
) -> HashSet<String> {
    let bid = sm.meta.storage_bytes_bid;
    let mut spare = (sm.meta.available_bytes() + sm.filler_bytes()).saturating_sub(bid / 2);
    let mut candidates: Vec<(bool, usize, String, u64, &str)> = ledger
        .placements
        .iter()
        .filter(|(file_id, p)| {
            tier >= p.layer
                && !p.holders.contains(me)
                && !p.extra.contains(me)
                && p.k > 0
                && !ledger
                    .bets
                    .iter()
                    .any(|b| b.file_id == **file_id && b.released == me)
        })
        .map(|(file_id, p)| {
            let started = p.chunks.iter().any(|c| sm.has_chunk(c));
            let size = (p.chunks.len() * crate::files::CHUNK_BYTES / p.k) as u64;
            let rank = blake3::hash(format!("{file_id}/{me}").as_bytes())
                .to_hex()
                .to_string();
            (
                !started,
                ledger.live_holders(p).len(),
                rank,
                size,
                file_id.as_str(),
            )
        })
        .collect();
    candidates.sort();
    let mut chosen = HashSet::new();
    for (fresh, _, _, size, file_id) in candidates {
        if chosen.len() >= MAX_VOLUNTEERS_PER_ROUND {
            break;
        }
        if fresh {
            let layer = ledger.placements[file_id].layer;
            if size > spare || size > sm.layer_free(layer) {
                continue;
            }
            spare -= size;
        }
        chosen.insert(file_id.to_string());
    }
    chosen
}

fn layer_tiers(
    network_state: &Arc<RwLock<NetworkState>>,
    network: &str,
    me: &str,
) -> Vec<(String, StorageTier)> {
    network_state
        .read()
        .map(|ns| {
            ns.in_network(network)
                .into_iter()
                .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != me)
                .filter(|n| n.metadata.get("evicting").and_then(|v| v.as_bool()) != Some(true))
                .filter_map(|n| {
                    let tier = StorageTier::parse(n.metadata.get("tier")?.as_str()?).ok()?;
                    Some((n.peer_id.clone(), tier))
                })
                .collect()
        })
        .unwrap_or_default()
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
        let score_before = qos.read().unwrap().availability("p1");

        for _ in 0..5 {
            record_timeout("p1", &qos);
        }
        let score_after = qos.read().unwrap().availability("p1");
        assert!(score_after < score_before, "timeout should degrade score");
    }

    #[test]
    fn new_peer_gets_timeout_entry() {
        let qos = Arc::new(RwLock::new(QosRegistry::new()));
        record_timeout("new-peer", &qos);
        let score = qos.read().unwrap().availability("new-peer");
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
    fn only_as_many_nodes_as_the_chunk_lacks_repair_it() {
        let layer: Vec<String> = (0..6).map(|i| format!("p{i}")).collect();
        let holding = vec!["p0".to_string(), "p1".to_string(), "p2".to_string()];
        let repairers = |planned: usize| {
            layer[3..]
                .iter()
                .filter(|me| repairs_fall_to_me("c", planned, &holding, &layer, me))
                .count()
        };
        assert_eq!(
            repairers(3),
            0,
            "a chunk with all its holders needs no repair"
        );
        assert_eq!(repairers(4), 1);
        assert_eq!(repairers(5), 2);
        assert_eq!(repairers(9), 3, "never more than the nodes that lack it");
        assert!(!repairs_fall_to_me("c", 9, &holding, &layer, "p0"));
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

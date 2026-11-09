use crate::{
    coding::{params as coding_params, rlnc, rlnc::EncodedFragment},
    control::{
        protocol::{
            ControlResponse, FileEntry, GetFileData, ListFilesData, PutFileData, ShareData,
        },
        server::DaemonState,
    },
    files::{
        new_file_id, Action, Grant, Head, Index, Manifest, Order, PendingShare, CHUNK_BYTES,
        ORDER_RESENDS,
    },
    membership::new_key,
    network::{
        fragment_gossip::{FragmentIndexAnnouncement, FragmentPointer},
        FragmentResponse, Gossip, NetworkCommand,
    },
    service::ServiceType,
    storage::{
        commitment::{Commitment, Held},
        ChunkCipher, StorageTier,
    },
};
use libp2p::PeerId;
use std::collections::HashSet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DEFAULT_PH: f64 = 0.999;
const FETCH_TIMEOUT_SECS: u64 = 10;

#[derive(Default)]
pub struct Stored {
    pub chunks: Vec<String>,
    pub k: usize,
    pub n: usize,
    pub pe: f64,
    pub stored: usize,
    pub distributed: usize,
    pub peers: usize,
}

async fn publish(state: &DaemonState, network_id: &str, message: Gossip) {
    let _ = state
        .net_tx
        .send(NetworkCommand::Publish {
            network_id: network_id.to_string(),
            message,
        })
        .await;
}

pub struct LayerNode {
    pub peer: String,
    pub tier: StorageTier,
    pub free: std::collections::BTreeMap<StorageTier, u64>,
}

pub fn layer_nodes(state: &DaemonState, network_id: &str) -> Vec<LayerNode> {
    let me = state.identity.peer_id.to_string();
    let mut nodes: Vec<LayerNode> = state
        .network_state
        .read()
        .unwrap()
        .in_network(network_id)
        .into_iter()
        .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != me)
        .filter(|n| n.metadata.get("evicting").and_then(|v| v.as_bool()) != Some(true))
        .filter_map(|n| {
            let tier = StorageTier::parse(n.metadata.get("tier")?.as_str()?).ok()?;
            let free = n
                .metadata
                .get("layer_free")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            Some(LayerNode {
                peer: n.peer_id.clone(),
                tier,
                free,
            })
        })
        .collect();
    for sm in state.storage_managers.read().unwrap().values() {
        let sm = sm.read().unwrap();
        if sm.meta.network_id == network_id {
            nodes.push(LayerNode {
                peer: me.clone(),
                tier: sm.meta.tier,
                free: sm
                    .meta
                    .tier
                    .layers()
                    .map(|l| (l, sm.layer_free(l)))
                    .collect(),
            });
        }
    }
    nodes
}

fn availabilities(state: &DaemonState, peers: &[String]) -> Vec<f64> {
    let qos = state.qos.read().unwrap();
    peers.iter().map(|p| qos.availability(p)).collect()
}

pub fn choose_layer(
    nodes: &[LayerNode],
    file_id: &str,
    availabilities: impl Fn(&[String]) -> Vec<f64>,
    data_len: usize,
    ph: f64,
) -> Result<(StorageTier, Vec<String>, coding_params::Plan), String> {
    if nodes.is_empty() {
        return Err("no Pouch available — hatch a Pouch or wait for Pouch peers to join".into());
    }
    for layer in StorageTier::all().iter().copied() {
        let mut members: Vec<&LayerNode> = nodes.iter().filter(|n| n.tier >= layer).collect();
        if members.is_empty() {
            break;
        }
        members.sort_by_key(|n| blake3::hash(format!("{file_id}/{}", n.peer).as_bytes()).to_hex());
        let peers: Vec<String> = members.iter().map(|n| n.peer.clone()).collect();
        let (plan, chosen) =
            coding_params::best_plan(&availabilities(&peers), ph).map_err(|e| e.to_string())?;
        let fragment = (data_len.div_ceil(plan.k) + plan.k + 16) as u64;
        if chosen
            .iter()
            .all(|&i| members[i].free.get(&layer).copied().unwrap_or(0) >= fragment)
        {
            let holders = chosen.into_iter().map(|i| peers[i].clone()).collect();
            return Ok((layer, holders, plan));
        }
    }
    Err("every layer of the network is full".into())
}

pub struct Rate {
    pub k: u64,
    pub n: u64,
    pub others: u64,
}

impl Rate {
    pub fn quota(&self, offered: u64) -> u64 {
        (offered as u128 * self.k as u128 / self.n.max(1) as u128) as u64
    }

    pub fn used(&self, usage: u64) -> u64 {
        (usage as u128 * self.k as u128 / self.others.max(1) as u128) as u64
    }

    pub fn on_others(&self, size: u64) -> u64 {
        size.div_ceil(self.k.max(1)) * self.others
    }
}

pub fn quota_rate(
    state: &DaemonState,
    network_id: &str,
    size: u64,
    ph: f64,
) -> Result<Rate, String> {
    let nodes = layer_nodes(state, network_id);
    let sample = (size as usize).clamp(1, CHUNK_BYTES);
    let (_, peers, plan) = choose_layer(&nodes, "", |p| availabilities(state, p), sample, ph)?;
    let chains = state.chains.read().unwrap();
    let ledger = chains
        .get(network_id)
        .map(|c| c.ledger())
        .ok_or_else(|| format!("you are not in a network named '{network_id}'"))?;
    let me = &state.identity.fingerprint;
    let others = peers
        .iter()
        .filter(|p| ledger.device_owner(p) != Some(me.as_str()))
        .count() as u64;
    Ok(Rate {
        k: plan.k as u64,
        n: peers.len() as u64,
        others,
    })
}

pub fn default_rate(state: &DaemonState, network_id: &str) -> Option<Rate> {
    quota_rate(state, network_id, CHUNK_BYTES as u64, DEFAULT_PH).ok()
}

fn check_allowance(
    state: &DaemonState,
    network_id: &str,
    size: u64,
    ph: f64,
) -> Result<(), String> {
    let rate = quota_rate(state, network_id, size, ph)?;
    if rate.others == 0 {
        return Ok(());
    }
    let chains = state.chains.read().unwrap();
    let ledger = chains
        .get(network_id)
        .map(|c| c.ledger())
        .ok_or_else(|| format!("you are not in a network named '{network_id}'"))?;
    let me = &state.identity.fingerprint;
    let left = rate
        .quota(ledger.credit(me))
        .saturating_sub(rate.used(ledger.usage(me)));
    if size <= left && ledger.may_store(me, rate.on_others(size)) {
        return Ok(());
    }
    Err(format!(
        "not enough quota: this file is {size} bytes and your quota has {left} bytes left (k/n = {}/{}). Your quota is the space the others verified on your Pouch times k/n: hatch a Pouch (or a bigger one) and let them fill and verify it",
        rate.k, rate.n
    ))
}

pub struct FilePlan {
    layer: StorageTier,
    holders: Vec<String>,
    plan: coding_params::Plan,
}

fn plan_file(
    state: &DaemonState,
    network_id: &str,
    file_id: &str,
    largest_chunk: usize,
    ph: f64,
) -> Result<FilePlan, String> {
    let (layer, holders, plan) = choose_layer(
        &layer_nodes(state, network_id),
        file_id,
        |peers| availabilities(state, peers),
        largest_chunk,
        ph,
    )?;
    Ok(FilePlan {
        layer,
        holders,
        plan,
    })
}

fn register_placement(
    state: &DaemonState,
    network_id: &str,
    file_id: &str,
    file: &FilePlan,
    chunks: &[String],
) {
    let op = crate::ledger::Op::Place {
        file_id: file_id.to_string(),
        k: file.plan.k,
        layer: file.layer,
        chunks: chunks.to_vec(),
        holders: file.holders.iter().cloned().collect(),
    };
    if let Some(chain) = state.chains.write().unwrap().get_mut(network_id) {
        if let Err(e) = chain.propose(op, crate::chain::now_ms()) {
            tracing::warn!(file_id, "cannot register the placement: {e}");
        }
    }
}

async fn store_chunk(
    state: &DaemonState,
    network_id: &str,
    data: &[u8],
    grant: &Grant,
    file: &FilePlan,
    total: &mut Stored,
) -> Result<String, String> {
    let local = state
        .storage_managers
        .read()
        .unwrap()
        .values()
        .find(|sm| sm.read().unwrap().meta.network_id == network_id)
        .cloned();
    let me = state.identity.peer_id.to_string();
    let chunk_id = rlnc::chunk_hash(data);
    let (layer, nodes, plan) = (file.layer, &file.holders, &file.plan);
    let fragments = rlnc::encode(data, plan.k, plan.n).map_err(|e| format!("Encode error: {e}"))?;
    let owner = crate::storage::ChunkOwner {
        owner_key: grant.owner_key.clone(),
        file_id: grant.file_id.clone(),
        layer,
    };

    let mut pointers = Vec::new();
    for (node, fragment) in nodes.iter().zip(fragments.iter()) {
        let bytes = fragment.to_bytes();
        let commitment = Commitment::sign(
            &state.identity.device,
            &me,
            Held::Fragment {
                chunk_id: chunk_id.clone(),
                fragment_id: fragment.id.clone(),
            },
            node,
            &bytes,
        )
        .map_err(|e| e.to_string())?;
        if *node == me {
            let Some(sm) = &local else { continue };
            let result = sm
                .write()
                .unwrap()
                .store_fragment(fragment, &owner, &commitment);
            match result {
                Ok(()) => {
                    total.stored += 1;
                    pointers.push(FragmentPointer {
                        peer_id: me.clone(),
                        fragment_id: fragment.id.clone(),
                    });
                }
                Err(e) => tracing::warn!(chunk_id=%chunk_id, "store_fragment failed: {e}"),
            }
            continue;
        }
        let Ok(peer) = node.parse::<PeerId>() else {
            continue;
        };
        let sent = state
            .net_tx
            .send(NetworkCommand::PushFragment {
                peer_id: peer,
                chunk_id: chunk_id.clone(),
                fragment_id: fragment.id.clone(),
                data: bytes,
                grant: grant.clone(),
                layer,
                commitment,
            })
            .await;
        if sent.is_ok() {
            pointers.push(FragmentPointer {
                peer_id: node.clone(),
                fragment_id: fragment.id.clone(),
            });
        }
    }
    total.distributed += pointers.iter().filter(|p| p.peer_id != me).count();
    if !pointers.is_empty() {
        let ann = FragmentIndexAnnouncement {
            network_id: network_id.to_string(),
            chunk_id: chunk_id.clone(),
            announced_at: chrono::Utc::now().timestamp() as u64,
            pointers,
        };
        state
            .remote_fragment_index
            .write()
            .unwrap()
            .upsert(ann.clone());
        publish(state, network_id, Gossip::Index(ann)).await;
    }
    if total.chunks.is_empty() || plan.pe < total.pe {
        total.k = plan.k;
        total.n = plan.n;
        total.pe = plan.pe;
    }
    total.peers = total.peers.max(nodes.len());
    total.chunks.push(chunk_id.clone());
    Ok(chunk_id)
}

pub async fn fetch_chunk(
    state: &DaemonState,
    network_id: &str,
    chunk_id: &str,
    grant: &Grant,
) -> Result<(Vec<u8>, usize), String> {
    let mut fragments: Vec<EncodedFragment> = Vec::new();
    for sm in state.storage_managers.read().unwrap().values() {
        let sm = sm.read().unwrap();
        if sm.meta.network_id != network_id {
            continue;
        }
        for meta in sm.index.fragments_for_chunk(chunk_id) {
            match sm.load_fragment(chunk_id, &meta.fragment_id) {
                Ok(f) => fragments.push(f),
                Err(e) => tracing::warn!("load_fragment failed: {e}"),
            }
        }
    }

    let k = fragments.first().map_or(0, |f| f.k);
    let mut remote_count = 0;
    if fragments.is_empty() || fragments.len() < k {
        let local_peer = state.identity.peer_id.to_string();
        let peers: Vec<PeerId> = {
            let idx = state.remote_fragment_index.read().unwrap();
            let mut candidates: Vec<String> = idx
                .pointers_for(chunk_id)
                .iter()
                .map(|p| p.peer_id.clone())
                .collect();
            candidates.extend(
                state
                    .network_state
                    .read()
                    .unwrap()
                    .in_network(network_id)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch)
                    .map(|n| n.peer_id.clone()),
            );
            let mut seen = HashSet::new();
            candidates
                .into_iter()
                .filter(|p| *p != local_peer && seen.insert(p.clone()))
                .filter_map(|p| p.parse().ok())
                .collect()
        };

        let mut receivers = Vec::new();
        for peer in peers {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let sent = state
                .net_tx
                .send(NetworkCommand::FetchChunkFragments {
                    peer_id: peer,
                    chunk_id: chunk_id.to_string(),
                    grant: grant.clone(),
                    resp_tx: tx,
                })
                .await;
            if sent.is_ok() {
                receivers.push(rx);
            }
        }
        for rx in receivers {
            let timeout = std::time::Duration::from_secs(FETCH_TIMEOUT_SECS);
            if let Ok(Ok(FragmentResponse::FoundMany { fragments: found })) =
                tokio::time::timeout(timeout, rx).await
            {
                for (id, bytes) in found {
                    match EncodedFragment::from_bytes(id, chunk_id.to_string(), &bytes) {
                        Ok(f) => {
                            fragments.push(f);
                            remote_count += 1;
                        }
                        Err(e) => tracing::warn!("Remote fragment parse error: {e}"),
                    }
                }
            }
            let k = fragments.first().map_or(0, |f| f.k);
            if k > 0 && fragments.len() >= k {
                break;
            }
        }
    }

    if fragments.is_empty() {
        return Err(format!(
            "Chunk '{chunk_id}' not found locally or on remote peers"
        ));
    }
    let data = rlnc::decode(&fragments).map_err(|e| format!("Decode failed: {e}"))?;
    if rlnc::chunk_hash(&data) != chunk_id {
        return Err(format!("chunk '{chunk_id}' decoded to different data"));
    }
    Ok((data, remote_count))
}

async fn read_up_to(file: &mut tokio::fs::File, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; limit];
    let mut filled = 0;
    while filled < limit {
        let n = file.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

pub async fn put_file(
    state: &DaemonState,
    path: std::path::PathBuf,
    ph: Option<f64>,
    network_id: String,
    file_name: Option<String>,
) -> ControlResponse {
    let ph = ph.unwrap_or(DEFAULT_PH);
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => return ControlResponse::err(format!("cannot read {}: {e}", path.display())),
    };
    let size_on_disk = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    if let Err(e) = check_allowance(state, &network_id, size_on_disk, ph) {
        return ControlResponse::err(e);
    }
    let key = new_key();
    let cipher = ChunkCipher::from_raw_key(key);
    let file_id = new_file_id();
    let grant = match Grant::sign(&state.identity, &file_id, &state.identity.fingerprint) {
        Ok(g) => g,
        Err(e) => return ControlResponse::err(e.to_string()),
    };
    let largest_chunk = (size_on_disk as usize).clamp(1, CHUNK_BYTES);
    let file = match plan_file(state, &network_id, &file_id, largest_chunk, ph) {
        Ok(f) => f,
        Err(e) => return ControlResponse::err(e),
    };
    let mut total = Stored::default();
    let mut size = 0u64;
    loop {
        let plain = match read_up_to(&mut file, CHUNK_BYTES).await {
            Ok(p) => p,
            Err(e) => return ControlResponse::err(format!("cannot read {}: {e}", path.display())),
        };
        if plain.is_empty() && size > 0 {
            break;
        }
        if plain.is_empty() {
            return ControlResponse::err(format!("{} is empty", path.display()));
        }
        size += plain.len() as u64;
        let last = plain.len() < CHUNK_BYTES;
        let sealed = match cipher.encrypt(&plain) {
            Ok(c) => c,
            Err(e) => return ControlResponse::err(format!("Encryption error: {e}")),
        };
        if let Err(e) = store_chunk(state, &network_id, &sealed, &grant, &file, &mut total).await {
            if !total.chunks.is_empty() {
                send_order(state, &network_id, &file_id, Action::Delete).await;
            }
            return ControlResponse::err(e);
        }
        if last {
            break;
        }
    }
    let name = file_name.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&file_id)
            .to_string()
    });
    let manifest = match Manifest::new(
        &state.identity,
        &network_id,
        &name,
        size,
        &file_id,
        total.chunks.clone(),
        &key,
    ) {
        Ok(m) => m,
        Err(e) => return ControlResponse::err(e.to_string()),
    };
    register_placement(state, &network_id, &file_id, &file, &total.chunks);
    add_to_index(state, &network_id, manifest);
    publish_index(state, &network_id).await;
    ControlResponse::ok(PutFileData {
        message: format!(
            "Stored {} chunk(s) of '{name}': {} fragment(s) on this node, {} pushed to {} Pouch(es) — file_id: {file_id}",
            total.chunks.len(),
            total.stored,
            total.distributed,
            total.peers
        ),
        file_id,
        chunks: total.chunks.len(),
        size_bytes: size,
        k: total.k,
        n: total.n,
        ph,
        pe: total.pe,
        fragments_stored: total.stored,
        fragments_distributed: total.distributed,
    })
}

fn manifest_of(state: &DaemonState, network_id: &str, file_id: &str) -> Option<Manifest> {
    state
        .files
        .read()
        .unwrap()
        .index(network_id)?
        .files
        .get(file_id)
        .cloned()
}

fn not_yours(file_id: &str, network_id: &str) -> ControlResponse {
    ControlResponse::err(format!(
        "'{file_id}' is not one of your files on '{network_id}' (see bp ls)"
    ))
}

pub async fn get_file(
    state: &DaemonState,
    file_id: String,
    network_id: String,
    output: std::path::PathBuf,
) -> ControlResponse {
    let Some(manifest) = manifest_of(state, &network_id, &file_id) else {
        return not_yours(&file_id, &network_id);
    };
    let (grant, key) = match manifest
        .grant_for(&state.identity.fingerprint)
        .and_then(|g| Ok((g.clone(), manifest.file_key(&state.identity)?)))
    {
        Ok(r) => r,
        Err(e) => return ControlResponse::err(e.to_string()),
    };
    let cipher = ChunkCipher::from_raw_key(key);
    let partial = output.with_extension("bp-partial");
    let mut out = match tokio::fs::File::create(&partial).await {
        Ok(f) => f,
        Err(e) => return ControlResponse::err(format!("cannot write {}: {e}", partial.display())),
    };
    let mut fragments_remote = 0;
    for chunk_id in &manifest.chunks {
        let result = fetch_chunk(state, &network_id, chunk_id, &grant)
            .await
            .and_then(|(sealed, remote)| {
                fragments_remote += remote;
                cipher
                    .decrypt(&sealed)
                    .map_err(|e| format!("Decryption failed: {e}"))
            });
        let plain = match result {
            Ok(p) => p,
            Err(e) => {
                let _ = tokio::fs::remove_file(&partial).await;
                return ControlResponse::err(e);
            }
        };
        if let Err(e) = out.write_all(&plain).await {
            return ControlResponse::err(format!("cannot write {}: {e}", partial.display()));
        }
    }
    if let Err(e) = out
        .flush()
        .await
        .and(tokio::fs::rename(&partial, &output).await)
    {
        return ControlResponse::err(format!("cannot write {}: {e}", output.display()));
    }
    ControlResponse::ok(GetFileData {
        file_id,
        file_name: manifest.file_name,
        size_bytes: manifest.size_bytes,
        chunks: manifest.chunks.len(),
        fragments_remote,
        output: output.display().to_string(),
    })
}

fn member_key(state: &DaemonState, network_id: &str, member: &str) -> Option<String> {
    let chains = state.chains.read().unwrap();
    let ledger = chains.get(network_id)?.ledger();
    ledger
        .is_member(member)
        .then(|| ledger.members[member].key.clone())
}

pub async fn share_file(
    state: &DaemonState,
    network_id: String,
    file_id: String,
    member: String,
) -> ControlResponse {
    let Some(member_key) = member_key(state, &network_id, &member) else {
        return ControlResponse::err(format!("{member} is not a member of '{network_id}'"));
    };
    let Some(mut manifest) = manifest_of(state, &network_id, &file_id) else {
        return not_yours(&file_id, &network_id);
    };
    if let Err(e) = manifest.share_with(&state.identity, &member_key) {
        return ControlResponse::err(e.to_string());
    }
    let file_name = manifest.file_name.clone();
    replace_in_index(state, &network_id, manifest.clone());
    {
        let mut files = state.files.write().unwrap();
        files.outbox.push(PendingShare {
            network: network_id.clone(),
            to: member.clone(),
            manifest: manifest.clone(),
        });
        files.save();
    }
    publish(
        state,
        &network_id,
        Gossip::Share {
            to: member.clone(),
            manifest: Box::new(manifest),
        },
    )
    .await;
    publish_index(state, &network_id).await;
    ControlResponse::ok(ShareData {
        network_id,
        file_id,
        file_name,
        member,
    })
}

pub async fn unshare_file(
    state: &DaemonState,
    network_id: String,
    file_id: String,
    member: String,
) -> ControlResponse {
    let Some(mut manifest) = manifest_of(state, &network_id, &file_id) else {
        return not_yours(&file_id, &network_id);
    };
    if let Err(e) = manifest.revoke(&state.identity, &member) {
        return ControlResponse::err(e.to_string());
    }
    let file_name = manifest.file_name.clone();
    replace_in_index(state, &network_id, manifest);
    {
        let mut files = state.files.write().unwrap();
        files
            .outbox
            .retain(|s| !(s.manifest.file_id == file_id && s.to == member));
        files.save();
    }
    send_order(
        state,
        &network_id,
        &file_id,
        Action::Revoke {
            member: member.clone(),
        },
    )
    .await;
    publish_index(state, &network_id).await;
    ControlResponse::ok(ShareData {
        network_id,
        file_id,
        file_name,
        member,
    })
}

pub async fn delete_file(
    state: &DaemonState,
    network_id: String,
    file_id: String,
) -> ControlResponse {
    let Some(manifest) = manifest_of(state, &network_id, &file_id) else {
        return not_yours(&file_id, &network_id);
    };
    let mine = manifest.owner == state.identity.fingerprint;
    {
        let mut files = state.files.write().unwrap();
        if let Some(index) = files.indexes.get_mut(&network_id) {
            index.remove(&file_id);
        }
        files.outbox.retain(|s| s.manifest.file_id != file_id);
        files.save();
    }
    if mine {
        send_order(state, &network_id, &file_id, Action::Delete).await;
    }
    publish_index(state, &network_id).await;
    ControlResponse::ok(serde_json::json!({
        "network_id": network_id,
        "file_id": file_id,
        "file_name": manifest.file_name,
        "removed_from_network": mine,
    }))
}

async fn send_order(state: &DaemonState, network_id: &str, file_id: &str, action: Action) {
    if matches!(action, Action::Delete) {
        if let Some(chain) = state.chains.write().unwrap().get_mut(network_id) {
            let placed = chain.ledger().placements.contains_key(file_id);
            let drop = crate::ledger::Op::Drop {
                file_id: file_id.to_string(),
            };
            if placed {
                if let Err(e) = chain.propose(drop, crate::chain::now_ms()) {
                    tracing::warn!(file_id, "cannot drop the placement: {e}");
                }
            }
        }
    }
    let order = match Order::sign(&state.identity, file_id, action) {
        Ok(o) => o,
        Err(e) => return tracing::warn!("cannot sign order: {e}"),
    };
    apply_order(state, &order);
    state
        .files
        .write()
        .unwrap()
        .queue_order(network_id, order.clone());
    publish(
        state,
        network_id,
        Gossip::Order {
            order: Box::new(order),
        },
    )
    .await;
}

fn apply_order(state: &DaemonState, order: &Order) {
    let Some(owner) = order.owner() else {
        return;
    };
    for sm in state.storage_managers.read().unwrap().values() {
        let mut sm = sm.write().unwrap();
        let result = match &order.action {
            Action::Delete => sm
                .delete_file(&order.file_id, &order.owner_key)
                .map(|freed| freed > 0),
            Action::Revoke { member } => sm.revoke(&order.file_id, &order.owner_key, member),
        };
        match result {
            Ok(true) => {
                tracing::info!(file = %order.file_id, owner = %owner, action = ?order.action, "owner order applied")
            }
            Ok(false) => {}
            Err(e) => tracing::warn!("cannot apply order: {e}"),
        }
    }
}

pub fn list_files(state: &DaemonState, network_id: &str) -> ListFilesData {
    let files = state.files.read().unwrap();
    let entries: Vec<FileEntry> = files
        .indexes
        .iter()
        .filter(|(network, _)| network_id.is_empty() || network.as_str() == network_id)
        .flat_map(|(_, index)| index.files.values())
        .map(|m| FileEntry {
            file_name: m.file_name.clone(),
            size_bytes: m.size_bytes,
            file_id: m.file_id.clone(),
            chunks: m.chunks.len(),
            network_id: m.network.clone(),
            uploaded_at: m.uploaded_at,
            owner: m.owner.clone(),
            mine: m.owner == state.identity.fingerprint,
            shared_with: m.wraps.len().saturating_sub(1),
        })
        .collect();
    ListFilesData {
        total_files: entries.len(),
        total_bytes: entries.iter().map(|e| e.size_bytes).sum(),
        files: entries,
        network_id: network_id.to_string(),
    }
}

fn add_to_index(state: &DaemonState, network_id: &str, manifest: Manifest) -> bool {
    let mut files = state.files.write().unwrap();
    let changed = files
        .indexes
        .entry(network_id.to_string())
        .or_default()
        .add(manifest);
    files.save();
    changed
}

fn replace_in_index(state: &DaemonState, network_id: &str, manifest: Manifest) {
    let mut files = state.files.write().unwrap();
    files
        .indexes
        .entry(network_id.to_string())
        .or_default()
        .files
        .insert(manifest.file_id.clone(), manifest);
    files.save();
}

pub async fn publish_index(state: &DaemonState, network_id: &str) {
    let (index, previous) = {
        let files = state.files.read().unwrap();
        let index = files.index(network_id).cloned().unwrap_or_default();
        let previous = files
            .head(network_id, &state.identity.fingerprint)
            .map(|h| h.file_id.clone());
        (index, previous)
    };
    let version = index.version + 1;
    let blob = match (Index { version, ..index }).seal(&state.identity) {
        Ok(b) => b,
        Err(e) => return tracing::warn!("cannot seal the file index: {e}"),
    };
    let file_id = new_file_id();
    let grant = match Grant::sign(&state.identity, &file_id, &state.identity.fingerprint) {
        Ok(g) => g,
        Err(e) => return tracing::warn!("cannot sign the index grant: {e}"),
    };
    let file = match plan_file(
        state,
        network_id,
        &file_id,
        blob.len().clamp(1, CHUNK_BYTES),
        DEFAULT_PH,
    ) {
        Ok(f) => f,
        Err(e) => return tracing::warn!(network = %network_id, "cannot store the file index: {e}"),
    };
    let mut total = Stored::default();
    for piece in blob.chunks(CHUNK_BYTES) {
        if let Err(e) = store_chunk(state, network_id, piece, &grant, &file, &mut total).await {
            return tracing::warn!(network = %network_id, "cannot store the file index: {e}");
        }
    }
    register_placement(state, network_id, &file_id, &file, &total.chunks);
    let head = match Head::sign(&state.identity, network_id, version, &file_id, total.chunks) {
        Ok(h) => h,
        Err(e) => return tracing::warn!("cannot sign the index head: {e}"),
    };
    {
        let mut files = state.files.write().unwrap();
        files
            .indexes
            .entry(network_id.to_string())
            .or_default()
            .version = version;
        files.remember_head(head.clone());
        files.save();
    }
    publish(
        state,
        network_id,
        Gossip::Head {
            head: Box::new(head),
        },
    )
    .await;
    if let Some(previous) = previous {
        send_order(state, network_id, &previous, Action::Delete).await;
    }
}

async fn pull_own_index(state: &DaemonState, network_id: &str, head: &Head) {
    let grant = match Grant::sign(&state.identity, &head.file_id, &state.identity.fingerprint) {
        Ok(g) => g,
        Err(e) => return tracing::warn!("cannot sign a grant for own index: {e}"),
    };
    let mut blob = Vec::new();
    for chunk_id in &head.chunks {
        match fetch_chunk(state, network_id, chunk_id, &grant).await {
            Ok((piece, _)) => blob.extend_from_slice(&piece),
            Err(e) => {
                return tracing::debug!(network = %network_id, "own index not reachable yet: {e}")
            }
        }
    }
    let remote = match Index::open(&state.identity, &blob) {
        Ok(i) => i,
        Err(e) => return tracing::warn!("cannot open own index: {e}"),
    };
    let local_has_more = {
        let mut files = state.files.write().unwrap();
        let index = files.indexes.entry(network_id.to_string()).or_default();
        let mut merged = remote.clone();
        let local_has_more = merged.merge(index);
        index.merge(&remote);
        files.save();
        local_has_more
    };
    tracing::info!(network = %network_id, version = head.version, "file index synced");
    if local_has_more {
        publish_index(state, network_id).await;
    }
}

fn own_head_is_newer(state: &DaemonState, network_id: &str) -> Option<Head> {
    let files = state.files.read().unwrap();
    let head = files.head(network_id, &state.identity.fingerprint)?;
    let local = files.index(network_id).map_or(0, |i| i.version);
    (head.version > local).then(|| head.clone())
}

pub async fn on_gossip(state: &DaemonState, network_id: &str, gossip: Gossip) {
    let me = state.identity.fingerprint.clone();
    match gossip {
        Gossip::Head { head } => {
            if head.network != network_id {
                return;
            }
            let owner = state.files.write().unwrap().remember_head(*head);
            if owner.as_deref() == Some(me.as_str()) {
                if let Some(head) = own_head_is_newer(state, network_id) {
                    pull_own_index(state, network_id, &head).await;
                }
            }
        }
        Gossip::HeadRequest { owner } => {
            let head = state
                .files
                .read()
                .unwrap()
                .head(network_id, &owner)
                .cloned();
            if let Some(head) = head {
                publish(
                    state,
                    network_id,
                    Gossip::Head {
                        head: Box::new(head),
                    },
                )
                .await;
            }
        }
        Gossip::Share { to, manifest } => {
            if to != me
                || manifest.network != network_id
                || manifest.file_key(&state.identity).is_err()
            {
                return;
            }
            let file_id = manifest.file_id.clone();
            tracing::info!(file = %manifest.file_name, from = %manifest.owner, "file shared with you");
            if add_to_index(state, network_id, *manifest) {
                publish_index(state, network_id).await;
            }
            publish(
                state,
                network_id,
                Gossip::ShareAck {
                    file_id,
                    member: me,
                },
            )
            .await;
        }
        Gossip::ShareAck { file_id, member } => {
            let mut files = state.files.write().unwrap();
            let before = files.outbox.len();
            files.outbox.retain(|s| {
                !(s.network == network_id && s.to == member && s.manifest.file_id == file_id)
            });
            if files.outbox.len() != before {
                files.save();
            }
        }
        Gossip::Order { order } => {
            apply_order(state, &order);
            let revoked_for_me =
                matches!(&order.action, Action::Revoke { member } if *member == me);
            if matches!(order.action, Action::Delete) || revoked_for_me {
                let removed = {
                    let mut files = state.files.write().unwrap();
                    let removed = files.indexes.get_mut(network_id).is_some_and(|index| {
                        index
                            .files
                            .get(&order.file_id)
                            .is_some_and(|m| Some(m.owner.as_str()) == order.owner().as_deref())
                            && index.remove(&order.file_id).is_some()
                    });
                    files.save();
                    removed
                };
                if removed {
                    publish_index(state, network_id).await;
                }
            }
        }
        _ => {}
    }
}

pub async fn sync_cycle(state: &DaemonState) {
    let networks: Vec<String> = state.networks.read().unwrap().clone();
    let me = state.identity.fingerprint.clone();
    for network_id in networks {
        let member = state
            .chains
            .read()
            .unwrap()
            .get(&network_id)
            .is_some_and(|c| c.is_member(&me));
        if !member {
            continue;
        }
        if let Some(head) = own_head_is_newer(state, &network_id) {
            pull_own_index(state, &network_id, &head).await;
        }
        let head = state.files.read().unwrap().head(&network_id, &me).cloned();
        let message = match head {
            Some(head) => Gossip::Head {
                head: Box::new(head),
            },
            None => Gossip::HeadRequest { owner: me.clone() },
        };
        publish(state, &network_id, message).await;
        let (shares, orders) = {
            let mut files = state.files.write().unwrap();
            let shares: Vec<PendingShare> = files
                .outbox
                .iter()
                .filter(|s| s.network == network_id)
                .cloned()
                .collect();
            let mut orders = Vec::new();
            for pending in files.orders.iter_mut().filter(|o| o.network == network_id) {
                pending.sends += 1;
                orders.push(pending.order.clone());
            }
            files.orders.retain(|o| o.sends < ORDER_RESENDS);
            files.save();
            (shares, orders)
        };
        for share in shares {
            publish(
                state,
                &network_id,
                Gossip::Share {
                    to: share.to,
                    manifest: Box::new(share.manifest),
                },
            )
            .await;
        }
        for order in orders {
            publish(
                state,
                &network_id,
                Gossip::Order {
                    order: Box::new(order),
                },
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rate;

    #[test]
    fn the_quota_is_the_offer_times_k_over_n() {
        let rate = Rate {
            k: 3,
            n: 4,
            others: 3,
        };
        assert_eq!(rate.quota(10 << 30), (10u64 << 30) / 4 * 3);
        let file = 3_000_000;
        let on_others = rate.on_others(file);
        assert_eq!(on_others, 3_000_000);
        assert_eq!(rate.used(on_others), file);
    }

    use super::*;
    use std::collections::BTreeMap;

    fn node(peer: &str, tier: StorageTier, free: u64) -> LayerNode {
        LayerNode {
            peer: peer.into(),
            tier,
            free: tier.layers().map(|l| (l, free)).collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn files_go_to_the_lowest_layer_with_room_on_every_node() {
        let mut nodes: Vec<LayerNode> = (0..6)
            .map(|i| node(&format!("t1-{i}"), StorageTier::T1, 1 << 30))
            .collect();
        nodes.extend((0..3).map(|i| node(&format!("t2-{i}"), StorageTier::T2, 1 << 30)));
        let even = |peers: &[String]| vec![0.95; peers.len()];

        let (layer, peers, plan) = choose_layer(&nodes, "f", even, 4 << 20, 0.999).unwrap();
        assert_eq!(layer, StorageTier::T1);
        assert_eq!(peers.len(), 9, "T1 holds every node, T2 ones included");
        assert_eq!(plan.n, 9);

        nodes[0].free.insert(StorageTier::T1, 10);
        let (layer, peers, _) = choose_layer(&nodes, "f", even, 4 << 20, 0.999).unwrap();
        assert_eq!(layer, StorageTier::T2);
        assert!(peers.iter().all(|p| p.starts_with("t2-")));

        for n in nodes.iter_mut() {
            n.free.insert(StorageTier::T2, 10);
        }
        assert!(choose_layer(&nodes, "f", even, 4 << 20, 0.999).is_err());
        assert!(choose_layer(&[], "f", even, 1, 0.999).is_err());
    }
}

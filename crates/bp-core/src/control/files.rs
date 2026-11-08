use crate::{
    coding::{params as coding_params, rlnc, rlnc::EncodedFragment},
    control::{
        protocol::{
            ControlResponse, FileEntry, GetFileData, ListFilesData, PutFileData, ShareData,
        },
        server::DaemonState,
    },
    files::{seal_file, Grant, Head, Index, Manifest, PendingShare},
    network::{
        fragment_gossip::{FragmentIndexAnnouncement, FragmentPointer},
        FragmentResponse, Gossip, NetworkCommand,
    },
    service::ServiceType,
};
use libp2p::PeerId;
use std::collections::HashSet;

const DEFAULT_PH: f64 = 0.999;
const FETCH_TIMEOUT_SECS: u64 = 10;
const UNOBSERVED_STABILITY: f64 = 0.8;

pub struct StoredBlob {
    pub chunk_id: String,
    pub k: usize,
    pub n: usize,
    pub q: f64,
    pub pe: f64,
    pub stored: usize,
    pub distributed: usize,
    pub peers: usize,
}

pub fn rank(chunk_id: &str, peers: &mut [String]) {
    peers.sort_by_key(|peer| {
        let mut h = blake3::Hasher::new();
        h.update(chunk_id.as_bytes());
        h.update(peer.as_bytes());
        *h.finalize().as_bytes()
    });
}

pub async fn store_blob(
    state: &DaemonState,
    network_id: &str,
    data: &[u8],
    ph: f64,
    q_target: f64,
) -> Result<StoredBlob, String> {
    let local = state
        .storage_managers
        .read()
        .unwrap()
        .values()
        .find(|sm| sm.read().unwrap().meta.network_id == network_id)
        .cloned();
    let me = state.identity.peer_id.to_string();
    let mut nodes: Vec<String> = state
        .network_state
        .read()
        .unwrap()
        .in_network(network_id)
        .into_iter()
        .filter(|n| n.service_type == ServiceType::Pouch && n.peer_id != me)
        .map(|n| n.peer_id.clone())
        .collect();
    if local.is_some() {
        nodes.push(me.clone());
    }
    if nodes.is_empty() {
        return Err(format!(
            "No Pouches available on network '{network_id}' — hatch a Pouch or wait for Pouch peers to join"
        ));
    }
    let chunk_id = rlnc::chunk_hash(data);
    rank(&chunk_id, &mut nodes);
    let stabilities: Vec<f64> = {
        let qos = state.qos.read().unwrap();
        nodes
            .iter()
            .map(|p| match qos.get(p) {
                Some(q) if q.is_observed() => q.stability_score(),
                _ => UNOBSERVED_STABILITY,
            })
            .collect()
    };
    let plan = coding_params::plan(&stabilities, ph, q_target).map_err(|e| e.to_string())?;
    let fragments = rlnc::encode(data, plan.k, plan.n).map_err(|e| format!("Encode error: {e}"))?;
    let grant = Grant::sign(&state.identity, &chunk_id, &state.identity.fingerprint)
        .map_err(|e| e.to_string())?;

    let mut stored = 0;
    let mut pointers = Vec::new();
    let mut fragments = fragments.iter();
    for (node, &count) in nodes.iter().zip(&plan.per_node) {
        for fragment in fragments.by_ref().take(count) {
            if *node == me {
                let Some(sm) = &local else { continue };
                let result = sm
                    .write()
                    .unwrap()
                    .store_fragment(fragment, &state.identity.user_key);
                match result {
                    Ok(evicted) => {
                        stored += 1;
                        if !evicted.is_empty() {
                            publish(state, network_id, Gossip::FillerEvicted { blobs: evicted })
                                .await;
                        }
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
                    data: fragment.to_bytes(),
                    grant: grant.clone(),
                })
                .await;
            if sent.is_ok() {
                pointers.push(FragmentPointer {
                    peer_id: node.clone(),
                    fragment_id: fragment.id.clone(),
                });
            }
        }
    }
    let distributed = pointers.len();
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
    tracing::info!(chunk_id=%chunk_id, k = plan.k, n = plan.n, pe = plan.pe, stored, distributed, "blob stored");
    Ok(StoredBlob {
        chunk_id,
        k: plan.k,
        n: plan.n,
        q: plan.q(),
        pe: plan.pe,
        stored,
        distributed,
        peers: nodes.len(),
    })
}

pub async fn fetch_blob(
    state: &DaemonState,
    network_id: &str,
    chunk_id: &str,
    grant: &Grant,
) -> Result<(Vec<u8>, usize, usize), String> {
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
            let pointed: Vec<String> = idx
                .pointers_for(chunk_id)
                .iter()
                .map(|p| p.peer_id.clone())
                .collect();
            let candidates: Vec<String> = if pointed.is_empty() {
                state
                    .network_state
                    .read()
                    .unwrap()
                    .in_network(network_id)
                    .into_iter()
                    .filter(|n| n.service_type == ServiceType::Pouch)
                    .map(|n| n.peer_id.clone())
                    .collect()
            } else {
                pointed
            };
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
    let used = fragments.len();
    rlnc::decode(&fragments)
        .map(|data| (data, used, remote_count))
        .map_err(|e| format!("Decode failed: {e}"))
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

pub async fn put_file(
    state: &DaemonState,
    data: Vec<u8>,
    ph: Option<f64>,
    q_target: Option<f64>,
    network_id: String,
    file_name: Option<String>,
) -> ControlResponse {
    let ph = ph.unwrap_or(DEFAULT_PH);
    let sealed = match seal_file(&data) {
        Ok(s) => s,
        Err(e) => return ControlResponse::err(format!("Encryption error: {e}")),
    };
    let blob = match store_blob(
        state,
        &network_id,
        &sealed.ciphertext,
        ph,
        q_target.unwrap_or(1.0),
    )
    .await
    {
        Ok(b) => b,
        Err(e) => return ControlResponse::err(e),
    };
    let name = file_name.unwrap_or_else(|| blob.chunk_id[..12].to_string());
    let manifest = match Manifest::new(
        &state.identity,
        &network_id,
        &name,
        data.len() as u64,
        &blob.chunk_id,
        &sealed.key,
    ) {
        Ok(m) => m,
        Err(e) => return ControlResponse::err(e.to_string()),
    };
    add_to_index(state, &network_id, manifest);
    publish_index(state, &network_id).await;
    ControlResponse::ok(PutFileData {
        message: format!(
            "Stored {}/{} locally, pushed {} to {} remote pouch(es) — chunk_id: {}",
            blob.stored, blob.n, blob.distributed, blob.peers, blob.chunk_id
        ),
        chunk_id: blob.chunk_id,
        k: blob.k,
        n: blob.n,
        q: blob.q,
        ph,
        pe: blob.pe,
        fragments_stored: blob.stored,
        fragments_distributed: blob.distributed,
    })
}

fn manifest_of(state: &DaemonState, network_id: &str, chunk_id: &str) -> Option<Manifest> {
    state
        .files
        .read()
        .unwrap()
        .index(network_id)?
        .files
        .get(chunk_id)
        .cloned()
}

pub async fn get_file(
    state: &DaemonState,
    chunk_id: String,
    network_id: String,
) -> ControlResponse {
    let Some(manifest) = manifest_of(state, &network_id, &chunk_id) else {
        return ControlResponse::err(format!(
            "'{chunk_id}' is not one of your files on '{network_id}' (see bp ls)"
        ));
    };
    let grant = match manifest.grant_for(&state.identity.fingerprint) {
        Ok(g) => g.clone(),
        Err(e) => return ControlResponse::err(e.to_string()),
    };
    let (ciphertext, fragments_used, fragments_remote) =
        match fetch_blob(state, &network_id, &chunk_id, &grant).await {
            Ok(r) => r,
            Err(e) => return ControlResponse::err(e),
        };
    match manifest.open(&state.identity, &ciphertext) {
        Ok(data) => ControlResponse::ok(GetFileData {
            chunk_id,
            data,
            fragments_used,
            fragments_remote,
        }),
        Err(e) => ControlResponse::err(format!("Decryption failed: {e}")),
    }
}

pub async fn share_file(
    state: &DaemonState,
    network_id: String,
    chunk_id: String,
    member: String,
) -> ControlResponse {
    let member_key = {
        let chains = state.chains.read().unwrap();
        let ledger = chains.get(&network_id).map(|c| c.ledger());
        match ledger.and_then(|l| l.is_member(&member).then(|| l.members[&member].key.clone())) {
            Some(key) => key,
            None => {
                return ControlResponse::err(format!("{member} is not a member of '{network_id}'"))
            }
        }
    };
    let Some(mut manifest) = manifest_of(state, &network_id, &chunk_id) else {
        return ControlResponse::err(format!(
            "'{chunk_id}' is not one of your files on '{network_id}' (see bp ls)"
        ));
    };
    if let Err(e) = manifest.share_with(&state.identity, &member_key) {
        return ControlResponse::err(e.to_string());
    }
    let file_name = manifest.file_name.clone();
    add_to_index(state, &network_id, manifest.clone());
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
        chunk_id,
        file_name,
        member,
    })
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
            chunk_id: m.chunk_id.clone(),
            network_id: m.network.clone(),
            uploaded_at: m.uploaded_at,
            owner: m.owner.clone(),
            mine: m.owner == state.identity.fingerprint,
            shared_with: m.wraps.len() - 1,
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

pub async fn publish_index(state: &DaemonState, network_id: &str) {
    let (index, version) = {
        let files = state.files.read().unwrap();
        let index = files.index(network_id).cloned().unwrap_or_default();
        let version = index.version + 1;
        (index, version)
    };
    let sealed = Index { version, ..index }.seal(&state.identity);
    let blob = match sealed {
        Ok(b) => b,
        Err(e) => return tracing::warn!("cannot seal the file index: {e}"),
    };
    let stored = match store_blob(state, network_id, &blob, DEFAULT_PH, 1.0).await {
        Ok(s) => s,
        Err(e) => return tracing::warn!(network = %network_id, "cannot store the file index: {e}"),
    };
    let head = match Head::sign(&state.identity, network_id, version, &stored.chunk_id) {
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
}

async fn pull_own_index(state: &DaemonState, network_id: &str, head: &Head) {
    let grant = match Grant::sign(&state.identity, &head.chunk_id, &state.identity.fingerprint) {
        Ok(g) => g,
        Err(e) => return tracing::warn!("cannot sign a grant for own index: {e}"),
    };
    let blob = match fetch_blob(state, network_id, &head.chunk_id, &grant).await {
        Ok((blob, _, _)) => blob,
        Err(e) => return tracing::debug!(network = %network_id, "own index not reachable yet: {e}"),
    };
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
            let chunk_id = manifest.chunk_id.clone();
            tracing::info!(file = %manifest.file_name, from = %manifest.owner, "file shared with you");
            if add_to_index(state, network_id, *manifest) {
                publish_index(state, network_id).await;
            }
            publish(
                state,
                network_id,
                Gossip::ShareAck {
                    chunk_id,
                    member: me,
                },
            )
            .await;
        }
        Gossip::ShareAck { chunk_id, member } => {
            let mut files = state.files.write().unwrap();
            let before = files.outbox.len();
            files.outbox.retain(|s| {
                !(s.network == network_id && s.to == member && s.manifest.chunk_id == chunk_id)
            });
            if files.outbox.len() != before {
                files.save();
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
        let pending: Vec<PendingShare> = state
            .files
            .read()
            .unwrap()
            .outbox
            .iter()
            .filter(|s| s.network == network_id)
            .cloned()
            .collect();
        for share in pending {
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
    }
}

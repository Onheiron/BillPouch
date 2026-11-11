use super::{
    protocol::{ChatData, ChatLine},
    server::{member_name, DaemonState},
    ControlResponse,
};
use crate::{
    chat::{ChatMessage, SKEW_SECS, SYNC_EVERY_SECS},
    network::{Gossip, NetworkCommand},
};
use rand::seq::SliceRandom;

fn now() -> u64 {
    crate::chain::now_ms() / 1000
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

fn room(state: &DaemonState, network_id: &str) -> Option<String> {
    let chains = state.chains.read().unwrap();
    let chain = chains.get(network_id)?;
    chain
        .is_member(&state.identity.fingerprint)
        .then(|| chain.id())
}

fn keep(state: &DaemonState, network_id: &str, messages: Vec<ChatMessage>) {
    let authentic: Vec<ChatMessage> = {
        let chains = state.chains.read().unwrap();
        let Some(chain) = chains.get(network_id) else {
            return;
        };
        let (room, now) = (chain.id(), now());
        messages
            .into_iter()
            .filter(|m| m.is_authentic(chain.ledger(), &room, now))
            .collect()
    };
    let mut chat = state.chat.write().unwrap();
    let added = authentic
        .into_iter()
        .filter(|m| chat.add(network_id, m.clone()))
        .count();
    if added > 0 {
        chat.save();
    }
}

pub async fn send(state: &DaemonState, network_id: String, text: String) -> ControlResponse {
    let Some(room) = room(state, &network_id) else {
        return ControlResponse::err(format!("you are not a member of '{network_id}'"));
    };
    let message = match ChatMessage::write(&state.identity, &room, &text, now()) {
        Ok(message) => message,
        Err(e) => return ControlResponse::err(e),
    };
    keep(state, &network_id, vec![message.clone()]);
    publish(
        state,
        &network_id,
        Gossip::Chat {
            message: Box::new(message),
        },
    )
    .await;
    history(state, network_id, None)
}

pub fn history(state: &DaemonState, network_id: String, known: Option<u64>) -> ControlResponse {
    let chains = state.chains.read().unwrap();
    let Some(chain) = chains.get(&network_id) else {
        return ControlResponse::err(format!("you are not in a network named '{network_id}'"));
    };
    let chat = state.chat.read().unwrap();
    let revision = chat.revision();
    let me = &state.identity.fingerprint;
    let messages = (known != Some(revision)).then(|| {
        chat.messages(&network_id)
            .iter()
            .map(|m| ChatLine {
                id: m.id(),
                name: chain
                    .ledger()
                    .members
                    .get(&m.author)
                    .map(|member| member_name(&member.key))
                    .unwrap_or_default(),
                author: m.author.clone(),
                at: m.at,
                text: m.text.clone(),
                mine: &m.author == me,
            })
            .collect()
    });
    ControlResponse::ok(ChatData {
        network_id: network_id.clone(),
        revision,
        messages,
    })
}

pub async fn on_gossip(state: &DaemonState, network_id: &str, gossip: Gossip) {
    match gossip {
        Gossip::Chat { message } => keep(state, network_id, vec![*message]),
        Gossip::ChatHistory { messages } => {
            keep(state, network_id, messages);
            let mut chat = state.chat.write().unwrap();
            chat.mark_synced(network_id, now());
            chat.save();
        }
        Gossip::ChatRequest { to, since } => {
            if to != state.identity.peer_id.to_string() || room(state, network_id).is_none() {
                return;
            }
            let batches = state.chat.read().unwrap().batches_since(network_id, since);
            for messages in batches {
                publish(state, network_id, Gossip::ChatHistory { messages }).await;
            }
        }
        _ => {}
    }
}

pub async fn sync_cycle(state: &DaemonState) {
    let networks: Vec<String> = state.networks.read().unwrap().clone();
    let me = state.identity.peer_id.to_string();
    for network_id in networks {
        let synced_at = state.chat.read().unwrap().synced_at(&network_id);
        if synced_at + SYNC_EVERY_SECS > now() || room(state, &network_id).is_none() {
            continue;
        }
        let peers: Vec<String> = state
            .network_state
            .read()
            .unwrap()
            .in_network(&network_id)
            .into_iter()
            .map(|n| n.peer_id.clone())
            .filter(|peer| peer != &me)
            .collect();
        let chosen = peers.choose(&mut rand::thread_rng()).cloned();
        if let Some(to) = chosen {
            let request = Gossip::ChatRequest {
                to,
                since: synced_at.saturating_sub(SKEW_SECS),
            };
            publish(state, &network_id, request).await;
        }
    }
}

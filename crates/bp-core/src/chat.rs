use crate::{
    config,
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
    ledger::Ledger,
    wire,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_TEXT_CHARS: usize = 1000;
pub const KEPT_PER_NETWORK: usize = 500;
pub const SYNC_EVERY_SECS: u64 = 600;
pub const SKEW_SECS: u64 = 300;
const BATCH_BYTES: usize = 40 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub author: String,
    pub at: u64,
    pub text: String,
    #[serde(with = "crate::wire::hex_bytes")]
    pub signature: String,
}

impl ChatMessage {
    pub fn write(identity: &Identity, room: &str, text: &str, at: u64) -> BpResult<Self> {
        let text = text.trim().to_string();
        if !valid_text(&text) {
            return Err(BpError::InvalidInput(format!(
                "a message has between 1 and {MAX_TEXT_CHARS} characters"
            )));
        }
        let signature = identity
            .user
            .sign(&signed_bytes(room, &identity.fingerprint, at, &text))
            .map_err(|e| BpError::Identity(format!("cannot sign the message: {e}")))?;
        Ok(Self {
            author: identity.fingerprint.clone(),
            at,
            text,
            signature: hex::encode(signature),
        })
    }

    pub fn id(&self) -> String {
        wire::digest(self)[..16].to_string()
    }

    pub fn is_authentic(&self, ledger: &Ledger, room: &str, now: u64) -> bool {
        let Some(member) = ledger.members.get(&self.author) else {
            return false;
        };
        let Ok((public, fingerprint)) = parse_user_key(&member.key) else {
            return false;
        };
        let Ok(signature) = hex::decode(&self.signature) else {
            return false;
        };
        ledger.is_member(&self.author)
            && fingerprint == self.author
            && valid_text(&self.text)
            && self.at <= now + SKEW_SECS
            && public.verify(
                &signed_bytes(room, &self.author, self.at, &self.text),
                &signature,
            )
    }
}

fn valid_text(text: &str) -> bool {
    !text.is_empty() && text.chars().count() <= MAX_TEXT_CHARS && text.trim() == text
}

fn signed_bytes(room: &str, author: &str, at: u64, text: &str) -> Vec<u8> {
    wire::signing_bytes(b"billpouch/chat/v1", &(room, author, at, text))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ChatStore {
    rooms: BTreeMap<String, Vec<ChatMessage>>,
    synced_at: BTreeMap<String, u64>,
    #[serde(skip)]
    revision: u64,
    #[serde(skip)]
    persistent: bool,
}

impl ChatStore {
    pub fn load(now: u64) -> Self {
        let mut store: Self = config::chat_path()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| wire::decode(&b).ok())
            .unwrap_or_default();
        store.persistent = true;
        store.revision = now;
        store
    }

    pub fn save(&self) {
        if !self.persistent {
            return;
        }
        let result = config::chat_path().and_then(|p| {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, wire::encode(self)?)?;
            Ok(())
        });
        if let Err(e) = result {
            tracing::warn!("cannot save the chat: {e}");
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn messages(&self, network: &str) -> &[ChatMessage] {
        self.rooms.get(network).map_or(&[], Vec::as_slice)
    }

    pub fn add(&mut self, network: &str, message: ChatMessage) -> bool {
        let room = self.rooms.entry(network.to_string()).or_default();
        let key = (message.at, message.id());
        let Err(place) = room.binary_search_by(|m| (m.at, m.id()).cmp(&key)) else {
            return false;
        };
        if place == 0 && room.len() >= KEPT_PER_NETWORK {
            return false;
        }
        room.insert(place, message);
        if room.len() > KEPT_PER_NETWORK {
            room.remove(0);
        }
        self.revision += 1;
        true
    }

    pub fn forget(&mut self, network: &str) {
        self.rooms.remove(network);
        self.synced_at.remove(network);
        self.revision += 1;
    }

    pub fn written_by(&self, network: &str) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for message in self.messages(network) {
            *counts.entry(message.author.as_str()).or_default() += 1;
        }
        counts
    }

    pub fn synced_at(&self, network: &str) -> u64 {
        self.synced_at.get(network).copied().unwrap_or(0)
    }

    pub fn mark_synced(&mut self, network: &str, now: u64) {
        self.synced_at.insert(network.to_string(), now);
    }

    pub fn batches_since(&self, network: &str, since: u64) -> Vec<Vec<ChatMessage>> {
        let mut batches = vec![Vec::new()];
        let mut size = 0;
        for message in self.messages(network).iter().filter(|m| m.at >= since) {
            let bytes = wire::encode(message).map_or(0, |b| b.len());
            if size + bytes > BATCH_BYTES {
                batches.push(Vec::new());
                size = 0;
            }
            size += bytes;
            batches
                .last_mut()
                .expect("never empty")
                .push(message.clone());
        }
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::membership::genesis;

    fn room() -> (Identity, Ledger, String) {
        let founder = Identity::in_memory(None).unwrap();
        let (root, _) = genesis(&founder, "amici").unwrap();
        let id = root.id();
        let ledger = Ledger::new(root, &founder.cert).unwrap();
        (founder, ledger, id)
    }

    #[test]
    fn a_message_is_authentic_only_from_a_member_in_its_own_room() {
        let (founder, ledger, id) = room();
        let now = ledger.time;
        let message = ChatMessage::write(&founder, &id, "  ciao a tutti \n", now).unwrap();
        assert_eq!(message.text, "ciao a tutti");
        assert!(message.is_authentic(&ledger, &id, now));
        assert!(!message.is_authentic(&ledger, "another network", now));

        let mut edited = message.clone();
        edited.text = "ciao a nessuno".into();
        assert!(!edited.is_authentic(&ledger, &id, now));

        let stranger = Identity::in_memory(None).unwrap();
        let outside = ChatMessage::write(&stranger, &id, "posso?", now).unwrap();
        assert!(!outside.is_authentic(&ledger, &id, now));

        let future = ChatMessage::write(&founder, &id, "domani", now + SKEW_SECS + 1).unwrap();
        assert!(!future.is_authentic(&ledger, &id, now));
    }

    #[test]
    fn empty_and_endless_messages_are_refused() {
        let (founder, _, id) = room();
        assert!(ChatMessage::write(&founder, &id, "   ", 1).is_err());
        assert!(ChatMessage::write(&founder, &id, &"a".repeat(MAX_TEXT_CHARS + 1), 1).is_err());
        assert!(ChatMessage::write(&founder, &id, &"è".repeat(MAX_TEXT_CHARS), 1).is_ok());
    }

    #[test]
    fn the_store_orders_by_time_drops_duplicates_and_keeps_the_newest() {
        let (founder, _, id) = room();
        let mut store = ChatStore::default();
        let write = |at: u64| ChatMessage::write(&founder, &id, &format!("m{at}"), at).unwrap();
        let before = store.revision();
        assert!(store.add("amici", write(20)));
        assert!(store.add("amici", write(10)));
        assert!(!store.add("amici", write(10)));
        assert_eq!(store.revision(), before + 2);
        let texts: Vec<_> = store.messages("amici").iter().map(|m| &m.text).collect();
        assert_eq!(texts, ["m10", "m20"]);

        for at in 100..100 + KEPT_PER_NETWORK as u64 {
            store.add("amici", write(at));
        }
        assert_eq!(store.messages("amici").len(), KEPT_PER_NETWORK);
        assert_eq!(store.messages("amici")[0].text, "m100");
        assert!(!store.add("amici", write(5)));
        assert_eq!(
            store.written_by("amici")[founder.fingerprint.as_str()],
            KEPT_PER_NETWORK
        );
    }

    #[test]
    fn history_is_cut_in_batches_that_fit_a_gossip_message() {
        let (founder, _, id) = room();
        let mut store = ChatStore::default();
        let long = "x".repeat(MAX_TEXT_CHARS);
        for at in 0..200 {
            store.add(
                "amici",
                ChatMessage::write(&founder, &id, &long, at).unwrap(),
            );
        }
        let batches = store.batches_since("amici", 100);
        assert!(batches.len() > 1);
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 100);
        assert!(batches
            .iter()
            .all(|b| wire::encode(b).unwrap().len() < 60 * 1024));
        assert_eq!(store.batches_since("nobody", 0), vec![Vec::new()]);
    }
}

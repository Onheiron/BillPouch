use crate::{
    config,
    consensus::{epoch_of, Commit, ConsensusMsg, Engine},
    error::{BpError, BpResult},
    identity::{DeviceCert, Identity},
    ledger::{Ledger, Observation, Op, SignedOp, EPOCH_SECS, MAX_CHALLENGES_PER_DEVICE_PER_EPOCH},
    membership::{
        genesis, invite_entry, key_commitment, new_key, open_key, wrap_for, Entry, Invite,
    },
    network::ReputationTier,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

pub const SYNC_BATCH: usize = 100;
const RESUBMIT_MS: u64 = 60_000;
const SYNC_RETRY_MS: u64 = 5_000;
const REPORT_AT: f64 = 0.9;
const ROTATION_SLOT_SECS: u64 = 60;

pub type Chains = Arc<RwLock<HashMap<String, Chain>>>;

#[derive(Debug, Clone, Copy, Default)]
pub struct Tally {
    pub epoch: u64,
    pub sent: u32,
    pub passed: u32,
}

pub type ObservationLog = Arc<RwLock<HashMap<String, Tally>>>;

pub fn observe(log: &ObservationLog, device: &str, passed: bool, now_ms: u64) {
    let epoch = epoch_of(now_ms);
    if let Ok(mut log) = log.write() {
        let tally = log.entry(device.to_string()).or_default();
        if tally.epoch != epoch {
            *tally = Tally {
                epoch,
                ..Tally::default()
            };
        }
        tally.sent += 1;
        tally.passed += u32::from(passed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpelOutcome {
    Proposed,
    Approved,
    Expelled,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    genesis: Entry,
    creator_device: DeviceCert,
    own_entry: Entry,
    commits: Vec<Commit>,
}

pub struct Chain {
    pub name: String,
    pub key: [u8; 32],
    pub engine: Engine,
    me: Identity,
    genesis: Entry,
    creator_device: DeviceCert,
    own_entry: Entry,
    commits: Vec<Commit>,
    device_submitted_ms: u64,
    rotation_submitted_ms: u64,
    reported_epoch: Option<u64>,
    sync_requested_ms: u64,
    persistent: bool,
}

impl Chain {
    pub fn create(identity: &Identity, name: &str) -> BpResult<Self> {
        let (root, key) = genesis(identity, name)?;
        Self::build(identity, root.clone(), identity.cert.clone(), root, key)
    }

    pub fn from_invite(identity: &Identity, invite: Invite) -> BpResult<Self> {
        let key = invite.entry.unwrap_key(identity)?;
        Self::build(
            identity,
            invite.genesis,
            invite.creator_device,
            invite.entry,
            key,
        )
    }

    fn build(
        identity: &Identity,
        genesis: Entry,
        creator_device: DeviceCert,
        own_entry: Entry,
        key: [u8; 32],
    ) -> BpResult<Self> {
        let ledger = Ledger::new(genesis.clone(), &creator_device)?;
        Ok(Self {
            name: genesis.body.network.clone(),
            key,
            engine: Engine::new(identity, ledger),
            me: identity.clone(),
            genesis,
            creator_device,
            own_entry,
            commits: Vec::new(),
            device_submitted_ms: 0,
            rotation_submitted_ms: 0,
            reported_epoch: None,
            sync_requested_ms: 0,
            persistent: false,
        })
    }

    pub fn id(&self) -> String {
        self.genesis.id()
    }

    pub fn ledger(&self) -> &Ledger {
        &self.engine.ledger
    }

    pub fn height(&self) -> u64 {
        self.engine.ledger.height
    }

    pub fn is_member(&self, user: &str) -> bool {
        self.engine.ledger.is_member(user)
    }

    pub fn tier(&self, user: &str) -> Option<ReputationTier> {
        self.engine.ledger.snapshot.tier(user)
    }

    pub fn device_tier(&self, device: &str) -> Option<ReputationTier> {
        self.tier(self.engine.ledger.device_owner(device)?)
    }

    pub fn invite(&mut self, member_key: &str, now_ms: u64) -> BpResult<Invite> {
        let entry = invite_entry(&self.me, &self.genesis, &self.key, member_key)?;
        let op = SignedOp::sign(
            &self.me,
            Op::AddMember {
                entry: entry.clone(),
            },
        )?;
        self.engine.submit(op, now_ms)?;
        self.record();
        Ok(Invite {
            genesis: self.genesis.clone(),
            creator_device: self.creator_device.clone(),
            entry,
        })
    }

    pub fn own_invite(&self) -> Invite {
        Invite {
            genesis: self.genesis.clone(),
            creator_device: self.creator_device.clone(),
            entry: self.own_entry.clone(),
        }
    }

    pub fn key_is_current(&self) -> bool {
        key_commitment(&self.key) == self.engine.ledger.commitment
    }

    pub fn expel(&mut self, member: &str, now_ms: u64) -> BpResult<()> {
        let op = if self.engine.ledger.expel_approvals.contains_key(member) {
            Op::ApproveExpel {
                member: member.to_string(),
            }
        } else {
            Op::ProposeExpel {
                member: member.to_string(),
            }
        };
        let op = SignedOp::sign(&self.me, op)?;
        self.engine.submit(op, now_ms)?;
        self.record();
        Ok(())
    }

    pub fn expel_outcome(&self, member: &str) -> Option<ExpelOutcome> {
        let ledger = &self.engine.ledger;
        if ledger.expelled.contains(member) {
            return Some(ExpelOutcome::Expelled);
        }
        let approvals = ledger.expel_approvals.get(member)?;
        let outcome = if approvals.len() == 1 {
            ExpelOutcome::Proposed
        } else {
            ExpelOutcome::Approved
        };
        approvals.contains(&self.me.fingerprint).then_some(outcome)
    }

    pub fn handle(&mut self, msg: ConsensusMsg, now_ms: u64) -> bool {
        let behind = matches!(&msg, ConsensusMsg::Commit { commit } if commit.block.height > self.height() + 1);
        self.engine.handle(msg, now_ms);
        self.record();
        behind && self.want_sync(now_ms)
    }

    pub fn tick(&mut self, log: &ObservationLog, now_ms: u64) {
        self.register_device(now_ms);
        self.report(log, now_ms);
        self.rotate(now_ms);
        self.engine.tick(now_ms);
        self.record();
    }

    pub fn outbox(&mut self) -> Vec<ConsensusMsg> {
        self.engine.drain()
    }

    pub fn commits_from(&self, from: u64) -> Vec<Commit> {
        self.commits
            .iter()
            .skip(from as usize)
            .take(SYNC_BATCH)
            .cloned()
            .collect()
    }

    pub fn apply_synced(&mut self, commits: Vec<Commit>, now_ms: u64) {
        for commit in commits {
            if commit.block.height == self.height() + 1 {
                self.engine.handle(
                    ConsensusMsg::Commit {
                        commit: Box::new(commit),
                    },
                    now_ms,
                );
            }
        }
        self.record();
        self.sync_requested_ms = 0;
    }

    fn want_sync(&mut self, now_ms: u64) -> bool {
        if now_ms < self.sync_requested_ms + SYNC_RETRY_MS {
            return false;
        }
        self.sync_requested_ms = now_ms;
        true
    }

    fn record(&mut self) {
        let new = self.engine.take_commits();
        if new.is_empty() {
            return;
        }
        self.commits.extend(new);
        self.refresh_key();
        if !self.persistent {
            return;
        }
        if let Err(e) = self.save() {
            tracing::warn!(network = %self.name, "cannot save ledger: {e}");
        }
    }

    fn refresh_key(&mut self) {
        let ledger = &self.engine.ledger;
        if key_commitment(&self.key) == ledger.commitment {
            return;
        }
        let Some(wrapped) = ledger.key_wraps.get(&self.me.fingerprint) else {
            return;
        };
        match open_key(wrapped, &ledger.commitment, &self.me) {
            Ok(key) => {
                self.key = key;
                tracing::info!(network = %self.name, "network key rotated");
            }
            Err(e) => tracing::warn!(network = %self.name, "cannot open the rotated key: {e}"),
        }
    }

    fn submit(&mut self, op: Op, now_ms: u64, what: &str) {
        let result = SignedOp::sign(&self.me, op).and_then(|op| self.engine.submit(op, now_ms));
        if let Err(e) = result {
            tracing::warn!(network = %self.name, "cannot {what}: {e}");
        }
    }

    fn register_device(&mut self, now_ms: u64) {
        let ledger = &self.engine.ledger;
        if !ledger.is_member(&self.me.fingerprint)
            || ledger.devices.contains_key(&self.me.peer_id.to_string())
            || now_ms < self.device_submitted_ms + RESUBMIT_MS
        {
            return;
        }
        self.device_submitted_ms = now_ms;
        let cert = self.me.cert.clone();
        self.submit(Op::RegisterDevice { cert }, now_ms, "register this device");
    }

    fn report(&mut self, log: &ObservationLog, now_ms: u64) {
        let epoch = epoch_of(now_ms);
        let into_epoch = (now_ms / 1000 % EPOCH_SECS) as f64 / EPOCH_SECS as f64;
        let ledger = &self.engine.ledger;
        let me = &self.me.fingerprint;
        if into_epoch < REPORT_AT
            || self.reported_epoch == Some(epoch)
            || !ledger.is_member(me)
            || ledger.reported.contains(&(me.clone(), epoch))
        {
            return;
        }
        self.reported_epoch = Some(epoch);
        let results: Vec<Observation> = log
            .read()
            .map(|log| {
                log.iter()
                    .filter(|(_, t)| t.epoch == epoch && t.sent > 0)
                    .filter_map(|(device, t)| {
                        let user = ledger.device_owner(device)?;
                        (user != me && ledger.is_member(user)).then(|| {
                            let sent = t.sent.min(MAX_CHALLENGES_PER_DEVICE_PER_EPOCH);
                            Observation {
                                device: device.clone(),
                                user: user.to_string(),
                                sent,
                                passed: t.passed.min(sent),
                            }
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !results.is_empty() {
            self.submit(
                Op::Observations { epoch, results },
                now_ms,
                "report observations",
            );
        }
    }

    fn rotation_turn(&self, now_secs: u64) -> bool {
        let ledger = &self.engine.ledger;
        let Some(since) = ledger.rotation_due(now_secs) else {
            return false;
        };
        let rotators: Vec<&String> = ledger
            .snapshot
            .users
            .iter()
            .filter(|(user, s)| s.weight > 0 && ledger.voting_device(user).is_some())
            .map(|(user, _)| user)
            .collect();
        if rotators.is_empty() {
            return false;
        }
        let slot = now_secs.saturating_sub(since) / ROTATION_SLOT_SECS;
        let turn = (now_secs / EPOCH_SECS + slot) % rotators.len() as u64;
        rotators[turn as usize] == &self.me.fingerprint
            && ledger.voting_device(&self.me.fingerprint) == Some(&self.me.peer_id.to_string())
    }

    fn rotate(&mut self, now_ms: u64) {
        if now_ms < self.rotation_submitted_ms + RESUBMIT_MS || !self.rotation_turn(now_ms / 1000) {
            return;
        }
        self.rotation_submitted_ms = now_ms;
        let key = new_key();
        let wrapped: BpResult<BTreeMap<_, _>> = self
            .engine
            .ledger
            .active_members()
            .map(|(user, member)| Ok((user.clone(), wrap_for(&key, &member.key)?)))
            .collect();
        match wrapped {
            Ok(wrapped) => self.submit(
                Op::RotateKey {
                    epoch: epoch_of(now_ms),
                    wrapped,
                    commitment: key_commitment(&key),
                },
                now_ms,
                "rotate the network key",
            ),
            Err(e) => tracing::warn!(network = %self.name, "cannot wrap the new key: {e}"),
        }
    }

    pub fn save(&mut self) -> BpResult<()> {
        self.persistent = true;
        let dir = config::networks_dir()?;
        std::fs::create_dir_all(&dir)?;
        let stored = Stored {
            genesis: self.genesis.clone(),
            creator_device: self.creator_device.clone(),
            own_entry: self.own_entry.clone(),
            commits: self.commits.clone(),
        };
        std::fs::write(
            dir.join(format!("{}.json", self.name)),
            serde_json::to_vec(&stored)?,
        )?;
        Ok(())
    }

    fn restore(identity: &Identity, stored: Stored, now_ms: u64) -> BpResult<Self> {
        let key = stored.own_entry.unwrap_key(identity)?;
        let mut chain = Self::build(
            identity,
            stored.genesis,
            stored.creator_device,
            stored.own_entry,
            key,
        )?;
        let total = stored.commits.len();
        chain.apply_synced(stored.commits, now_ms);
        chain.engine.drain();
        chain.persistent = true;
        if chain.commits.len() != total {
            return Err(BpError::InvalidInput(format!(
                "ledger of '{}' does not replay past height {}",
                chain.name,
                chain.height()
            )));
        }
        Ok(chain)
    }

    pub fn load_all(identity: &Identity, now_ms: u64) -> BpResult<HashMap<String, Chain>> {
        let dir = config::networks_dir()?;
        let mut all = HashMap::new();
        if !dir.exists() {
            return Ok(all);
        }
        for entry in std::fs::read_dir(dir)? {
            let stored: Stored = serde_json::from_slice(&std::fs::read(entry?.path())?)?;
            let chain = Self::restore(identity, stored, now_ms)?;
            all.insert(chain.name.clone(), chain);
        }
        Ok(all)
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Net {
        people: Vec<Identity>,
        chains: Vec<Chain>,
        log: ObservationLog,
        now: u64,
    }

    impl Net {
        fn new(n: usize) -> Self {
            let people: Vec<Identity> =
                (0..n).map(|_| Identity::in_memory(None).unwrap()).collect();
            let first = Chain::create(&people[0], "amici").unwrap();
            let now = first.ledger().time * 1000 + 1000;
            let mut net = Self {
                people,
                chains: vec![first],
                log: ObservationLog::default(),
                now,
            };
            for i in 1..n {
                let key = net.people[i].user_key.clone();
                let invite = net.chains[0].invite(&key, net.now).unwrap();
                let mut chain = Chain::from_invite(&net.people[i], invite).unwrap();
                chain.apply_synced(net.chains[0].commits_from(0), net.now);
                net.chains.push(chain);
                net.settle();
            }
            net
        }

        fn settle(&mut self) {
            for _ in 0..10 {
                for i in 0..self.chains.len() {
                    self.chains[i].tick(&self.log, self.now);
                    let msgs = self.chains[i].outbox();
                    for j in 0..self.chains.len() {
                        if i != j {
                            for msg in msgs.clone() {
                                if self.chains[j].handle(msg, self.now) {
                                    let from = self.chains[j].height();
                                    let missing = self.chains[i].commits_from(from);
                                    self.chains[j].apply_synced(missing, self.now);
                                }
                            }
                        }
                    }
                }
            }
        }

        fn fp(&self, i: usize) -> String {
            self.people[i].fingerprint.clone()
        }
    }

    #[test]
    fn invite_commits_and_the_invitee_catches_up_and_registers_its_device() {
        let net = Net::new(2);
        let device = net.people[1].peer_id.to_string();
        assert!(net.chains[0].is_member(&net.fp(1)));
        assert!(net.chains[0].ledger().devices.contains_key(&device));
        assert!(net.chains[1].ledger().devices.contains_key(&device));
        assert_eq!(net.chains[0].ledger().head, net.chains[1].ledger().head);
        assert_eq!(net.chains[0].key, net.chains[1].key);
    }

    #[test]
    fn observations_are_reported_near_the_end_of_the_epoch() {
        let mut net = Net::new(2);
        let epoch_start = (epoch_of(net.now) + 1) * EPOCH_SECS * 1000;
        let device = net.people[1].peer_id.to_string();
        for i in 0..10 {
            observe(&net.log, &device, i != 0, epoch_start + 1);
        }
        net.now = epoch_start + 1000;
        net.settle();
        assert_eq!(net.chains[0].ledger().members[&net.fp(1)].sent, 0);
        net.now = epoch_start + EPOCH_SECS * 950;
        net.settle();
        let member = &net.chains[0].ledger().members[&net.fp(1)];
        assert_eq!((member.sent, member.passed), (10, 9));
    }

    #[test]
    fn the_key_rotates_every_epoch_for_every_member() {
        let mut net = Net::new(3);
        let old = net.chains[0].key;
        net.now = (epoch_of(net.now) + 1) * EPOCH_SECS * 1000 + 1000;
        net.settle();
        assert_ne!(net.chains[0].key, old);
        assert!(net.chains.iter().all(|c| c.key == net.chains[0].key));
        let rotated = net.chains[0].key;
        net.now += 3_600_000;
        net.settle();
        assert_eq!(net.chains[0].key, rotated);
    }

    #[test]
    fn an_expelled_member_does_not_get_the_new_key() {
        let mut net = Net::new(3);
        let old = net.chains[0].key;
        let lucia = net.fp(2);
        net.chains[0].expel(&lucia, net.now).unwrap();
        assert_eq!(
            net.chains[0].expel_outcome(&lucia),
            Some(ExpelOutcome::Expelled)
        );
        net.settle();
        assert!(!net.chains[1].is_member(&lucia));
        assert_ne!(net.chains[0].key, old);
        assert_eq!(net.chains[0].key, net.chains[1].key);
        assert_eq!(net.chains[2].key, old);

        let key = Identity::in_memory(None).unwrap().user_key;
        let entry = invite_entry(&net.people[0], &net.chains[0].genesis, &old, &key).unwrap();
        let stale = SignedOp::sign(&net.people[0], Op::AddMember { entry }).unwrap();
        assert!(net.chains[0].engine.submit(stale, net.now).is_err());
    }
}

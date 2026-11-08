use crate::{
    error::{BpError, BpResult},
    identity::Identity,
    ledger::{Block, Ledger, SignedOp, EPOCH_SECS},
};
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const BASE_TIMEOUT_MS: u64 = 3_000;
pub const TIMEOUT_STEP_MS: u64 = 1_000;
pub const REBROADCAST_MS: u64 = 2_000;
pub const MAX_CLOCK_SKEW_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum VoteKind {
    Prevote,
    Precommit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Propose,
    Prevote,
    Precommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vote {
    pub height: u64,
    pub round: u32,
    pub kind: VoteKind,
    pub block: Option<String>,
    pub device: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub round: u32,
    pub block: Block,
    pub valid_round: Option<u32>,
    pub device: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub block: Block,
    pub round: u32,
    pub precommits: Vec<Vote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsensusMsg {
    Op { op: SignedOp },
    Proposal { proposal: Box<Proposal> },
    Vote { vote: Vote },
    Commit { commit: Box<Commit> },
}

fn signing_bytes<T: Serialize>(context: &[u8], value: &T) -> Vec<u8> {
    let mut bytes = context.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(value).unwrap_or_default());
    bytes
}

impl Vote {
    fn message(&self) -> Vec<u8> {
        signing_bytes(
            b"billpouch/vote/v1",
            &(
                self.height,
                self.round,
                self.kind,
                &self.block,
                &self.device,
            ),
        )
    }
}

impl Proposal {
    fn message(&self) -> Vec<u8> {
        signing_bytes(
            b"billpouch/proposal/v1",
            &(self.round, &self.block, self.valid_round, &self.device),
        )
    }
}

pub struct Engine {
    device: Keypair,
    device_id: String,
    pub ledger: Ledger,
    round: u32,
    step: Step,
    locked: Option<(u32, Block)>,
    valid: Option<(u32, Block)>,
    proposals: BTreeMap<u32, Proposal>,
    votes: BTreeMap<(u32, VoteKind), BTreeMap<String, Vote>>,
    mempool: BTreeMap<String, SignedOp>,
    timeout: Option<(u64, u32, Step)>,
    fired: BTreeSet<(u32, &'static str)>,
    proposed_in: Option<u32>,
    last_commit: Option<Commit>,
    committed: Vec<Commit>,
    last_rebroadcast: u64,
    outbox: Vec<ConsensusMsg>,
}

impl Engine {
    pub fn new(identity: &Identity, ledger: Ledger) -> Self {
        Self {
            device: identity.device.clone(),
            device_id: identity.peer_id.to_string(),
            ledger,
            round: 0,
            step: Step::Propose,
            locked: None,
            valid: None,
            proposals: BTreeMap::new(),
            votes: BTreeMap::new(),
            mempool: BTreeMap::new(),
            timeout: None,
            fired: BTreeSet::new(),
            proposed_in: None,
            last_commit: None,
            committed: Vec::new(),
            last_rebroadcast: 0,
            outbox: Vec::new(),
        }
    }

    pub fn drain(&mut self) -> Vec<ConsensusMsg> {
        std::mem::take(&mut self.outbox)
    }

    pub fn take_commits(&mut self) -> Vec<Commit> {
        std::mem::take(&mut self.committed)
    }

    pub fn last_commit(&self) -> Option<&Commit> {
        self.last_commit.as_ref()
    }

    pub fn pending_ops(&self) -> usize {
        self.mempool.len()
    }

    pub fn submit(&mut self, op: SignedOp, now_ms: u64) -> BpResult<()> {
        self.ledger.validate_op(&op, &self.probe_block(now_ms))?;
        if self.mempool.insert(op.id(), op.clone()).is_none() {
            self.outbox.push(ConsensusMsg::Op { op });
        }
        self.progress(now_ms);
        Ok(())
    }

    pub fn handle(&mut self, msg: ConsensusMsg, now_ms: u64) {
        match msg {
            ConsensusMsg::Op { op } => {
                if !self.mempool.contains_key(&op.id())
                    && self
                        .ledger
                        .validate_op(&op, &self.probe_block(now_ms))
                        .is_ok()
                {
                    self.mempool.insert(op.id(), op);
                }
            }
            ConsensusMsg::Proposal { proposal } => {
                if self.verify_proposal(&proposal).is_ok() {
                    self.proposals.entry(proposal.round).or_insert(*proposal);
                }
            }
            ConsensusMsg::Vote { vote } => {
                if let Ok(user) = self.verify_vote(&vote) {
                    self.votes
                        .entry((vote.round, vote.kind))
                        .or_default()
                        .entry(user)
                        .or_insert(vote);
                }
            }
            ConsensusMsg::Commit { commit } => {
                if commit.block.height == self.ledger.height + 1 && self.verify_commit(&commit) {
                    self.finalize(*commit, now_ms);
                }
            }
        }
        self.progress(now_ms);
    }

    pub fn tick(&mut self, now_ms: u64) {
        if let Some((deadline, round, step)) = self.timeout {
            if now_ms >= deadline && round == self.round {
                self.timeout = None;
                match step {
                    Step::Propose if self.step == Step::Propose => {
                        self.cast(VoteKind::Prevote, None);
                        self.step = Step::Prevote;
                    }
                    Step::Prevote if self.step == Step::Prevote => {
                        self.cast(VoteKind::Precommit, None);
                        self.step = Step::Precommit;
                    }
                    Step::Precommit => self.start_round(round + 1),
                    _ => {}
                }
            }
        }
        if now_ms >= self.last_rebroadcast + REBROADCAST_MS {
            self.last_rebroadcast = now_ms;
            self.rebroadcast();
        }
        self.progress(now_ms);
    }

    fn active(&self) -> bool {
        !self.mempool.is_empty()
            || !self.proposals.is_empty()
            || self.votes.values().any(|v| !v.is_empty())
    }

    fn my_user(&self) -> Option<String> {
        let user = self.ledger.device_owner(&self.device_id)?;
        (self.ledger.voting_device(user) == Some(self.device_id.as_str())
            && self.ledger.snapshot.weight(user) > 0)
            .then(|| user.to_string())
    }

    fn proposer(&self, round: u32) -> Option<String> {
        let validators: Vec<&String> = self
            .ledger
            .snapshot
            .users
            .iter()
            .filter(|(user, s)| s.weight > 0 && self.ledger.voting_device(user).is_some())
            .map(|(user, _)| user)
            .collect();
        if validators.is_empty() {
            return None;
        }
        let index = (self.ledger.height + 1 + u64::from(round)) % validators.len() as u64;
        Some(validators[index as usize].clone())
    }

    fn signer_user(&self, device: &str) -> BpResult<String> {
        let user = self
            .ledger
            .device_owner(device)
            .ok_or_else(|| invalid("unknown device"))?;
        if self.ledger.voting_device(user) != Some(device) {
            return Err(invalid("not the user's voting device"));
        }
        Ok(user.to_string())
    }

    fn verify_signature(&self, device: &str, message: &[u8], signature: &str) -> BpResult<()> {
        let key = self
            .ledger
            .device_key(device)
            .ok_or_else(|| invalid("unknown device key"))?;
        let signature = hex::decode(signature).map_err(|_| invalid("bad signature"))?;
        key.verify(message, &signature)
            .then_some(())
            .ok_or_else(|| invalid("invalid signature"))
    }

    fn verify_vote(&self, vote: &Vote) -> BpResult<String> {
        if vote.height != self.ledger.height + 1 {
            return Err(invalid("vote for another height"));
        }
        let user = self.signer_user(&vote.device)?;
        self.verify_signature(&vote.device, &vote.message(), &vote.signature)?;
        Ok(user)
    }

    fn verify_proposal(&self, proposal: &Proposal) -> BpResult<()> {
        if proposal.block.height != self.ledger.height + 1 {
            return Err(invalid("proposal for another height"));
        }
        let user = self.signer_user(&proposal.device)?;
        if Some(user) != self.proposer(proposal.round) {
            return Err(invalid("not this round's proposer"));
        }
        self.verify_signature(&proposal.device, &proposal.message(), &proposal.signature)
    }

    fn verify_commit(&self, commit: &Commit) -> bool {
        let hash = commit.block.hash();
        let voters: BTreeSet<String> = commit
            .precommits
            .iter()
            .filter(|v| {
                v.kind == VoteKind::Precommit
                    && v.round == commit.round
                    && v.block.as_deref() == Some(hash.as_str())
            })
            .filter_map(|v| self.verify_vote(v).ok())
            .collect();
        self.ledger.snapshot.is_quorum(&voters) && self.ledger.clone().apply(&commit.block).is_ok()
    }

    fn block_is_valid(&self, block: &Block, now_ms: u64) -> bool {
        block.time <= now_ms / 1000 + MAX_CLOCK_SKEW_SECS
            && self.ledger.clone().apply(block).is_ok()
    }

    fn probe_block(&self, now_ms: u64) -> Block {
        Block {
            height: self.ledger.height + 1,
            prev: self.ledger.head.clone(),
            time: (now_ms / 1000).max(self.ledger.time),
            proposer: self.ledger.creator(),
            ops: vec![],
        }
    }

    fn weight_of<'a>(&self, users: impl IntoIterator<Item = &'a String>) -> u64 {
        users
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|u| self.ledger.snapshot.weight(u))
            .sum()
    }

    fn quorum(&self, round: u32, kind: VoteKind, block: Option<Option<&str>>) -> bool {
        let Some(votes) = self.votes.get(&(round, kind)) else {
            return false;
        };
        let users = votes
            .iter()
            .filter(|(_, v)| block.is_none_or(|b| v.block.as_deref() == b))
            .map(|(u, _)| u);
        self.weight_of(users) * 3 > self.ledger.snapshot.total_weight() * 2
    }

    fn cast(&mut self, kind: VoteKind, block: Option<String>) {
        let Some(user) = self.my_user() else {
            return;
        };
        let mut vote = Vote {
            height: self.ledger.height + 1,
            round: self.round,
            kind,
            block,
            device: self.device_id.clone(),
            signature: String::new(),
        };
        let Ok(signature) = self.device.sign(&vote.message()) else {
            return;
        };
        vote.signature = hex::encode(signature);
        self.votes
            .entry((vote.round, kind))
            .or_default()
            .entry(user)
            .or_insert(vote.clone());
        self.outbox.push(ConsensusMsg::Vote { vote });
    }

    fn start_round(&mut self, round: u32) {
        self.round = round;
        self.step = Step::Propose;
        self.timeout = None;
    }

    fn schedule(&mut self, step: Step, now_ms: u64) {
        let pending = self
            .timeout
            .is_some_and(|(_, r, s)| r == self.round && s == step);
        if !pending {
            let delay = BASE_TIMEOUT_MS + TIMEOUT_STEP_MS * u64::from(self.round);
            self.timeout = Some((now_ms + delay, self.round, step));
        }
    }

    fn maybe_propose(&mut self, now_ms: u64) {
        let Some(user) = self.my_user() else {
            return;
        };
        if self.step != Step::Propose
            || self.proposed_in == Some(self.round)
            || self.proposer(self.round).as_ref() != Some(&user)
        {
            return;
        }
        let (block, valid_round) = match &self.valid {
            Some((round, block)) => (block.clone(), Some(*round)),
            None => {
                if self.mempool.is_empty() {
                    return;
                }
                let mut block = Block {
                    height: self.ledger.height + 1,
                    prev: self.ledger.head.clone(),
                    time: (now_ms / 1000).max(self.ledger.time),
                    proposer: user,
                    ops: vec![],
                };
                for op in self.mempool.values() {
                    block.ops.push(op.clone());
                    if self.ledger.clone().apply(&block).is_err() {
                        block.ops.pop();
                    }
                }
                if block.ops.is_empty() {
                    return;
                }
                (block, None)
            }
        };
        let mut proposal = Proposal {
            round: self.round,
            block,
            valid_round,
            device: self.device_id.clone(),
            signature: String::new(),
        };
        let Ok(signature) = self.device.sign(&proposal.message()) else {
            return;
        };
        proposal.signature = hex::encode(signature);
        self.proposed_in = Some(self.round);
        self.proposals.entry(self.round).or_insert(proposal.clone());
        self.outbox.push(ConsensusMsg::Proposal {
            proposal: Box::new(proposal),
        });
    }

    fn progress(&mut self, now_ms: u64) {
        for _ in 0..16 {
            let before = (self.ledger.height, self.round, self.step);
            self.step_rules(now_ms);
            if (self.ledger.height, self.round, self.step) == before {
                break;
            }
        }
        if self.active() && self.timeout.is_none() {
            self.schedule(self.step, now_ms);
        }
    }

    fn step_rules(&mut self, now_ms: u64) {
        let rounds: Vec<u32> = self
            .proposals
            .keys()
            .chain(self.votes.keys().map(|(r, _)| r))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for round in &rounds {
            let Some(proposal) = self.proposals.get(round).cloned() else {
                continue;
            };
            let hash = proposal.block.hash();
            if self.quorum(*round, VoteKind::Precommit, Some(Some(&hash)))
                && self.block_is_valid(&proposal.block, now_ms)
            {
                let precommits = self.votes[&(*round, VoteKind::Precommit)]
                    .values()
                    .filter(|v| v.block.as_deref() == Some(hash.as_str()))
                    .cloned()
                    .collect();
                self.finalize(
                    Commit {
                        block: proposal.block,
                        round: *round,
                        precommits,
                    },
                    now_ms,
                );
                return;
            }
        }
        if let Some(&skip) = rounds.iter().find(|&&r| r > self.round) {
            let users: BTreeSet<&String> = self
                .votes
                .iter()
                .filter(|((r, _), _)| *r == skip)
                .flat_map(|(_, v)| v.keys())
                .collect();
            if self.weight_of(users) * 3 > self.ledger.snapshot.total_weight() {
                self.start_round(skip);
            }
        }

        self.maybe_propose(now_ms);
        let round = self.round;
        let proposal = self.proposals.get(&round).cloned();

        if self.step == Step::Propose {
            if let Some(p) = &proposal {
                let hash = p.block.hash();
                let acceptable = match p.valid_round {
                    None => Some(self.locked.as_ref().is_none_or(|(_, b)| b.hash() == hash)),
                    Some(vr)
                        if vr < round && self.quorum(vr, VoteKind::Prevote, Some(Some(&hash))) =>
                    {
                        Some(
                            self.locked
                                .as_ref()
                                .is_none_or(|(lr, b)| *lr <= vr || b.hash() == hash),
                        )
                    }
                    Some(_) => None,
                };
                if let Some(acceptable) = acceptable {
                    let vote =
                        (acceptable && self.block_is_valid(&p.block, now_ms)).then_some(hash);
                    self.cast(VoteKind::Prevote, vote);
                    self.step = Step::Prevote;
                    self.timeout = None;
                }
            }
        }

        if self.step >= Step::Prevote {
            if let Some(p) = &proposal {
                let hash = p.block.hash();
                if self.quorum(round, VoteKind::Prevote, Some(Some(&hash)))
                    && self.block_is_valid(&p.block, now_ms)
                    && self.fired.insert((round, "lock"))
                {
                    if self.step == Step::Prevote {
                        self.locked = Some((round, p.block.clone()));
                        self.cast(VoteKind::Precommit, Some(hash));
                        self.step = Step::Precommit;
                        self.timeout = None;
                    }
                    self.valid = Some((round, p.block.clone()));
                }
            }
        }

        if self.step == Step::Prevote {
            if self.quorum(round, VoteKind::Prevote, Some(None)) {
                self.cast(VoteKind::Precommit, None);
                self.step = Step::Precommit;
                self.timeout = None;
            } else if self.quorum(round, VoteKind::Prevote, None)
                && self.fired.insert((round, "prevote-timeout"))
            {
                self.timeout = None;
                self.schedule(Step::Prevote, now_ms);
            }
        }

        if self.quorum(round, VoteKind::Precommit, None)
            && self.fired.insert((round, "precommit-timeout"))
        {
            self.timeout = None;
            self.schedule(Step::Precommit, now_ms);
        }
    }

    fn finalize(&mut self, commit: Commit, now_ms: u64) {
        if self.ledger.apply(&commit.block).is_err() {
            return;
        }
        let probe = self.probe_block(now_ms);
        let ledger = &self.ledger;
        self.mempool
            .retain(|_, op| ledger.validate_op(op, &probe).is_ok());
        self.outbox.push(ConsensusMsg::Commit {
            commit: Box::new(commit.clone()),
        });
        self.committed.push(commit.clone());
        self.last_commit = Some(commit);
        self.round = 0;
        self.step = Step::Propose;
        self.locked = None;
        self.valid = None;
        self.proposals.clear();
        self.votes.clear();
        self.timeout = None;
        self.fired.clear();
        self.proposed_in = None;
    }

    fn rebroadcast(&mut self) {
        let mut again: Vec<ConsensusMsg> = self
            .mempool
            .values()
            .map(|op| ConsensusMsg::Op { op: op.clone() })
            .collect();
        if let Some(user) = self.my_user() {
            if let Some(p) = self.proposals.get(&self.round) {
                if p.device == self.device_id {
                    again.push(ConsensusMsg::Proposal {
                        proposal: Box::new(p.clone()),
                    });
                }
            }
            for ((round, _), votes) in &self.votes {
                if *round == self.round {
                    if let Some(v) = votes.get(&user) {
                        again.push(ConsensusMsg::Vote { vote: v.clone() });
                    }
                }
            }
        }
        if let Some(c) = &self.last_commit {
            again.push(ConsensusMsg::Commit {
                commit: Box::new(c.clone()),
            });
        }
        self.outbox.extend(again);
    }
}

pub fn epoch_of(now_ms: u64) -> u64 {
    now_ms / 1000 / EPOCH_SECS
}

fn invalid(reason: &str) -> BpError {
    BpError::InvalidInput(format!("consensus: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ledger::{Op, SignedOp},
        membership::{genesis, invite_entry},
    };

    type Tamper = Box<dyn FnMut(usize, usize, ConsensusMsg) -> Option<ConsensusMsg>>;

    struct Sim {
        people: Vec<Identity>,
        engines: Vec<Engine>,
        online: Vec<bool>,
        now: u64,
        tamper: Option<Tamper>,
        commits: Vec<BTreeMap<u64, String>>,
    }

    impl Sim {
        fn new(n: usize) -> Self {
            let people: Vec<Identity> =
                (0..n).map(|_| Identity::in_memory(None).unwrap()).collect();
            let (root, key) = genesis(&people[0], "amici").unwrap();
            let mut ledger = Ledger::new(root.clone(), &people[0].cert).unwrap();
            let start = ledger.time;
            let apply = |ledger: &mut Ledger, time: u64, ops: Vec<SignedOp>| {
                let block = Block {
                    height: ledger.height + 1,
                    prev: ledger.head.clone(),
                    time,
                    proposer: people[0].fingerprint.clone(),
                    ops,
                };
                ledger.apply(&block).unwrap();
            };
            for p in &people[1..] {
                let entry = invite_entry(&people[0], &root, &key, &p.user_key).unwrap();
                apply(
                    &mut ledger,
                    start,
                    vec![SignedOp::sign(&people[0], Op::AddMember { entry }).unwrap()],
                );
                apply(
                    &mut ledger,
                    start,
                    vec![SignedOp::sign(
                        p,
                        Op::RegisterDevice {
                            cert: p.cert.clone(),
                        },
                    )
                    .unwrap()],
                );
            }
            apply(&mut ledger, start + EPOCH_SECS, vec![]);
            let now = (start + EPOCH_SECS) * 1000;
            let engines = people
                .iter()
                .map(|p| Engine::new(p, ledger.clone()))
                .collect();
            Self {
                online: vec![true; n],
                engines,
                people,
                now,
                tamper: None,
                commits: vec![BTreeMap::new(); n],
            }
        }

        fn run(&mut self, ms: u64) {
            let end = self.now + ms;
            while self.now < end {
                self.now += 100;
                for i in 0..self.engines.len() {
                    if self.online[i] {
                        self.engines[i].tick(self.now);
                    }
                }
                for _ in 0..8 {
                    let mut delivered = false;
                    for from in 0..self.engines.len() {
                        if !self.online[from] {
                            self.engines[from].drain();
                            continue;
                        }
                        for msg in self.engines[from].drain() {
                            for to in 0..self.engines.len() {
                                if to == from || !self.online[to] {
                                    continue;
                                }
                                let msg = match self.tamper.as_mut() {
                                    Some(t) => t(from, to, msg.clone()),
                                    None => Some(msg.clone()),
                                };
                                if let Some(msg) = msg {
                                    self.engines[to].handle(msg, self.now);
                                    delivered = true;
                                }
                            }
                        }
                    }
                    if !delivered {
                        break;
                    }
                }
                for i in 0..self.engines.len() {
                    let l = &self.engines[i].ledger;
                    self.commits[i].insert(l.height, l.head.clone());
                }
            }
        }

        fn submit_observation(&mut self, by: usize, about: usize) {
            let op = SignedOp::sign(
                &self.people[by],
                Op::Observations {
                    held: Default::default(),
                    epoch: epoch_of(self.now),
                    results: vec![crate::ledger::Observation {
                        device: self.people[about].peer_id.to_string(),
                        user: self.people[about].fingerprint.clone(),
                        sent: 1,
                        passed: 1,
                        bytes: 0,
                    }],
                },
            )
            .unwrap();
            self.engines[by].submit(op, self.now).unwrap();
        }

        fn heights(&self) -> Vec<u64> {
            self.engines.iter().map(|e| e.ledger.height).collect()
        }

        fn assert_agreement(&self) {
            let mut decided: BTreeMap<u64, &String> = BTreeMap::new();
            for history in &self.commits {
                for (height, head) in history {
                    if let Some(other) = decided.insert(*height, head) {
                        assert_eq!(other, head, "two different blocks at height {height}");
                    }
                }
            }
        }
    }

    #[test]
    fn all_validators_online_commit_the_same_block() {
        let mut sim = Sim::new(4);
        let start = sim.heights()[0];
        sim.submit_observation(1, 2);
        sim.run(20_000);
        assert!(
            sim.heights().iter().all(|h| *h == start + 1),
            "{:?}",
            sim.heights()
        );
        assert!(sim.engines.iter().all(|e| e.pending_ops() == 0));
        sim.assert_agreement();
    }

    #[test]
    fn an_idle_network_produces_no_blocks() {
        let mut sim = Sim::new(4);
        let start = sim.heights();
        sim.run(30_000);
        assert_eq!(sim.heights(), start);
    }

    #[test]
    fn one_of_four_offline_still_commits() {
        let mut sim = Sim::new(4);
        let start = sim.heights()[0];
        for offline in 0..4 {
            sim.online = vec![true; 4];
            sim.online[offline] = false;
            let by = (offline + 1) % 4;
            let about = (offline + 2) % 4;
            sim.submit_observation(by, about);
            sim.run(60_000);
        }
        assert!(
            sim.heights().iter().all(|h| *h >= start + 3),
            "{:?}",
            sim.heights()
        );
        sim.assert_agreement();
    }

    #[test]
    fn two_of_four_offline_stall_then_recover() {
        let mut sim = Sim::new(4);
        let start = sim.heights()[0];
        sim.online = vec![true, true, false, false];
        sim.submit_observation(0, 1);
        sim.run(60_000);
        assert_eq!(sim.heights()[..2], [start, start]);
        sim.online = vec![true; 4];
        sim.run(60_000);
        assert!(
            sim.heights().iter().all(|h| *h == start + 1),
            "{:?}",
            sim.heights()
        );
        sim.assert_agreement();
    }

    #[test]
    fn an_equivocating_proposer_cannot_split_the_network() {
        let mut sim = Sim::new(4);
        let liar = sim.people[0].device.clone();
        let liar_id = sim.people[0].peer_id.to_string();
        sim.tamper = Some(Box::new(move |from, to, msg| match msg {
            ConsensusMsg::Proposal { mut proposal } if from == 0 && to % 2 == 0 => {
                proposal.block.time += 1;
                proposal.device = liar_id.clone();
                proposal.signature = hex::encode(liar.sign(&proposal.message()).unwrap());
                Some(ConsensusMsg::Proposal { proposal })
            }
            other => Some(other),
        }));
        let start = sim.heights()[0];
        for round in 0..4 {
            sim.submit_observation(round % 4, (round + 1) % 4);
            sim.run(40_000);
        }
        sim.assert_agreement();
        assert!(
            sim.heights().iter().all(|h| *h >= start + 3),
            "{:?}",
            sim.heights()
        );
    }

    #[test]
    fn forged_votes_and_proposals_are_ignored() {
        let mut sim = Sim::new(4);
        let outsider = Identity::in_memory(None).unwrap();
        let start = sim.heights()[0];
        sim.submit_observation(1, 2);
        let mut vote = Vote {
            height: start + 1,
            round: 0,
            kind: VoteKind::Precommit,
            block: Some("fake".into()),
            device: outsider.peer_id.to_string(),
            signature: String::new(),
        };
        vote.signature = hex::encode(outsider.device.sign(&vote.message()).unwrap());
        for e in &mut sim.engines {
            assert!(e.verify_vote(&vote).is_err());
        }
        let mut stolen = vote.clone();
        stolen.device = sim.people[1].peer_id.to_string();
        assert!(sim.engines[0].verify_vote(&stolen).is_err());
        sim.run(20_000);
        assert!(sim.heights().iter().all(|h| *h == start + 1));
    }

    #[test]
    fn a_lagging_node_catches_up_from_a_commit() {
        let mut sim = Sim::new(4);
        let start = sim.heights()[0];
        sim.online[3] = false;
        sim.submit_observation(0, 1);
        sim.run(30_000);
        assert_eq!(sim.heights()[3], start);
        sim.online[3] = true;
        sim.run(10_000);
        assert_eq!(sim.heights()[3], start + 1);
        sim.assert_agreement();
    }

    #[test]
    fn a_block_committed_by_one_node_is_never_replaced_by_another() {
        let mut sim = Sim::new(4);
        let start = sim.heights()[0];
        let index_of = |sim: &Sim, round: u32| {
            let user = sim.engines[0].proposer(round).unwrap();
            sim.people
                .iter()
                .position(|p| p.fingerprint == user)
                .unwrap()
        };
        let next_proposer = index_of(&sim, 1);
        let lucky = (0..4).find(|i| *i != next_proposer).unwrap();
        sim.tamper = Some(Box::new(move |_, to, msg| match &msg {
            ConsensusMsg::Commit { .. } => None,
            ConsensusMsg::Vote { vote } if vote.round == 0 => match vote.kind {
                VoteKind::Prevote if to == next_proposer => None,
                VoteKind::Precommit if to != lucky => None,
                _ => Some(msg),
            },
            _ => Some(msg),
        }));
        sim.submit_observation(lucky, next_proposer);
        sim.run(60_000);
        assert_eq!(
            sim.heights()[lucky],
            start + 1,
            "the lucky node commits in round 0"
        );
        sim.assert_agreement();

        sim.tamper = None;
        sim.run(30_000);
        assert!(
            sim.heights().iter().all(|h| *h == start + 1),
            "{:?}",
            sim.heights()
        );
        sim.assert_agreement();
    }
}

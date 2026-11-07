use crate::{
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
    membership::Entry,
    network::ReputationTier,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EPOCH_SECS: u64 = 86_400;
pub const MAX_CHALLENGES_PER_DEVICE_PER_EPOCH: u32 = 288;
pub const MIN_CHALLENGES_FOR_PROMOTION: u64 = 100;
const DAY: u64 = 86_400;

const PROMOTIONS: [(ReputationTier, u64, f64); 3] = [
    (ReputationTier::R4, 365, 0.999),
    (ReputationTier::R3, 90, 0.995),
    (ReputationTier::R2, 30, 0.98),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub device: String,
    pub user: String,
    pub sent: u32,
    pub passed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Op {
    AddMember {
        entry: Entry,
    },
    Observations {
        epoch: u64,
        results: Vec<Observation>,
    },
    ProposeExpel {
        member: String,
    },
    ApproveExpel {
        member: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedOp {
    pub op: Op,
    pub author: String,
    pub nonce: u64,
    pub signature: String,
}

impl SignedOp {
    pub fn sign(identity: &Identity, op: Op) -> BpResult<Self> {
        let nonce = rand::random();
        let message = op_message(&op, &identity.user_key, nonce)?;
        let signature = identity
            .user
            .sign(&message)
            .map_err(|e| BpError::Identity(format!("cannot sign op: {e}")))?;
        Ok(Self {
            op,
            author: identity.user_key.clone(),
            nonce,
            signature: hex::encode(signature),
        })
    }

    pub fn author_fingerprint(&self) -> BpResult<String> {
        let (public, fingerprint) = parse_user_key(&self.author)?;
        let signature = hex::decode(&self.signature)
            .map_err(|_| BpError::Identity("op: bad signature encoding".into()))?;
        if public.verify(&op_message(&self.op, &self.author, self.nonce)?, &signature) {
            Ok(fingerprint)
        } else {
            Err(BpError::Identity("op: invalid signature".into()))
        }
    }

    pub fn id(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap_or_default())
            .to_hex()
            .to_string()
    }
}

fn op_message(op: &Op, author: &str, nonce: u64) -> BpResult<Vec<u8>> {
    let mut msg = b"billpouch/op/v1".to_vec();
    msg.extend_from_slice(&serde_json::to_vec(&(op, author, nonce))?);
    Ok(msg)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub height: u64,
    pub prev: String,
    pub time: u64,
    pub proposer: String,
    pub ops: Vec<SignedOp>,
}

impl Block {
    pub fn hash(&self) -> String {
        blake3::hash(&serde_json::to_vec(self).unwrap_or_default())
            .to_hex()
            .to_string()
    }

    pub fn epoch(&self) -> u64 {
        self.time / EPOCH_SECS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    pub tier: ReputationTier,
    pub weight: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub epoch: u64,
    pub users: BTreeMap<String, Standing>,
}

impl Snapshot {
    pub fn total_weight(&self) -> u64 {
        self.users.values().map(|s| s.weight).sum()
    }

    pub fn weight(&self, user: &str) -> u64 {
        self.users.get(user).map_or(0, |s| s.weight)
    }

    pub fn tier(&self, user: &str) -> Option<ReputationTier> {
        self.users.get(user).map(|s| s.tier)
    }

    pub fn is_quorum<'a>(&self, voters: impl IntoIterator<Item = &'a String>) -> bool {
        let voters: BTreeSet<&String> = voters.into_iter().collect();
        let weight: u64 = voters.iter().map(|v| self.weight(v)).sum();
        weight * 3 > self.total_weight() * 2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Invite,
    ProposeExpel,
    Vote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub key: String,
    pub joined_at: u64,
    pub sent: u64,
    pub passed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub genesis: Entry,
    pub height: u64,
    pub head: String,
    pub time: u64,
    pub members: BTreeMap<String, Member>,
    pub entries: Vec<Entry>,
    pub expelled: BTreeSet<String>,
    pub expel_approvals: BTreeMap<String, BTreeSet<String>>,
    pub reported: BTreeSet<(String, u64)>,
    pub applied_ops: BTreeSet<String>,
    pub snapshot: Snapshot,
}

impl Ledger {
    pub fn new(genesis: Entry) -> BpResult<Self> {
        let (_, creator) = parse_user_key(&genesis.body.author)?;
        let mut members = BTreeMap::new();
        members.insert(
            creator,
            Member {
                key: genesis.body.author.clone(),
                joined_at: genesis.body.at,
                sent: 0,
                passed: 0,
            },
        );
        let mut ledger = Self {
            head: genesis.id(),
            time: genesis.body.at,
            height: 0,
            members,
            entries: Vec::new(),
            expelled: BTreeSet::new(),
            expel_approvals: BTreeMap::new(),
            reported: BTreeSet::new(),
            applied_ops: BTreeSet::new(),
            snapshot: Snapshot {
                epoch: genesis.body.at / EPOCH_SECS,
                users: BTreeMap::new(),
            },
            genesis,
        };
        ledger.snapshot = ledger.compute_snapshot(ledger.snapshot.epoch);
        Ok(ledger)
    }

    pub fn creator(&self) -> String {
        parse_user_key(&self.genesis.body.author)
            .map(|(_, fp)| fp)
            .unwrap_or_default()
    }

    pub fn is_member(&self, user: &str) -> bool {
        self.members.contains_key(user) && !self.expelled.contains(user)
    }

    pub fn has_capability(&self, user: &str, capability: Capability) -> bool {
        if !self.is_member(user) {
            return false;
        }
        let tier = self.snapshot.tier(user).unwrap_or(ReputationTier::R1);
        let nobody_at =
            |level: ReputationTier| self.snapshot.users.values().all(|s| s.tier < level);
        let creator = user == self.creator();
        match capability {
            Capability::Vote => tier >= ReputationTier::R1,
            Capability::Invite => {
                tier >= ReputationTier::R2 || (creator && nobody_at(ReputationTier::R2))
            }
            Capability::ProposeExpel => {
                tier >= ReputationTier::R3 || (creator && nobody_at(ReputationTier::R3))
            }
        }
    }

    pub fn apply(&mut self, block: &Block) -> BpResult<()> {
        if block.height != self.height + 1 || block.prev != self.head {
            return Err(invalid("block does not extend the current head"));
        }
        if block.time < self.time {
            return Err(invalid("block time goes backwards"));
        }
        if !self.is_member(&block.proposer) {
            return Err(invalid("proposer is not a member"));
        }
        let mut next = self.clone();
        if block.epoch() > next.snapshot.epoch {
            next.snapshot = next.compute_snapshot(block.epoch());
        }
        for op in &block.ops {
            next.apply_op(op, block)?;
        }
        next.height = block.height;
        next.head = block.hash();
        next.time = block.time;
        *self = next;
        Ok(())
    }

    pub fn validate_op(&self, op: &SignedOp, block: &Block) -> BpResult<()> {
        self.clone().apply_op(op, block)
    }

    fn apply_op(&mut self, signed: &SignedOp, block: &Block) -> BpResult<()> {
        let id = signed.id();
        if self.applied_ops.contains(&id) {
            return Err(invalid("op already applied"));
        }
        let author = signed.author_fingerprint()?;
        if !self.is_member(&author) {
            return Err(invalid("op author is not a member"));
        }
        match &signed.op {
            Op::AddMember { entry } => {
                if !self.has_capability(&author, Capability::Invite) {
                    return Err(invalid("author cannot invite"));
                }
                if entry.body.author != signed.author
                    || entry.body.genesis.as_deref() != Some(&self.genesis.id())
                    || entry.signed_by_author().is_none()
                {
                    return Err(invalid("malformed member entry"));
                }
                let (_, member) = parse_user_key(&entry.body.member)?;
                if self.members.contains_key(&member) {
                    return Err(invalid("already a member"));
                }
                self.members.insert(
                    member,
                    Member {
                        key: entry.body.member.clone(),
                        joined_at: block.time,
                        sent: 0,
                        passed: 0,
                    },
                );
                self.entries.push(entry.clone());
            }
            Op::Observations { epoch, results } => {
                if *epoch != block.epoch() || !self.reported.insert((author.clone(), *epoch)) {
                    return Err(invalid(
                        "observations for a wrong or already reported epoch",
                    ));
                }
                let mut devices = BTreeSet::new();
                for o in results {
                    if o.user == author
                        || !self.is_member(&o.user)
                        || o.passed > o.sent
                        || o.sent > MAX_CHALLENGES_PER_DEVICE_PER_EPOCH
                        || !devices.insert(&o.device)
                    {
                        return Err(invalid("invalid observation"));
                    }
                }
                for o in results {
                    let member = self.members.get_mut(&o.user).expect("checked member");
                    member.sent += u64::from(o.sent);
                    member.passed += u64::from(o.passed);
                }
            }
            Op::ProposeExpel { member } => {
                if !self.has_capability(&author, Capability::ProposeExpel)
                    || !self.is_member(member)
                    || member == &author
                    || self.expel_approvals.contains_key(member)
                {
                    return Err(invalid("cannot propose this expulsion"));
                }
                self.expel_approvals
                    .insert(member.clone(), BTreeSet::from([author]));
                self.maybe_expel(member);
            }
            Op::ApproveExpel { member } => {
                if !self.has_capability(&author, Capability::Vote)
                    || member == &author
                    || !self
                        .expel_approvals
                        .get_mut(member)
                        .is_some_and(|a| a.insert(author))
                {
                    return Err(invalid("cannot approve this expulsion"));
                }
                self.maybe_expel(member);
            }
        }
        self.applied_ops.insert(id);
        Ok(())
    }

    fn maybe_expel(&mut self, member: &str) {
        let Some(approvals) = self.expel_approvals.get(member) else {
            return;
        };
        let others: Vec<&String> = self
            .members
            .keys()
            .filter(|m| self.is_member(m) && m.as_str() != member)
            .collect();
        let others_weight: u64 = others.iter().map(|m| self.snapshot.weight(m)).sum();
        let approved_weight: u64 = approvals.iter().map(|m| self.snapshot.weight(m)).sum();
        if approved_weight * 3 > others_weight * 2 && approvals.len() * 2 >= others.len() {
            self.expelled.insert(member.to_string());
            self.expel_approvals.remove(member);
        }
    }

    pub fn compute_snapshot(&self, epoch: u64) -> Snapshot {
        let epoch_start = epoch * EPOCH_SECS;
        let mut users: BTreeMap<String, Standing> = self
            .members
            .iter()
            .filter(|(fp, _)| !self.expelled.contains(*fp))
            .map(|(fp, m)| {
                let tier = tier_for(m, epoch_start);
                (
                    fp.clone(),
                    Standing {
                        tier,
                        weight: tier as u64,
                    },
                )
            })
            .collect();
        if users.len() >= 4 {
            loop {
                let limit = (users.values().map(|s| s.weight).sum::<u64>() / 4).max(1);
                let mut changed = false;
                for s in users.values_mut() {
                    if s.weight > limit {
                        s.weight = limit;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        Snapshot { epoch, users }
    }
}

fn tier_for(member: &Member, at: u64) -> ReputationTier {
    let age_days = at.saturating_sub(member.joined_at) / DAY;
    if member.sent < MIN_CHALLENGES_FOR_PROMOTION {
        return ReputationTier::R1;
    }
    let pass_rate = member.passed as f64 / member.sent as f64;
    PROMOTIONS
        .iter()
        .find(|(_, days, rate)| age_days >= *days && pass_rate >= *rate)
        .map_or(ReputationTier::R1, |(tier, _, _)| *tier)
}

fn invalid(reason: &str) -> BpError {
    BpError::InvalidInput(format!("ledger: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::membership::Registry;

    struct Net {
        people: Vec<Identity>,
        ledger: Ledger,
        registry: Registry,
    }

    impl Net {
        fn new(n: usize) -> Self {
            let people: Vec<Identity> =
                (0..n).map(|_| Identity::in_memory(None).unwrap()).collect();
            let (registry, _) = Registry::create(&people[0], "amici").unwrap();
            let ledger = Ledger::new(registry.genesis().unwrap().clone()).unwrap();
            Self {
                people,
                ledger,
                registry,
            }
        }

        fn block(&self, proposer: usize, time: u64, ops: Vec<SignedOp>) -> Block {
            Block {
                height: self.ledger.height + 1,
                prev: self.ledger.head.clone(),
                time,
                proposer: self.people[proposer].fingerprint.clone(),
                ops,
            }
        }

        fn commit(&mut self, proposer: usize, time: u64, ops: Vec<SignedOp>) -> BpResult<()> {
            let block = self.block(proposer, time, ops);
            self.ledger.apply(&block)?;
            for entry in self.ledger.entries.clone() {
                self.registry.add_entry(entry);
            }
            Ok(())
        }

        fn next_epoch(&mut self) {
            let t = self.now() + EPOCH_SECS;
            self.commit(0, t, vec![]).unwrap();
        }

        fn invite_op(&mut self, by: usize, who: usize) -> SignedOp {
            let member_key = self.people[who].user_key.clone();
            let entry = self
                .registry
                .entry_for(&self.people[by], &member_key)
                .unwrap();
            SignedOp::sign(&self.people[by], Op::AddMember { entry }).unwrap()
        }

        fn now(&self) -> u64 {
            self.ledger.time
        }

        fn fp(&self, i: usize) -> String {
            self.people[i].fingerprint.clone()
        }

        fn everyone_joined(n: usize) -> Self {
            let mut net = Self::new(n);
            for who in 1..n {
                let op = net.invite_op(0, who);
                let t = net.now();
                net.commit(0, t, vec![op]).unwrap();
            }
            net.next_epoch();
            net
        }

        fn observe(
            &mut self,
            by: usize,
            about: usize,
            sent: u32,
            passed: u32,
            time: u64,
        ) -> SignedOp {
            SignedOp::sign(
                &self.people[by],
                Op::Observations {
                    epoch: time / EPOCH_SECS,
                    results: vec![Observation {
                        device: self.people[about].peer_id.to_string(),
                        user: self.fp(about),
                        sent,
                        passed,
                    }],
                },
            )
            .unwrap()
        }
    }

    #[test]
    fn a_new_network_has_only_its_creator() {
        let net = Net::new(1);
        assert!(net.ledger.is_member(&net.fp(0)));
        assert_eq!(net.ledger.snapshot.total_weight(), 1);
        assert!(net.ledger.snapshot.is_quorum([&net.fp(0)]));
        assert!(net.ledger.has_capability(&net.fp(0), Capability::Invite));
    }

    #[test]
    fn only_capable_members_can_add_members() {
        let mut net = Net::new(3);
        let op = net.invite_op(0, 1);
        let t = net.now();
        net.commit(0, t, vec![op]).unwrap();
        assert!(net.ledger.is_member(&net.fp(1)));

        let op = net.invite_op(1, 2);
        assert!(net.commit(1, t, vec![op]).is_err());
        assert!(!net.ledger.is_member(&net.fp(2)));
    }

    #[test]
    fn blocks_must_extend_the_head_and_ops_cannot_be_replayed() {
        let mut net = Net::new(2);
        let op = net.invite_op(0, 1);
        let t = net.now();
        let block = net.block(0, t, vec![op.clone()]);
        net.ledger.apply(&block).unwrap();
        assert!(net.ledger.apply(&block).is_err());
        assert!(net.commit(0, t, vec![op]).is_err());
        let mut stale = net.block(0, t, vec![]);
        stale.prev = "nope".into();
        assert!(net.ledger.apply(&stale).is_err());
        assert!(net.commit(0, t - 1, vec![]).is_err());
    }

    #[test]
    fn a_failed_op_leaves_the_ledger_untouched() {
        let mut net = Net::new(3);
        let before = net.ledger.clone();
        let good = net.invite_op(0, 1);
        let t = net.now();
        let bad = net.observe(2, 0, 1, 1, t);
        assert!(net.commit(0, t, vec![good, bad]).is_err());
        assert_eq!(net.ledger, before);
    }

    #[test]
    fn observations_promote_reliable_users_after_thirty_days() {
        let mut net = Net::everyone_joined(2);
        let mut t = net.now();
        for _ in 0..31 {
            t += EPOCH_SECS;
            let op = net.observe(0, 1, 10, 10, t);
            net.commit(0, t, vec![op]).unwrap();
        }
        t += EPOCH_SECS;
        net.commit(0, t, vec![]).unwrap();
        assert_eq!(
            net.ledger.snapshot.tier(&net.fp(1)),
            Some(ReputationTier::R2)
        );
        assert_eq!(
            net.ledger.snapshot.tier(&net.fp(0)),
            Some(ReputationTier::R1)
        );
        assert!(net.ledger.has_capability(&net.fp(1), Capability::Invite));
        assert!(!net.ledger.has_capability(&net.fp(0), Capability::Invite));
    }

    #[test]
    fn observations_are_bounded() {
        let mut net = Net::everyone_joined(2);
        let t = net.now();
        let about_self = net.observe(0, 0, 10, 10, t);
        assert!(net.commit(0, t, vec![about_self]).is_err());
        let too_many = net.observe(0, 1, MAX_CHALLENGES_PER_DEVICE_PER_EPOCH + 1, 0, t);
        assert!(net.commit(0, t, vec![too_many]).is_err());
        let first = net.observe(0, 1, 5, 5, t);
        net.commit(0, t, vec![first]).unwrap();
        let second = net.observe(0, 1, 5, 5, t);
        assert!(net.commit(0, t, vec![second]).is_err());
        let wrong_epoch = net.observe(0, 1, 5, 5, t + EPOCH_SECS);
        assert!(net.commit(0, t, vec![wrong_epoch]).is_err());
    }

    #[test]
    fn no_user_gets_more_than_a_quarter_of_the_weight() {
        let mut net = Net::everyone_joined(5);
        for m in net.ledger.members.values_mut().take(2) {
            m.sent = 10_000;
            m.passed = 10_000;
            m.joined_at = 0;
        }
        let snapshot = net.ledger.compute_snapshot(1_000);
        let total = snapshot.total_weight();
        assert!(snapshot.users.values().all(|s| s.weight * 4 <= total));
        assert!(snapshot
            .users
            .values()
            .any(|s| s.tier == ReputationTier::R4));
    }

    #[test]
    fn quorum_needs_more_than_two_thirds_of_the_weight() {
        let net = Net::everyone_joined(4);
        let all: Vec<String> = (0..4).map(|i| net.fp(i)).collect();
        assert!(net.ledger.snapshot.is_quorum(&all[..3]));
        assert!(!net.ledger.snapshot.is_quorum(&all[..2]));
        assert!(!net.ledger.snapshot.is_quorum([&all[0], &all[0], &all[0]]));
    }

    #[test]
    fn expulsion_needs_two_thirds_of_weight_and_half_of_members() {
        let mut net = Net::everyone_joined(5);
        let t = net.now();
        let target = net.fp(4);
        let propose = SignedOp::sign(
            &net.people[0],
            Op::ProposeExpel {
                member: target.clone(),
            },
        )
        .unwrap();
        net.commit(0, t, vec![propose]).unwrap();
        let approve = |net: &Net, voter: usize| {
            SignedOp::sign(
                &net.people[voter],
                Op::ApproveExpel {
                    member: target.clone(),
                },
            )
            .unwrap()
        };
        let op = approve(&net, 1);
        net.commit(0, t, vec![op]).unwrap();
        assert!(net.ledger.is_member(&target), "2 of 4 votes are not enough");
        let op = SignedOp::sign(
            &net.people[2],
            Op::ApproveExpel {
                member: target.clone(),
            },
        )
        .unwrap();
        net.commit(0, t, vec![op]).unwrap();
        assert!(!net.ledger.is_member(&target));

        let late = net.observe(4, 1, 1, 1, t);
        assert!(net.commit(0, t, vec![late]).is_err());
        assert!(net.commit(4, t, vec![]).is_err());
    }

    #[test]
    fn members_cannot_propose_expulsions_without_the_capability() {
        let mut net = Net::everyone_joined(3);
        let t = net.now();
        let op = SignedOp::sign(&net.people[1], Op::ProposeExpel { member: net.fp(2) }).unwrap();
        assert!(net.commit(1, t, vec![op]).is_err());
    }
}

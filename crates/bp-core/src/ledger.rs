use crate::{
    error::{BpError, BpResult},
    identity::{parse_user_key, DeviceCert, Identity},
    membership::{Entry, WrappedKey},
    network::ReputationTier,
    storage::StorageTier,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EPOCH_SECS: u64 = 86_400;
pub const MAX_CHALLENGES_PER_DEVICE_PER_EPOCH: u32 = 2880;
pub const MIN_CHALLENGES_FOR_PROMOTION: u64 = 100;
pub const MAX_BYTES_PER_DEVICE: u64 = 1 << 46;
pub const USAGE_INTERVAL_SECS: u64 = 120;
pub const MAX_CHUNKS_PER_FILE: usize = 1 << 20;
const DAY: u64 = 86_400;
const PPM: i64 = 1_000_000;
const STAKE: i64 = 250_000;
const SPONSOR_REWARD: i64 = 50_000;
const FREE_COMMITMENTS: usize = 8;

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
    Usage {
        verified: BTreeMap<String, u64>,
        held: BTreeMap<String, u64>,
    },
    ProposeExpel {
        member: String,
    },
    ApproveExpel {
        member: String,
    },
    RegisterDevice {
        cert: DeviceCert,
    },
    SetVotingDevice {
        device: String,
    },
    Leave,
    Place {
        file_id: String,
        k: usize,
        layer: StorageTier,
        chunks: Vec<String>,
        bytes: u64,
        holders: BTreeSet<String>,
    },
    Hold {
        file_id: String,
        device: String,
    },
    Release {
        file_id: String,
        device: String,
    },
    Drop {
        file_id: String,
    },
    Trust {
        file_id: String,
        released: String,
        newcomer: String,
        confidence_ppm: u32,
    },
    SetParams {
        params: Params,
    },
    RotateKey {
        epoch: u64,
        wrapped: BTreeMap<String, WrappedKey>,
        commitment: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    pub owner: String,
    pub k: usize,
    pub layer: StorageTier,
    pub chunks: Vec<String>,
    pub bytes: u64,
    pub planned: usize,
    pub holders: BTreeSet<String>,
    pub extra: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    pub ph_ppm: u32,
    pub decay_ppm: u32,
    pub horizon_epochs: u64,
    pub grace_epochs: u64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            ph_ppm: 999_000,
            decay_ppm: 900_000,
            horizon_epochs: 7,
            grace_epochs: 2,
        }
    }
}

impl Params {
    pub fn ph(&self) -> f64 {
        f64::from(self.ph_ppm) / PPM as f64
    }

    fn valid(&self) -> bool {
        (1..1_000_000).contains(&self.ph_ppm)
            && self.decay_ppm <= 1_000_000
            && (1..=365).contains(&self.horizon_epochs)
            && (1..=365).contains(&self.grace_epochs)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bet {
    pub user: String,
    pub file_id: String,
    pub released: String,
    pub newcomer: String,
    pub confidence_ppm: u32,
    pub baseline_ppm: u32,
    pub until_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sponsorship {
    pub sponsor: String,
    pub member: String,
    pub from_epoch: u64,
    pub until_epoch: u64,
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
    pub voting_device: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub user: String,
    pub key: String,
    pub since: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub genesis: Entry,
    pub height: u64,
    pub head: String,
    pub time: u64,
    pub members: BTreeMap<String, Member>,
    pub devices: BTreeMap<String, Device>,
    pub entries: Vec<Entry>,
    pub expelled: BTreeSet<String>,
    pub left: BTreeSet<String>,
    pub expel_approvals: BTreeMap<String, BTreeSet<String>>,
    pub reported: BTreeSet<(String, u64)>,
    pub applied_ops: BTreeSet<String>,
    pub snapshot: Snapshot,
    pub commitment: String,
    pub key_wraps: BTreeMap<String, WrappedKey>,
    pub rotated_epoch: u64,
    pub rekey_since: Option<u64>,
    pub verified: BTreeMap<String, BTreeMap<String, u64>>,
    pub held: BTreeMap<String, BTreeMap<String, u64>>,
    pub usage_at: BTreeMap<String, u64>,
    pub placements: BTreeMap<String, Placement>,
    pub proven: BTreeMap<String, u64>,
    pub last_proof: u64,
    pub params: Params,
    pub param_votes: BTreeMap<String, Params>,
    pub bets: Vec<Bet>,
    pub sponsorships: Vec<Sponsorship>,
    pub trust: BTreeMap<String, i64>,
}

impl Ledger {
    pub fn new(genesis: Entry, creator_device: &DeviceCert) -> BpResult<Self> {
        let (_, creator) = parse_user_key(&genesis.body.author)?;
        let device = creator_device.verify()?;
        if device.fingerprint != creator {
            return Err(invalid("the first device must belong to the creator"));
        }
        let peer = device.peer_id.to_string();
        let mut members = BTreeMap::new();
        members.insert(
            creator.clone(),
            Member {
                key: genesis.body.author.clone(),
                joined_at: genesis.body.at,
                sent: 0,
                passed: 0,
                voting_device: Some(peer.clone()),
            },
        );
        let devices = BTreeMap::from([(
            peer,
            Device {
                user: creator,
                key: creator_device.device_key.clone(),
                since: genesis.body.at / EPOCH_SECS,
            },
        )]);
        let mut ledger = Self {
            head: genesis.id(),
            time: genesis.body.at,
            height: 0,
            members,
            devices,
            entries: Vec::new(),
            expelled: BTreeSet::new(),
            left: BTreeSet::new(),
            expel_approvals: BTreeMap::new(),
            reported: BTreeSet::new(),
            applied_ops: BTreeSet::new(),
            snapshot: Snapshot {
                epoch: genesis.body.at / EPOCH_SECS,
                users: BTreeMap::new(),
            },
            commitment: genesis.body.commitment.clone(),
            key_wraps: BTreeMap::new(),
            rotated_epoch: genesis.body.at / EPOCH_SECS,
            rekey_since: None,
            verified: BTreeMap::new(),
            held: BTreeMap::new(),
            usage_at: BTreeMap::new(),
            placements: BTreeMap::new(),
            proven: BTreeMap::new(),
            last_proof: 0,
            params: Params::default(),
            param_votes: BTreeMap::new(),
            bets: Vec::new(),
            sponsorships: Vec::new(),
            trust: BTreeMap::new(),
            genesis,
        };
        ledger.snapshot = ledger.compute_snapshot(ledger.snapshot.epoch);
        Ok(ledger)
    }

    pub fn voting_device(&self, user: &str) -> Option<&str> {
        self.members.get(user)?.voting_device.as_deref()
    }

    pub fn device_owner(&self, device: &str) -> Option<&str> {
        self.devices.get(device).map(|d| d.user.as_str())
    }

    pub fn device_key(&self, device: &str) -> Option<libp2p::identity::PublicKey> {
        let bytes = hex::decode(&self.devices.get(device)?.key).ok()?;
        libp2p::identity::PublicKey::try_decode_protobuf(&bytes).ok()
    }

    pub fn creator(&self) -> String {
        parse_user_key(&self.genesis.body.author)
            .map(|(_, fp)| fp)
            .unwrap_or_default()
    }

    pub fn is_member(&self, user: &str) -> bool {
        self.members.contains_key(user)
            && !self.expelled.contains(user)
            && !self.left.contains(user)
    }

    pub fn is_live(&self, device: &str) -> bool {
        let Some(d) = self.devices.get(device) else {
            return false;
        };
        let seen = self.proven.get(device).copied().unwrap_or(0).max(d.since);
        self.is_member(&d.user) && seen + self.params.grace_epochs >= self.last_proof
    }

    pub fn survival(&self, device: &str) -> f64 {
        let Some(d) = self.devices.get(device) else {
            return 0.0;
        };
        let lived = self
            .proven
            .get(device)
            .map_or(0, |&e| e.saturating_sub(d.since)) as f64;
        let horizon = i32::try_from(self.params.horizon_epochs).unwrap_or(i32::MAX);
        ((lived + 1.0) / (lived + 2.0)).powi(horizon)
    }

    pub fn live_holders<'a>(&'a self, placement: &'a Placement) -> Vec<&'a String> {
        placement
            .holders
            .iter()
            .chain(&placement.extra)
            .filter(|device| self.is_live(device))
            .collect()
    }

    pub fn trust_score(&self, user: &str) -> f64 {
        self.trust.get(user).copied().unwrap_or(0) as f64 / PPM as f64
    }

    pub fn open_bets(&self, user: &str) -> usize {
        self.bets.iter().filter(|b| b.user == user).count()
    }

    pub fn sponsoring(&self, user: &str) -> usize {
        self.sponsorships
            .iter()
            .filter(|s| s.sponsor == user)
            .count()
    }

    pub fn locked_trust(&self, user: &str) -> f64 {
        (self.open_bets(user) + self.sponsoring(user)) as f64 * STAKE as f64 / PPM as f64
    }

    pub fn allowance(&self, user: &str) -> usize {
        let trust = self.trust.get(user).copied().unwrap_or(0).max(0);
        FREE_COMMITMENTS + usize::try_from(trust / STAKE).unwrap_or(usize::MAX)
    }

    fn may_commit(&self, user: &str) -> bool {
        self.open_bets(user) + self.sponsoring(user) < self.allowance(user)
    }

    fn invited(&self, by: &str, user: &str) -> bool {
        let (Some(by), Some(user)) = (self.members.get(by), self.members.get(user)) else {
            return false;
        };
        self.entries
            .iter()
            .any(|e| e.body.author == by.key && e.body.member == user.key)
    }

    fn promote(&mut self, file_id: &str) {
        let Some(p) = self.placements.get(file_id) else {
            return;
        };
        let charged = p.holders.iter().filter(|d| self.is_live(d)).count();
        let promoted: Vec<String> = p
            .extra
            .iter()
            .filter(|d| self.is_live(d))
            .take(p.planned.saturating_sub(charged))
            .cloned()
            .collect();
        let p = self.placements.get_mut(file_id).expect("checked above");
        for device in promoted {
            p.extra.remove(&device);
            p.holders.insert(device);
        }
    }

    fn vacate(&mut self, user: &str) {
        let gone: BTreeSet<String> = self
            .devices
            .iter()
            .filter(|(_, d)| d.user == user)
            .map(|(peer, _)| peer.clone())
            .collect();
        let files: Vec<String> = self.placements.keys().cloned().collect();
        for file_id in files {
            let p = self.placements.get_mut(&file_id).expect("listed above");
            p.holders.retain(|d| !gone.contains(d));
            p.extra.retain(|d| !gone.contains(d));
            self.promote(&file_id);
        }
    }

    pub fn may_trust(
        &self,
        user: &str,
        file_id: &str,
        released: &str,
        newcomer: &str,
    ) -> Result<(), &'static str> {
        let p = self.placements.get(file_id).ok_or("no such file")?;
        if self.device_owner(released) != Some(user) || !p.holders.contains(released) {
            return Err("you do not hold this file");
        }
        let other = self.device_owner(newcomer).ok_or("unknown newcomer")?;
        if !p.extra.contains(newcomer) || !self.is_live(newcomer) {
            return Err("the newcomer is not an extra holder");
        }
        if other == user {
            return Err("you cannot trust your own devices");
        }
        let open = self.bets.iter().filter(|b| b.file_id == file_id).count();
        if open + p.k >= p.planned {
            return Err("the file has no tolerance left for untested holders");
        }
        if !self.may_commit(user) {
            return Err("too many open bets and invitations for your trust");
        }
        Ok(())
    }

    fn settle_bets(&mut self, epoch: u64) {
        for bet in std::mem::take(&mut self.bets) {
            let Some(p) = self.placements.get(&bet.file_id) else {
                continue;
            };
            if !self.is_member(&bet.user) {
                continue;
            }
            let holding = (p.holders.contains(&bet.newcomer) || p.extra.contains(&bet.newcomer))
                && self.is_live(&bet.newcomer);
            if holding && epoch < bet.until_epoch {
                self.bets.push(bet);
                continue;
            }
            let newcomer = self.device_owner(&bet.newcomer).unwrap_or_default();
            if self.invited(&bet.user, newcomer) {
                continue;
            }
            let outcome = if holding { PPM } else { 0 };
            let baseline = i64::from(bet.baseline_ppm);
            let forecast = i64::from(bet.confidence_ppm);
            let skill = ((baseline - outcome).pow(2) - (forecast - outcome).pow(2)) / PPM;
            let decay = i64::from(self.params.decay_ppm);
            let score = self.trust.entry(bet.user).or_default();
            *score = *score * decay / PPM + skill;
        }
        for sponsorship in std::mem::take(&mut self.sponsorships) {
            let worked = self.devices.iter().any(|(device, d)| {
                d.user == sponsorship.member
                    && self.is_live(device)
                    && self
                        .proven
                        .get(device)
                        .is_some_and(|&e| e >= sponsorship.from_epoch)
            });
            let gone = !self.is_member(&sponsorship.member);
            if !gone && epoch < sponsorship.until_epoch {
                self.sponsorships.push(sponsorship);
                continue;
            }
            let change = match (gone, worked) {
                (true, _) => -STAKE,
                (false, true) => SPONSOR_REWARD,
                (false, false) => 0,
            };
            *self.trust.entry(sponsorship.sponsor).or_default() += change;
        }
    }

    pub fn active_members(&self) -> impl Iterator<Item = (&String, &Member)> {
        self.members.iter().filter(|(user, _)| self.is_member(user))
    }

    pub fn credit(&self, user: &str) -> u64 {
        let mut by_device: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for (observer, devices) in &self.verified {
            if !self.is_member(observer) {
                continue;
            }
            for (device, &bytes) in devices {
                if self.device_owner(device) == Some(user) {
                    by_device.entry(device).or_default().push(bytes);
                }
            }
        }
        by_device
            .into_values()
            .map(|mut reports| {
                reports.sort_unstable();
                reports[reports.len() / 2]
            })
            .fold(0u64, u64::saturating_add)
    }

    pub fn usage(&self, user: &str) -> u64 {
        self.held
            .iter()
            .filter(|(reporter, _)| self.is_member(reporter))
            .filter_map(|(_, owners)| owners.get(user))
            .fold(0u64, |a, &b| a.saturating_add(b))
    }

    pub fn may_store(&self, user: &str, more: u64) -> bool {
        self.usage(user).saturating_add(more) <= self.credit(user)
    }

    pub fn rotation_due(&self, now_secs: u64) -> Option<u64> {
        let epoch = now_secs / EPOCH_SECS;
        self.rekey_since
            .or_else(|| (epoch > self.rotated_epoch).then_some(epoch * EPOCH_SECS))
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
        next.settle_bets(block.epoch());
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
                    || entry.body.commitment != self.commitment
                    || entry.signed_by_author().is_none()
                {
                    return Err(invalid("malformed member entry"));
                }
                let (_, member) = parse_user_key(&entry.body.member)?;
                if self.members.contains_key(&member) {
                    return Err(invalid("already a member"));
                }
                if !self.may_commit(&author) {
                    return Err(invalid("too many open bets and invitations for your trust"));
                }
                self.sponsorships.push(Sponsorship {
                    sponsor: author.clone(),
                    member: member.clone(),
                    from_epoch: block.epoch(),
                    until_epoch: block.epoch() + self.params.horizon_epochs,
                });
                self.members.insert(
                    member,
                    Member {
                        key: entry.body.member.clone(),
                        joined_at: block.time,
                        sent: 0,
                        passed: 0,
                        voting_device: None,
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
                    if o.passed > 0 && self.device_owner(&o.device) == Some(o.user.as_str()) {
                        self.proven.insert(o.device.clone(), *epoch);
                        self.last_proof = self.last_proof.max(*epoch);
                    }
                }
                let files: Vec<String> = self.placements.keys().cloned().collect();
                for file_id in files {
                    self.promote(&file_id);
                }
            }
            Op::Usage { verified, held } => {
                if self
                    .usage_at
                    .get(&author)
                    .is_some_and(|&at| block.time < at + USAGE_INTERVAL_SECS)
                {
                    return Err(invalid("usage reported too often"));
                }
                let bad_device = verified.iter().any(|(device, &bytes)| {
                    bytes > MAX_BYTES_PER_DEVICE
                        || self
                            .device_owner(device)
                            .is_none_or(|owner| owner == author || !self.is_member(owner))
                });
                let bad_owner = held.iter().any(|(owner, &bytes)| {
                    bytes > MAX_BYTES_PER_DEVICE || *owner == author || !self.is_member(owner)
                });
                if bad_device || bad_owner {
                    return Err(invalid("invalid usage report"));
                }
                self.usage_at.insert(author.clone(), block.time);
                self.verified.insert(author.clone(), verified.clone());
                self.held.insert(author.clone(), held.clone());
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
                self.maybe_expel(member, block.time);
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
                self.maybe_expel(member, block.time);
            }
            Op::RegisterDevice { cert } => {
                let device = cert.verify()?;
                let peer = device.peer_id.to_string();
                if device.fingerprint != author || self.devices.contains_key(&peer) {
                    return Err(invalid("cannot register this device"));
                }
                self.devices.insert(
                    peer.clone(),
                    Device {
                        user: author.clone(),
                        key: cert.device_key.clone(),
                        since: block.epoch(),
                    },
                );
                let member = self.members.get_mut(&author).expect("checked member");
                member.voting_device.get_or_insert(peer);
            }
            Op::SetVotingDevice { device } => {
                if self.devices.get(device).map(|d| &d.user) != Some(&author) {
                    return Err(invalid("not one of your devices"));
                }
                self.members
                    .get_mut(&author)
                    .expect("checked member")
                    .voting_device = Some(device.clone());
            }
            Op::Leave => {
                self.left.insert(author.clone());
                self.vacate(&author);
                self.expel_approvals.remove(&author);
                self.snapshot.users.remove(&author);
                self.rekey_since.get_or_insert(block.time);
            }
            Op::Place {
                file_id,
                k,
                layer,
                chunks,
                bytes,
                holders,
            } => {
                if self.placements.contains_key(file_id)
                    || *k == 0
                    || *k > holders.len()
                    || chunks.is_empty()
                    || chunks.len() > MAX_CHUNKS_PER_FILE
                    || *bytes > MAX_BYTES_PER_DEVICE
                    || !holders.iter().all(|d| self.is_live(d))
                {
                    return Err(invalid("invalid placement"));
                }
                self.placements.insert(
                    file_id.clone(),
                    Placement {
                        owner: author.clone(),
                        k: *k,
                        layer: *layer,
                        chunks: chunks.clone(),
                        bytes: *bytes,
                        planned: holders.len(),
                        holders: holders.clone(),
                        extra: BTreeSet::new(),
                    },
                );
            }
            Op::Hold { file_id, device } => {
                let owned = self.device_owner(device) == Some(author.as_str());
                let Some(p) = self.placements.get(file_id) else {
                    return Err(invalid("no such file"));
                };
                if !owned || p.holders.contains(device) || p.extra.contains(device) {
                    return Err(invalid("invalid holder"));
                }
                let charged = p.holders.iter().filter(|d| self.is_live(d)).count();
                let full = charged >= p.planned;
                let p = self.placements.get_mut(file_id).expect("checked above");
                if full {
                    p.extra.insert(device.clone());
                } else {
                    p.holders.insert(device.clone());
                }
            }
            Op::Release { file_id, device } => {
                let owned = self.device_owner(device) == Some(author.as_str());
                let Some(p) = self.placements.get(file_id) else {
                    return Err(invalid("no such file"));
                };
                if !owned || !(p.holders.contains(device) || p.extra.contains(device)) {
                    return Err(invalid("invalid release"));
                }
                let p = self.placements.get_mut(file_id).expect("checked above");
                p.holders.remove(device);
                p.extra.remove(device);
                self.promote(file_id);
            }
            Op::Drop { file_id } => {
                if self.placements.get(file_id).map(|p| &p.owner) != Some(&author) {
                    return Err(invalid("only the owner drops a file"));
                }
                self.placements.remove(file_id);
            }
            Op::Trust {
                file_id,
                released,
                newcomer,
                confidence_ppm,
            } => {
                if *confidence_ppm > 1_000_000 {
                    return Err(invalid("confidence above 1"));
                }
                self.may_trust(&author, file_id, released, newcomer)
                    .map_err(invalid)?;
                let p = self.placements.get_mut(file_id).expect("checked above");
                p.holders.remove(released);
                p.extra.remove(newcomer);
                p.holders.insert(newcomer.clone());
                self.bets.push(Bet {
                    user: author.clone(),
                    file_id: file_id.clone(),
                    released: released.clone(),
                    newcomer: newcomer.clone(),
                    confidence_ppm: *confidence_ppm,
                    baseline_ppm: (self.survival(newcomer) * PPM as f64) as u32,
                    until_epoch: block.epoch() + self.params.horizon_epochs,
                });
            }
            Op::SetParams { params } => {
                if !self.has_capability(&author, Capability::Vote) || !params.valid() {
                    return Err(invalid("cannot vote these parameters"));
                }
                self.param_votes.insert(author.clone(), *params);
                let voters: Vec<&String> = self
                    .param_votes
                    .iter()
                    .filter(|(_, v)| *v == params)
                    .map(|(u, _)| u)
                    .collect();
                if self.snapshot.is_quorum(voters) {
                    self.params = *params;
                    self.param_votes.clear();
                }
            }
            Op::RotateKey {
                epoch,
                wrapped,
                commitment,
            } => {
                let members: BTreeSet<&String> = self.active_members().map(|(u, _)| u).collect();
                if *epoch != block.epoch()
                    || self.rotation_due(block.time).is_none()
                    || wrapped.keys().collect::<BTreeSet<_>>() != members
                    || *commitment == self.commitment
                {
                    return Err(invalid("invalid key rotation"));
                }
                self.commitment = commitment.clone();
                self.key_wraps = wrapped.clone();
                self.rotated_epoch = *epoch;
                self.rekey_since = None;
            }
        }
        self.applied_ops.insert(id);
        Ok(())
    }

    fn maybe_expel(&mut self, member: &str, time: u64) {
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
            self.vacate(member);
            self.expel_approvals.remove(member);
            self.snapshot.users.remove(member);
            self.rekey_since.get_or_insert(time);
        }
    }

    pub fn compute_snapshot(&self, epoch: u64) -> Snapshot {
        let epoch_start = epoch * EPOCH_SECS;
        let mut users: BTreeMap<String, Standing> = self
            .members
            .iter()
            .filter(|(fp, _)| self.is_member(fp))
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
    use crate::membership::{genesis, invite_entry, key_commitment, new_key, wrap_for};

    struct Net {
        people: Vec<Identity>,
        ledger: Ledger,
        key: [u8; 32],
    }

    impl Net {
        fn new(n: usize) -> Self {
            let people: Vec<Identity> =
                (0..n).map(|_| Identity::in_memory(None).unwrap()).collect();
            let (root, key) = genesis(&people[0], "amici").unwrap();
            let ledger = Ledger::new(root, &people[0].cert).unwrap();
            Self {
                people,
                ledger,
                key,
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
            self.ledger.apply(&block)
        }

        fn next_epoch(&mut self) {
            let t = self.now() + EPOCH_SECS;
            self.commit(0, t, vec![]).unwrap();
        }

        fn invite_op(&mut self, by: usize, who: usize) -> SignedOp {
            let member_key = self.people[who].user_key.clone();
            let entry = invite_entry(
                &self.people[by],
                &self.ledger.genesis,
                &self.key,
                &member_key,
            )
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

        fn rotate_op(&self, by: usize, members: &[usize], time: u64) -> SignedOp {
            let key = new_key();
            let wrapped = members
                .iter()
                .map(|&i| {
                    (
                        self.fp(i),
                        wrap_for(&key, &self.people[i].user_key).unwrap(),
                    )
                })
                .collect();
            SignedOp::sign(
                &self.people[by],
                Op::RotateKey {
                    epoch: time / EPOCH_SECS,
                    wrapped,
                    commitment: key_commitment(&key),
                },
            )
            .unwrap()
        }
    }

    #[test]
    fn the_key_rotates_once_per_epoch_for_exactly_the_members() {
        let mut net = Net::new(4);
        for who in 1..3 {
            let op = net.invite_op(0, who);
            let t = net.now();
            net.commit(0, t, vec![op]).unwrap();
        }
        let t = net.now();
        assert_eq!(net.ledger.rotation_due(t), None);
        assert!(net
            .commit(0, t, vec![net.rotate_op(0, &[0, 1, 2], t)])
            .is_err());

        net.next_epoch();
        let t = net.now();
        assert!(net.ledger.rotation_due(t).is_some());
        assert!(net
            .commit(0, t, vec![net.rotate_op(0, &[0, 1], t)])
            .is_err());
        assert!(net
            .commit(0, t, vec![net.rotate_op(0, &[0, 1, 2, 3], t)])
            .is_err());
        net.commit(0, t, vec![net.rotate_op(0, &[0, 1, 2], t)])
            .unwrap();
        assert_eq!(net.ledger.rotation_due(t), None);
        assert!(net
            .commit(0, t, vec![net.rotate_op(1, &[0, 1, 2], t)])
            .is_err());

        let stale = net.invite_op(0, 3);
        assert!(net.commit(0, t, vec![stale]).is_err());
    }

    #[test]
    fn an_expulsion_makes_a_rotation_due_without_the_expelled() {
        let mut net = Net::everyone_joined(3);
        let t = net.now();
        net.commit(0, t, vec![net.rotate_op(0, &[0, 1, 2], t)])
            .unwrap();
        let lucia = net.fp(2);
        let propose = SignedOp::sign(
            &net.people[0],
            Op::ProposeExpel {
                member: lucia.clone(),
            },
        )
        .unwrap();
        let approve = SignedOp::sign(
            &net.people[1],
            Op::ApproveExpel {
                member: lucia.clone(),
            },
        )
        .unwrap();
        net.commit(0, t, vec![propose, approve]).unwrap();
        assert!(!net.ledger.is_member(&lucia));
        assert_eq!(net.ledger.snapshot.weight(&lucia), 0);
        assert_eq!(net.ledger.rotation_due(t), Some(t));
        assert!(net
            .commit(0, t, vec![net.rotate_op(0, &[0, 1, 2], t)])
            .is_err());
        net.commit(0, t, vec![net.rotate_op(1, &[0, 1], t)])
            .unwrap();
        assert_eq!(net.ledger.rotation_due(t), None);
    }

    #[test]
    fn a_member_who_leaves_is_out_and_the_key_rotates() {
        let mut net = Net::everyone_joined(3);
        let t = net.now();
        net.commit(0, t, vec![net.rotate_op(0, &[0, 1, 2], t)])
            .unwrap();
        let leave = SignedOp::sign(&net.people[2], Op::Leave).unwrap();
        net.commit(0, t, vec![leave]).unwrap();
        assert!(!net.ledger.is_member(&net.fp(2)));
        assert!(net.ledger.left.contains(&net.fp(2)));
        assert_eq!(net.ledger.snapshot.weight(&net.fp(2)), 0);
        assert_eq!(net.ledger.rotation_due(t), Some(t));
        let again = SignedOp::sign(&net.people[2], Op::Leave).unwrap();
        assert!(net.commit(0, t, vec![again]).is_err());
        net.commit(0, t, vec![net.rotate_op(1, &[0, 1], t)])
            .unwrap();
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

    fn usage(net: &Net, by: usize, verified: &[(usize, u64)], held: &[(usize, u64)]) -> SignedOp {
        SignedOp::sign(
            &net.people[by],
            Op::Usage {
                verified: verified
                    .iter()
                    .map(|&(i, b)| (net.people[i].peer_id.to_string(), b))
                    .collect(),
                held: held.iter().map(|&(i, b)| (net.fp(i), b)).collect(),
            },
        )
        .unwrap()
    }

    #[test]
    fn credit_and_usage_are_the_latest_reports_of_the_others() {
        let mut net = Net::everyone_joined(3);
        let mut t = net.now();
        for i in 1..3 {
            let op = SignedOp::sign(
                &net.people[i],
                Op::RegisterDevice {
                    cert: net.people[i].cert.clone(),
                },
            )
            .unwrap();
            net.commit(0, t, vec![op]).unwrap();
        }
        let marco = net.fp(1);
        assert_eq!(
            (net.ledger.credit(&marco), net.ledger.usage(&marco)),
            (0, 0)
        );
        assert!(!net.ledger.may_store(&marco, 1));
        assert!(net.ledger.may_store(&marco, 0));

        let a = usage(&net, 0, &[(1, 600)], &[(1, 100)]);
        let b = usage(&net, 2, &[(1, 1000)], &[(1, 200)]);
        net.commit(0, t, vec![a, b]).unwrap();
        assert_eq!(net.ledger.credit(&marco), 1000, "median of 600 and 1000");
        assert_eq!(net.ledger.usage(&marco), 300);
        assert!(net.ledger.may_store(&marco, 700));
        assert!(!net.ledger.may_store(&marco, 701));

        let too_soon = usage(&net, 0, &[(1, 50)], &[]);
        assert!(net.commit(0, t + 1, vec![too_soon]).is_err());
        t += USAGE_INTERVAL_SECS;
        let replaced = usage(&net, 0, &[(1, 50)], &[]);
        net.commit(0, t, vec![replaced]).unwrap();
        assert_eq!(net.ledger.credit(&marco), 1000, "median of 50 and 1000");
        assert_eq!(net.ledger.usage(&marco), 200);

        t += USAGE_INTERVAL_SECS;
        for bad in [
            usage(&net, 1, &[(1, 10)], &[]),
            usage(&net, 1, &[], &[(1, 10)]),
            usage(&net, 0, &[(2, MAX_BYTES_PER_DEVICE + 1)], &[]),
        ] {
            assert!(net.commit(0, t, vec![bad]).is_err());
        }

        let leave = SignedOp::sign(&net.people[2], Op::Leave).unwrap();
        net.commit(0, t, vec![leave]).unwrap();
        assert_eq!(net.ledger.credit(&marco), 50, "only one observer left");
        assert_eq!(net.ledger.usage(&marco), 0);
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

    #[test]
    fn each_user_votes_from_one_registered_device() {
        let mut net = Net::everyone_joined(2);
        let t = net.now();
        assert_eq!(
            net.ledger.voting_device(&net.fp(0)),
            Some(net.people[0].peer_id.to_string().as_str())
        );
        assert_eq!(net.ledger.voting_device(&net.fp(1)), None);

        let laptop = net.people[1].cert.clone();
        let op = SignedOp::sign(&net.people[1], Op::RegisterDevice { cert: laptop }).unwrap();
        net.commit(0, t, vec![op]).unwrap();
        let laptop_id = net.people[1].peer_id.to_string();
        assert_eq!(
            net.ledger.voting_device(&net.fp(1)),
            Some(laptop_id.as_str())
        );
        assert_eq!(
            net.ledger.device_owner(&laptop_id),
            Some(net.fp(1).as_str())
        );
        assert!(net.ledger.device_key(&laptop_id).is_some());

        let stolen = SignedOp::sign(
            &net.people[0],
            Op::RegisterDevice {
                cert: net.people[1].cert.clone(),
            },
        )
        .unwrap();
        assert!(net.commit(0, t, vec![stolen]).is_err());
        let hijack =
            SignedOp::sign(&net.people[0], Op::SetVotingDevice { device: laptop_id }).unwrap();
        assert!(net.commit(0, t, vec![hijack]).is_err());
    }

    #[test]
    fn placements_track_who_holds_a_file() {
        let mut net = Net::everyone_joined(3);
        for who in 1..3 {
            let op = SignedOp::sign(
                &net.people[who],
                Op::RegisterDevice {
                    cert: net.people[who].cert.clone(),
                },
            )
            .unwrap();
            let t = net.now();
            net.commit(0, t, vec![op]).unwrap();
        }
        let device = |i: usize| net.people[i].peer_id.to_string();
        let (d0, d1, d2) = (device(0), device(1), device(2));
        let people = net.people.clone();
        let sign = |who: usize, op: Op| SignedOp::sign(&people[who], op).unwrap();
        let place = |holders: &[&String], k: usize| Op::Place {
            file_id: "f".into(),
            k,
            layer: StorageTier::T1,
            chunks: vec!["c1".into(), "c2".into()],
            bytes: 1 << 20,
            holders: holders.iter().map(|d| (*d).clone()).collect(),
        };
        let t = net.now();
        assert!(net
            .commit(0, t, vec![sign(0, place(&[&d0, &d1], 3))])
            .is_err());
        let unknown = "12D3KooWnobody".to_string();
        assert!(net
            .commit(0, t, vec![sign(0, place(&[&d0, &unknown], 1))])
            .is_err());
        net.commit(0, t, vec![sign(0, place(&[&d0, &d1], 1))])
            .unwrap();
        let p = &net.ledger.placements["f"];
        assert_eq!((p.planned, p.k, p.owner.clone()), (2, 1, net.fp(0)));

        let hold = |device: &String| Op::Hold {
            file_id: "f".into(),
            device: device.clone(),
        };
        assert!(
            net.commit(0, t, vec![sign(2, hold(&d1))]).is_err(),
            "only the device's user"
        );
        assert!(
            net.commit(0, t, vec![sign(1, hold(&d1))]).is_err(),
            "already a holder"
        );
        net.commit(0, t, vec![sign(2, hold(&d2))]).unwrap();
        let p = &net.ledger.placements["f"];
        assert!(
            p.extra.contains(&d2),
            "a full file takes newcomers as extra"
        );
        assert!(
            net.commit(0, t, vec![sign(2, hold(&d2))]).is_err(),
            "already an extra holder"
        );

        let release = Op::Release {
            file_id: "f".into(),
            device: d1.clone(),
        };
        net.commit(0, t, vec![sign(1, release)]).unwrap();
        let p = &net.ledger.placements["f"];
        assert_eq!(net.ledger.live_holders(p).len(), 2);
        assert!(
            p.holders.contains(&d2) && p.extra.is_empty(),
            "an extra holder takes the place of a charged one"
        );

        let leave = sign(2, Op::Leave);
        net.commit(0, t, vec![leave]).unwrap();
        let p = &net.ledger.placements["f"];
        assert_eq!(
            net.ledger.live_holders(p),
            vec![&d0],
            "a departed user holds nothing"
        );

        let drop = || Op::Drop {
            file_id: "f".into(),
        };
        assert!(
            net.commit(0, t, vec![sign(1, drop())]).is_err(),
            "only the owner drops"
        );
        net.commit(0, t, vec![sign(0, drop())]).unwrap();
        assert!(net.ledger.placements.is_empty());
    }

    #[test]
    fn an_extra_holder_replaces_a_user_who_is_gone() {
        let mut net = Net::everyone_joined(3);
        for who in 1..3 {
            let op = SignedOp::sign(
                &net.people[who],
                Op::RegisterDevice {
                    cert: net.people[who].cert.clone(),
                },
            )
            .unwrap();
            let t = net.now();
            net.commit(0, t, vec![op]).unwrap();
        }
        let device = |i: usize| net.people[i].peer_id.to_string();
        let (d0, d1, d2) = (device(0), device(1), device(2));
        let people = net.people.clone();
        let sign = |who: usize, op: Op| SignedOp::sign(&people[who], op).unwrap();
        let t = net.now();
        let place = Op::Place {
            file_id: "f".into(),
            k: 1,
            layer: StorageTier::T1,
            chunks: vec!["c".into()],
            bytes: 1 << 20,
            holders: [d0.clone(), d1.clone()].into(),
        };
        let hold = Op::Hold {
            file_id: "f".into(),
            device: d2.clone(),
        };
        net.commit(0, t, vec![sign(0, place), sign(2, hold)])
            .unwrap();
        net.commit(0, t, vec![sign(1, Op::Leave)]).unwrap();
        let p = &net.ledger.placements["f"];
        assert_eq!(p.holders, [d0, d2].into());
        assert!(p.extra.is_empty());
    }

    #[test]
    fn a_holder_that_stops_proving_its_storage_is_replaced() {
        let mut net = Net::everyone_joined(3);
        for who in 1..3 {
            let op = SignedOp::sign(
                &net.people[who],
                Op::RegisterDevice {
                    cert: net.people[who].cert.clone(),
                },
            )
            .unwrap();
            let t = net.now();
            net.commit(0, t, vec![op]).unwrap();
        }
        let device = |i: usize| net.people[i].peer_id.to_string();
        let (d0, d1, d2) = (device(0), device(1), device(2));
        let people = net.people.clone();
        let sign = |who: usize, op: Op| SignedOp::sign(&people[who], op).unwrap();
        let t = net.now();
        let place = Op::Place {
            file_id: "f".into(),
            k: 1,
            layer: StorageTier::T1,
            chunks: vec!["c".into()],
            bytes: 1 << 20,
            holders: [d0.clone(), d1.clone()].into(),
        };
        let hold = Op::Hold {
            file_id: "f".into(),
            device: d2.clone(),
        };
        net.commit(0, t, vec![sign(0, place), sign(2, hold)])
            .unwrap();
        for _ in 0..net.ledger.params.grace_epochs {
            net.next_epoch();
            let t = net.now();
            let seen = vec![net.observe(0, 2, 5, 5, t), net.observe(2, 0, 5, 5, t)];
            net.commit(0, t, seen).unwrap();
        }
        assert!(net.ledger.is_live(&d1), "still within the grace epochs");
        net.next_epoch();
        let t = net.now();
        let seen = vec![net.observe(0, 2, 5, 5, t), net.observe(2, 0, 5, 5, t)];
        net.commit(0, t, seen).unwrap();
        assert!(!net.ledger.is_live(&d1));
        let p = &net.ledger.placements["f"];
        assert!(p.holders.contains(&d2) && p.extra.is_empty());
        assert_eq!(net.ledger.live_holders(p).len(), 2);
        assert!(net.ledger.survival(&d2) > net.ledger.survival(&d1));
    }

    #[test]
    fn trusting_a_newcomer_is_a_scored_bet() {
        let mut net = Net::everyone_joined(3);
        for who in 1..3 {
            let op = SignedOp::sign(
                &net.people[who],
                Op::RegisterDevice {
                    cert: net.people[who].cert.clone(),
                },
            )
            .unwrap();
            let t = net.now();
            net.commit(0, t, vec![op]).unwrap();
        }
        let device = |i: usize| net.people[i].peer_id.to_string();
        let (d0, d1, d2) = (device(0), device(1), device(2));
        let people = net.people.clone();
        let sign = |who: usize, op: Op| SignedOp::sign(&people[who], op).unwrap();
        let place = |file: &str| Op::Place {
            file_id: file.into(),
            k: 1,
            layer: StorageTier::T1,
            chunks: vec!["c".into()],
            bytes: 1 << 20,
            holders: [d0.clone(), d1.clone()].into(),
        };
        let hold = |file: &str| Op::Hold {
            file_id: file.into(),
            device: d2.clone(),
        };
        let trust = |file: &str, released: &String| Op::Trust {
            file_id: file.into(),
            released: released.clone(),
            newcomer: d2.clone(),
            confidence_ppm: 900_000,
        };
        let t = net.now();
        net.commit(0, t, vec![sign(0, place("f")), sign(2, hold("f"))])
            .unwrap();
        assert!(
            net.commit(0, t, vec![sign(0, trust("f", &d1))]).is_err(),
            "only with a device of your own"
        );
        net.commit(0, t, vec![sign(1, trust("f", &d1))]).unwrap();
        let p = &net.ledger.placements["f"];
        assert_eq!(p.holders, [d0.clone(), d2.clone()].into());
        assert!(p.extra.is_empty());

        net.commit(0, t, vec![sign(0, place("g")), sign(2, hold("g"))])
            .unwrap();
        for _ in 0..net.ledger.params.horizon_epochs {
            net.next_epoch();
        }
        assert!(net.ledger.bets.is_empty());
        assert_eq!(
            net.ledger.trust[&net.fp(1)],
            974_437,
            "it beat the ledger's forecast"
        );

        let t = net.now();
        net.commit(0, t, vec![sign(1, trust("g", &d1))]).unwrap();
        net.commit(0, t, vec![sign(2, Op::Leave)]).unwrap();
        assert!(
            net.ledger.bets.is_empty(),
            "a vanished newcomer loses the bet"
        );
        assert_eq!(net.ledger.trust[&net.fp(1)], 876_993 - 809_938);
    }

    #[test]
    fn inviting_stakes_trust_and_bets_on_invitees_do_not_score() {
        let mut net = Net::everyone_joined(3);
        for who in 1..3 {
            let op = SignedOp::sign(
                &net.people[who],
                Op::RegisterDevice {
                    cert: net.people[who].cert.clone(),
                },
            )
            .unwrap();
            let t = net.now();
            net.commit(0, t, vec![op]).unwrap();
        }
        let device = |i: usize| net.people[i].peer_id.to_string();
        let (d0, d1, d2) = (device(0), device(1), device(2));
        let people = net.people.clone();
        let sign = |who: usize, op: Op| SignedOp::sign(&people[who], op).unwrap();
        assert_eq!(net.ledger.sponsoring(&net.fp(0)), 2);
        let t = net.now();
        let place = Op::Place {
            file_id: "f".into(),
            k: 1,
            layer: StorageTier::T1,
            chunks: vec!["c".into()],
            bytes: 1 << 20,
            holders: [d0.clone(), d1.clone()].into(),
        };
        let hold = Op::Hold {
            file_id: "f".into(),
            device: d2.clone(),
        };
        let trust = Op::Trust {
            file_id: "f".into(),
            released: d0.clone(),
            newcomer: d2.clone(),
            confidence_ppm: 900_000,
        };
        net.commit(0, t, vec![sign(0, place), sign(2, hold)])
            .unwrap();
        net.commit(0, t, vec![sign(0, trust)]).unwrap();
        for _ in 0..net.ledger.params.horizon_epochs {
            net.next_epoch();
            let t = net.now();
            let seen = vec![net.observe(0, 1, 5, 5, t), net.observe(1, 2, 5, 5, t)];
            net.commit(0, t, seen).unwrap();
        }
        assert!(net.ledger.sponsorships.is_empty() && net.ledger.bets.is_empty());
        assert_eq!(
            net.ledger.trust[&net.fp(0)],
            2 * SPONSOR_REWARD,
            "two invitees who work, and an unscored bet on one of them"
        );

        let mut net = Net::everyone_joined(3);
        let t = net.now();
        let leave = SignedOp::sign(&net.people[2], Op::Leave).unwrap();
        net.commit(0, t, vec![leave]).unwrap();
        assert_eq!(net.ledger.trust[&net.fp(0)], -STAKE);
        for _ in 0..net.ledger.params.horizon_epochs {
            net.next_epoch();
        }
        assert_eq!(
            net.ledger.trust[&net.fp(0)],
            -STAKE,
            "an invitee who stays without working costs nothing"
        );
    }

    #[test]
    fn parameters_change_with_a_quorum() {
        let mut net = Net::everyone_joined(3);
        let people = net.people.clone();
        let wanted = Params {
            horizon_epochs: 3,
            ..Params::default()
        };
        let vote = |who: usize, params: Params| {
            SignedOp::sign(&people[who], Op::SetParams { params }).unwrap()
        };
        let t = net.now();
        let broken = Params {
            ph_ppm: 1_000_000,
            ..Params::default()
        };
        assert!(net.commit(0, t, vec![vote(0, broken)]).is_err());
        net.commit(0, t, vec![vote(0, wanted), vote(1, wanted)])
            .unwrap();
        assert_eq!(net.ledger.params, Params::default());
        net.commit(0, t, vec![vote(2, wanted)]).unwrap();
        assert_eq!(net.ledger.params, wanted);
        assert!(net.ledger.param_votes.is_empty());
    }
}

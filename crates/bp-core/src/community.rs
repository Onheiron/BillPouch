use crate::ledger::{Ledger, EPOCH_SECS};
use crate::network::ReputationTier;
use serde::{Deserialize, Serialize};

const GIB: u64 = 1 << 30;

pub const LEVELS: [(&str, u32); 6] = [
    ("Egg", 0),
    ("Hatchling", 50),
    ("Fledgling", 150),
    ("Fisher", 350),
    ("Glider", 650),
    ("Great Pelican", 1000),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub points: u32,
    pub progress_percent: u8,
    pub earned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Level {
    pub number: usize,
    pub name: String,
    pub points: u32,
    pub floor: u32,
    pub next: Option<u32>,
}

struct Facts {
    founder: bool,
    days: u64,
    credit: u64,
    used: u64,
    challenges: u64,
    passed: u64,
    invited: u64,
    trust_ppm: i64,
    devices: u64,
    files: u64,
    tier: Option<ReputationTier>,
    messages: u64,
}

fn facts(ledger: &Ledger, user: &str, messages: usize) -> Option<Facts> {
    let member = ledger.members.get(user)?;
    Some(Facts {
        founder: ledger.creator() == user,
        days: ledger.time.saturating_sub(member.joined_at) / EPOCH_SECS,
        credit: ledger.credit(user),
        used: ledger.usage(user),
        challenges: member.sent,
        passed: member.passed,
        invited: ledger
            .entries
            .iter()
            .filter(|e| e.body.author == member.key)
            .count() as u64,
        trust_ppm: ledger.trust.get(user).copied().unwrap_or(0),
        devices: ledger.devices.values().filter(|d| d.user == user).count() as u64,
        files: ledger
            .placements
            .values()
            .filter(|p| p.owner == user)
            .count() as u64,
        tier: ledger.snapshot.tier(user),
        messages: messages as u64,
    })
}

fn ratio(have: u64, want: u64) -> u8 {
    (have.min(want) * 100 / want.max(1)) as u8
}

fn rules(f: &Facts) -> Vec<(&'static str, &'static str, &'static str, u32, u8)> {
    let flag = |yes: bool| if yes { 100 } else { 0 };
    let steady = f.challenges >= 100 && f.passed * 100 >= f.challenges * 99;
    let trust = f.trust_ppm.max(0) as u64;
    vec![
        (
            "founder",
            "Founder",
            "Created the network",
            100,
            flag(f.founder),
        ),
        (
            "pouch_open",
            "Open pouch",
            "Host space the others have verified",
            50,
            flag(f.credit > 0),
        ),
        (
            "big_pouch",
            "Big pouch",
            "Host 10 GiB of verified space",
            100,
            ratio(f.credit, 10 * GIB),
        ),
        (
            "deep_pouch",
            "Deep pouch",
            "Host 100 GiB of verified space",
            200,
            ratio(f.credit, 100 * GIB),
        ),
        (
            "generous",
            "Generous",
            "Host at least twice what you store on the others",
            75,
            if f.credit == 0 {
                0
            } else {
                ratio(f.credit, f.used.saturating_mul(2).max(1))
            },
        ),
        (
            "rock_solid",
            "Rock solid",
            "Pass 99% of at least 100 challenges",
            150,
            if steady {
                100
            } else {
                ratio(f.passed, 100).min(99)
            },
        ),
        (
            "first_fish",
            "First fish",
            "Store your first file in the network",
            25,
            flag(f.files > 0),
        ),
        (
            "full_net",
            "Full net",
            "Store 100 files in the network",
            100,
            ratio(f.files, 100),
        ),
        (
            "recruiter",
            "Recruiter",
            "Invite someone who joined",
            50,
            flag(f.invited > 0),
        ),
        (
            "flock_leader",
            "Flock leader",
            "Invite five people who joined",
            150,
            ratio(f.invited, 5),
        ),
        (
            "good_eye",
            "Good eye",
            "Earn trust with a bet that came true",
            50,
            flag(f.trust_ppm > 0),
        ),
        (
            "oracle",
            "Oracle",
            "Reach a trust score of 1",
            150,
            ratio(trust, 1_000_000),
        ),
        (
            "everywhere",
            "Everywhere",
            "Register three devices",
            50,
            ratio(f.devices, 3),
        ),
        (
            "reliable",
            "Reliable",
            "Reach the Reliable reputation tier",
            100,
            flag(f.tier >= Some(ReputationTier::R2)),
        ),
        (
            "pillar",
            "Pillar",
            "Reach the Pillar reputation tier",
            300,
            flag(f.tier >= Some(ReputationTier::R4)),
        ),
        (
            "month",
            "A month in the flock",
            "Be a member for 30 days",
            50,
            ratio(f.days, 30),
        ),
        (
            "year",
            "A year in the flock",
            "Be a member for 365 days",
            200,
            ratio(f.days, 365),
        ),
        (
            "chatterbox",
            "Chatterbox",
            "Write 25 messages in Gossip",
            25,
            ratio(f.messages, 25),
        ),
    ]
}

pub fn achievements(ledger: &Ledger, user: &str, messages: usize) -> Vec<Achievement> {
    let Some(facts) = facts(ledger, user, messages) else {
        return Vec::new();
    };
    rules(&facts)
        .into_iter()
        .map(|(id, name, description, points, progress)| Achievement {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            points,
            progress_percent: progress,
            earned: progress >= 100,
        })
        .collect()
}

pub fn level(achievements: &[Achievement]) -> Level {
    let points = achievements
        .iter()
        .filter(|a| a.earned)
        .map(|a| a.points)
        .sum();
    let number = LEVELS
        .iter()
        .rposition(|(_, floor)| points >= *floor)
        .unwrap_or(0);
    Level {
        number: number + 1,
        name: LEVELS[number].0.into(),
        points,
        floor: LEVELS[number].1,
        next: LEVELS.get(number + 1).map(|(_, floor)| *floor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{identity::Identity, membership::genesis};

    fn earned(list: &[Achievement]) -> Vec<&str> {
        list.iter()
            .filter(|a| a.earned)
            .map(|a| a.id.as_str())
            .collect()
    }

    #[test]
    fn a_founder_starts_with_one_badge_and_grows_with_what_the_ledger_records() {
        let founder = Identity::in_memory(None).unwrap();
        let (root, _) = genesis(&founder, "amici").unwrap();
        let mut ledger = Ledger::new(root, &founder.cert).unwrap();
        let me = founder.fingerprint.clone();

        let fresh = achievements(&ledger, &me, 0);
        assert_eq!(earned(&fresh), ["founder"]);
        let start = level(&fresh);
        assert_eq!((start.number, start.name.as_str()), (2, "Hatchling"));
        assert_eq!(
            (start.points, start.floor, start.next),
            (100, 50, Some(150))
        );

        ledger.time += 31 * EPOCH_SECS;
        ledger.trust.insert(me.clone(), 500_000);
        let member = ledger.members.get_mut(&me).unwrap();
        member.sent = 200;
        member.passed = 199;
        let later = achievements(&ledger, &me, 30);
        assert_eq!(
            earned(&later),
            ["founder", "rock_solid", "good_eye", "month", "chatterbox"]
        );
        let oracle = later.iter().find(|a| a.id == "oracle").unwrap();
        assert_eq!((oracle.progress_percent, oracle.earned), (50, false));
        assert_eq!(level(&later).name, "Fisher");
    }

    #[test]
    fn failing_challenges_never_reads_as_rock_solid_and_strangers_have_no_badges() {
        let founder = Identity::in_memory(None).unwrap();
        let (root, _) = genesis(&founder, "amici").unwrap();
        let mut ledger = Ledger::new(root, &founder.cert).unwrap();
        let me = founder.fingerprint.clone();
        let member = ledger.members.get_mut(&me).unwrap();
        member.sent = 1000;
        member.passed = 900;
        let list = achievements(&ledger, &me, 0);
        let solid = list.iter().find(|a| a.id == "rock_solid").unwrap();
        assert_eq!((solid.progress_percent, solid.earned), (99, false));
        assert!(achievements(&ledger, "SHA256:nobody", 0).is_empty());
        assert_eq!(level(&[]).name, "Egg");
    }
}

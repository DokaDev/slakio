//! A seeded, deterministic fake Slack world: two workspaces (`A company`, `B side`), sidebar
//! sections, about 300 channels, DMs and a group DM, a Slack Connect channel with people from
//! another organization, bots, unread and mention counts, muted channels, a 10,000-message
//! channel, a 1,200-reply thread, Korean and emoji text, and a channel of hostile strings.
//!
//! The demo backend of the UI and the fake Slack server of the tests both build on it, so the
//! demo, the screen snapshots and the network tests share one world. The same seed always gives
//! the same world. Messages are not stored: message `i` of a conversation is generated from
//! `(seed, conversation, i)` on demand, so a huge channel costs nothing until it is read.
//!
//! Everything is invented. Names hold only printable text; hostile strings appear only as
//! message text, which reaches the screen only through the sanitiser.

mod hostile;
mod rng;
mod text;

pub use hostile::{Hostile, all as hostile_strings};

use rng::Rng;
use slakio_core::backend::Snapshot;
use slakio_core::model::{
    Conversation, ConversationId, ConversationKind, Message, Org, Reaction, Section, SectionId, SectionKind,
    ThreadSummary, Ts, User, UserId, Workspace, WorkspaceColor, WorkspaceId,
};
use std::collections::HashMap;

/// The seed of the demo.
pub const DEMO_SEED: u64 = 0x51A4_10DE_0000_0001;

/// The conversations the demo and the tests rely on, by name (all in `A company`).
pub mod names {
    /// 10,000 messages.
    pub const BIG_HISTORY: &str = "big-history";
    /// Its first message has a 1,200-reply thread.
    pub const LONG_THREADS: &str = "long-threads";
    /// One message per hostile string.
    pub const HOSTILE: &str = "hostile-strings";
    /// Shared with another organization (Slack Connect).
    pub const SHARED: &str = "partner-shared";
}

pub const BIG_HISTORY_MESSAGES: usize = 10_000;
pub const LONG_THREAD_REPLIES: u32 = 1_200;

/// 2026-01-05 09:00 UTC: the first message of the world.
const START_SECS: u64 = 1_767_603_600;
/// Replies of a thread are this far apart.
const REPLY_STEP_SECS: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum History {
    Normal,
    Hostile,
    LongThread,
}

/// How a conversation's messages are generated.
#[derive(Clone, Debug)]
struct Plan {
    count: usize,
    history: History,
    posters: Vec<UserId>,
    /// Message `i` falls in `[start + i * step, start + (i + 1) * step)` seconds.
    start: u64,
    step: u64,
}

/// The world of one seed.
pub struct World {
    seed: u64,
    snapshot: Snapshot,
    me: HashMap<WorkspaceId, UserId>,
    plans: HashMap<ConversationId, Plan>,
}

/// Display names of invented people. Some are Korean, as escapes (no Hangul in source files).
const PEOPLE: &[(&str, &str)] = &[
    ("kim", "Kim"),
    ("park", "Park"),
    ("lee", "Lee"),
    ("minsu", "Minsu"),
    ("jiwon", "Jiwon"),
    ("seoyeon", "Seoyeon"),
    ("hyunwoo", "Hyunwoo"),
    ("alex", "Alex Morgan"),
    ("sam", "Sam Rivera"),
    ("jordan", "Jordan Lee"),
    ("taylor", "Taylor Kim"),
    ("chris", "Chris Park"),
    ("robin", "Robin Choi"),
    ("dana", "Dana Yoon"),
    ("eunji", "\u{C774}\u{C740}\u{C9C0}"),
    ("junho", "\u{BC15}\u{C900}\u{D638}"),
    ("minji", "\u{AE40}\u{BBFC}\u{C9C0}"),
    ("sora", "\u{CD5C}\u{C18C}\u{B77C}"),
    ("noah", "Noah Han"),
    ("mia", "Mia Jang"),
    ("leo", "Leo Kang"),
    ("ava", "Ava Shin"),
    ("ethan", "Ethan Song"),
    ("zoe", "Zoe Lim"),
    ("ryan", "Ryan Oh"),
    ("nina", "Nina Seo"),
    ("owen", "Owen Hwang"),
    ("lily", "Lily Jeon"),
    ("max", "Max Baek"),
    ("ivy", "Ivy Moon"),
    ("ben", "Ben Yu"),
    ("kate", "Kate Nam"),
    ("tom", "Tom Ko"),
    ("amy", "Amy Ryu"),
    ("dan", "Dan Cho"),
    ("eve", "Eve Bae"),
];

const PARTNER_ORG: &str = "Partner Inc";
const PARTNERS: &[(&str, &str)] = &[("p.lin", "Pat Lin"), ("q.ford", "Quinn Ford"), ("r.diaz", "Rae Diaz")];
const BOTS: &[(&str, &str)] = &[("grafana", "Grafana"), ("deploybot", "Deploy Bot")];
const REACTIONS: &[&str] = &["+1", "eyes", "tada", "white_check_mark", "pray", "fire"];
const PREFIXES: &[&str] = &["team", "proj", "help", "ops", "feed"];

/// What one workspace looks like.
struct Spec {
    id: &'static str,
    name: &'static str,
    people: usize,
    partners: bool,
    channels: usize,
    dms: usize,
    /// (section, kind, fixed channels in it).
    sections: &'static [(&'static str, SectionKind, &'static [&'static str])],
}

const SPECS: &[Spec] = &[
    Spec {
        id: "TDEMOA",
        name: "A company",
        people: 36,
        partners: true,
        channels: 240,
        dms: 12,
        sections: &[
            ("Favorites", SectionKind::Favorites, &["backend", "incidents"]),
            (
                "Ops",
                SectionKind::Custom,
                &["deploys", "alerts", names::SHARED, names::HOSTILE, names::BIG_HISTORY, names::LONG_THREADS],
            ),
            ("Channels", SectionKind::Channels, &["general", "random", "announcements", "frontend", "design-review"]),
            ("Direct messages", SectionKind::DirectMessages, &[]),
        ],
    },
    Spec {
        id: "TDEMOB",
        name: "B side",
        people: 12,
        partners: false,
        channels: 60,
        dms: 5,
        sections: &[
            ("Channels", SectionKind::Channels, &["general", "random", "side-project", "ideas"]),
            ("Direct messages", SectionKind::DirectMessages, &[]),
        ],
    },
];

impl World {
    pub fn new(seed: u64) -> Self {
        let mut w = World { seed, snapshot: Snapshot::default(), me: HashMap::new(), plans: HashMap::new() };
        for (n, spec) in SPECS.iter().enumerate() {
            w.add_workspace(n, spec);
        }
        w
    }

    /// The demo world.
    pub fn demo() -> Self {
        Self::new(DEMO_SEED)
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Workspaces, people, sections and conversations.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The user's own account in `workspace`.
    pub fn me(&self, workspace: &WorkspaceId) -> Option<&UserId> {
        self.me.get(workspace)
    }

    /// The conversation of `workspace` named `name` (channels by name, DMs by peer).
    pub fn find(&self, workspace: &str, name: &str) -> Option<&Conversation> {
        self.snapshot.conversations.iter().find(|c| c.workspace.as_str() == workspace && c.name == name)
    }

    /// How many messages `conversation` has (0 when unknown).
    pub fn message_count(&self, conversation: &ConversationId) -> usize {
        self.plans.get(conversation).map_or(0, |p| p.count)
    }

    /// Message `index` of `conversation`, oldest first.
    pub fn message(&self, conversation: &ConversationId, index: usize) -> Option<Message> {
        let plan = self.plans.get(conversation)?;
        if index >= plan.count {
            return None;
        }
        let mut r = Rng::keyed(self.seed, conversation.as_str(), index as u64);
        let slot = plan.start + index as u64 * plan.step;
        let ts = Ts(slot * 1_000_000 + r.below(plan.step * 1_000_000));
        let user = r.pick(&plan.posters).clone();
        let text = match plan.history {
            History::Hostile => hostile::all().swap_remove(index).text,
            _ => text::message(&mut r),
        };
        let replies = match plan.history {
            History::LongThread if index == 0 => LONG_THREAD_REPLIES,
            History::Hostile => 0,
            _ if r.chance(8) => 1 + r.below(12) as u32,
            _ => 0,
        };
        let thread = (replies > 0).then(|| ThreadSummary {
            replies,
            last_reply: Ts(ts.0 + u64::from(replies) * REPLY_STEP_SECS * 1_000_000),
        });
        let mut reactions: Vec<Reaction> = Vec::new();
        if r.chance(15) {
            for _ in 0..1 + r.below(3) {
                let name = r.pick(REACTIONS).to_string();
                if !reactions.iter().any(|x| x.name == name) {
                    reactions.push(Reaction { name, count: 1 + r.below(5) as u32, mine: r.chance(30) });
                }
            }
        }
        Some(Message { ts, user, text, thread, reactions, edited: r.chance(5) })
    }

    /// Reply `index` of the thread under message `parent` of `conversation`, oldest first.
    pub fn reply(&self, conversation: &ConversationId, parent: usize, index: u32) -> Option<Message> {
        let plan = self.plans.get(conversation)?;
        let root = self.message(conversation, parent)?;
        if index >= root.thread?.replies {
            return None;
        }
        let mut r = Rng::keyed(self.seed, &format!("{conversation}/{parent}"), u64::from(index));
        let ts = Ts(root.ts.0 + (u64::from(index) + 1) * REPLY_STEP_SECS * 1_000_000);
        let user = r.pick(&plan.posters).clone();
        Some(Message { ts, user, text: text::message(&mut r), thread: None, reactions: Vec::new(), edited: false })
    }

    fn add_workspace(&mut self, n: usize, spec: &Spec) {
        let ws = WorkspaceId::new(spec.id);
        self.snapshot.workspaces.push(Workspace {
            id: ws.clone(),
            name: spec.name.to_string(),
            color: WorkspaceColor(n as u8),
        });
        let user = |id: String, name: &str, display: &str, org: Org, bot: bool| User {
            id: UserId::new(id),
            workspace: ws.clone(),
            name: name.to_string(),
            display_name: display.to_string(),
            org,
            bot,
        };
        let tag = &spec.id[5..];
        let me = user(format!("UDEMO{tag}000"), "me", "Me", Org::Own, false);
        self.me.insert(ws.clone(), me.id.clone());
        let mut people = vec![];
        for (i, (name, display)) in PEOPLE.iter().take(spec.people).enumerate() {
            people.push(user(format!("UDEMO{tag}{:03}", i + 1), name, display, Org::Own, false));
        }
        let bots: Vec<User> = BOTS
            .iter()
            .enumerate()
            .map(|(i, (name, display))| user(format!("BDEMO{tag}{:03}", i + 1), name, display, Org::Own, true))
            .collect();
        let partners: Vec<User> = if spec.partners {
            PARTNERS
                .iter()
                .enumerate()
                .map(|(i, (name, display))| {
                    user(format!("UDEMOX{:03}", i + 1), name, display, Org::External(PARTNER_ORG.into()), false)
                })
                .collect()
        } else {
            vec![]
        };
        let own: Vec<UserId> = people.iter().map(|u| u.id.clone()).collect();
        let with_bots: Vec<UserId> = own.iter().chain(bots.iter().map(|b| &b.id)).cloned().collect();
        let shared: Vec<UserId> = own.iter().take(4).chain(partners.iter().map(|p| &p.id)).cloned().collect();

        let mut r = Rng::keyed(self.seed, spec.id, 0);
        let fixed: Vec<&str> = spec.sections.iter().flat_map(|s| s.2.iter().copied()).collect();
        let mut generated: Vec<String> = Vec::new();
        while fixed.len() + generated.len() < spec.channels {
            let name = format!("{}-{}-{}", r.pick(PREFIXES), text::topic(&mut r), generated.len() + 1);
            generated.push(name);
        }
        generated.sort();
        for (s, (name, kind, channels)) in spec.sections.iter().enumerate() {
            let section = SectionId::new(format!("SDEMO{tag}{s}"));
            self.snapshot.sections.push(Section {
                id: section.clone(),
                workspace: ws.clone(),
                name: name.to_string(),
                kind: *kind,
            });
            let mut names: Vec<String> = channels.iter().map(|c| c.to_string()).collect();
            if *kind == SectionKind::Channels {
                names.extend(generated.iter().cloned());
                names.sort();
            }
            for name in names {
                let posters = match name.as_str() {
                    names::SHARED => shared.clone(),
                    "alerts" | "incidents" | "deploys" => with_bots.clone(),
                    _ => own.clone(),
                };
                let private = name != names::SHARED && r.chance(10);
                let kind = ConversationKind::Channel { private };
                let id = ConversationId::new(format!("CDEMO{tag}{:04}", self.snapshot.conversations.len()));
                self.add_conversation(&ws, id, kind, name, &section, posters);
            }
            if *kind == SectionKind::DirectMessages {
                for (i, peer) in people.iter().take(spec.dms).enumerate() {
                    let id = ConversationId::new(format!("DDEMO{tag}{:04}", i + 1));
                    let kind = ConversationKind::Dm { user: peer.id.clone() };
                    let posters = vec![me.id.clone(), peer.id.clone()];
                    self.add_conversation(&ws, id, kind, peer.display_name.clone(), &section, posters);
                }
                let group: Vec<&User> = people.iter().skip(1).take(3).collect();
                let name = group.iter().map(|u| u.display_name.as_str()).collect::<Vec<_>>().join(", ");
                let users: Vec<UserId> = group.iter().map(|u| u.id.clone()).collect();
                let posters = std::iter::once(me.id.clone()).chain(users.iter().cloned()).collect();
                let id = ConversationId::new(format!("GDEMO{tag}0001"));
                self.add_conversation(&ws, id, ConversationKind::GroupDm { users }, name, &section, posters);
            }
        }
        self.snapshot.users.push(me);
        self.snapshot.users.extend(people);
        self.snapshot.users.extend(bots);
        self.snapshot.users.extend(partners);
    }

    fn add_conversation(
        &mut self,
        ws: &WorkspaceId,
        id: ConversationId,
        kind: ConversationKind,
        name: String,
        section: &SectionId,
        posters: Vec<UserId>,
    ) {
        let mut r = Rng::keyed(self.seed, id.as_str(), u64::MAX);
        let (history, count) = match name.as_str() {
            names::BIG_HISTORY => (History::Normal, BIG_HISTORY_MESSAGES),
            names::LONG_THREADS => (History::LongThread, 40),
            names::HOSTILE => (History::Hostile, hostile::all().len()),
            _ => (History::Normal, 20 + r.below(180) as usize),
        };
        let step = 60 + r.below(3600);
        let start = START_SECS + r.below(86_400);
        let dm = !matches!(kind, ConversationKind::Channel { .. });
        let mut unread = match (dm, r.chance(25)) {
            (_, false) => 0,
            (true, true) => 1 + r.below(4) as u32,
            (false, true) => 1 + r.below(20) as u32,
        };
        if history == History::Hostile {
            unread = count as u32;
        }
        let mentions = if unread > 0 && r.chance(30) { 1 + r.below(u64::from(unread.min(3))) as u32 } else { 0 };
        let external = name == names::SHARED;
        let muted = !external && history == History::Normal && r.chance(10);
        self.snapshot.conversations.push(Conversation {
            id: id.clone(),
            workspace: ws.clone(),
            kind,
            name,
            section: section.clone(),
            external,
            muted,
            unread,
            mentions,
        });
        self.plans.insert(id, Plan { count, history, posters, start, step });
    }
}

#[cfg(test)]
mod tests;

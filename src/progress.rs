//! Your progression: XP from closed GitLab issues, levels, loot and gear, kept in
//! `~/.local/state/claude-hub/progress.json`. Everything here works offline from that file;
//! `xp.rs` fetches issues and hands them to `Progress::count_closed`.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::DropChances;
use crate::gear::Slot;
use crate::loot::{self, Item};

/// 2: random loot with generic names and stats from an item-level budget.
/// 3: the quality words Good, Excellent, Masterwork for Uncommon, Rare, Epic.
/// 4: Common items toned down (multiplier 1.4 → 1.2).
/// 5: flatter scaling: Poor is item level + 1, the other rarities multiply that.
/// 6: three hand-written legendaries per slot, which grow with your level.
/// 7: merged MRs and reviews give XP; a gentler level curve.
/// 8: gold and the store (no change to old data).
pub const VERSION: u32 = 8;


/// The store's daily stock, worst first: five Common, three Uncommon, two Rare and one Epic.
pub const STOCK: [(loot::Rarity, usize); 4] = [(loot::Rarity::Common, 5), (loot::Rarity::Uncommon, 3), (loot::Rarity::Rare, 2), (loot::Rarity::Epic, 1)];

// ---------------------------------------------------------------- the level curve

/// XP needed to go from `level` to the next one. The one place to tune how steep levelling is:
/// 50 for level 2, about 1000 at level 10, and a little steeper with every level.
pub fn xp_to_next(level: u32) -> u64 {
    (50.0 * (level.max(1) as f64).powf(1.3)).round() as u64
}

/// The level for this much XP, how far into that level it is, and how much the level needs.
pub fn level_for(total_xp: u64) -> (u32, u64, u64) {
    let (mut level, mut left) = (1u32, total_xp);
    loop {
        let need = xp_to_next(level);
        if left < need {
            return (level, left, need);
        }
        left -= need;
        level += 1;
    }
}

// ---------------------------------------------------------------- the model

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Progress {
    pub version: u32,
    /// The GitLab user the loot is seeded from. Set on the first sync and never changed after.
    pub username: Option<String>,
    /// When this file was made, in Unix seconds: the start for `xp_since = "install"`.
    pub started_at: u64,
    pub sync: SyncState,
    /// Every issue that gave XP, by GitLab's global issue id. An id here never counts again.
    pub issues: BTreeMap<u64, Counted>,
    /// Every merged MR that gave XP, by role and GitLab's global MR id ("mr:123", "review:123").
    pub mrs: BTreeMap<String, Counted>,
    /// Open issues assigned to you, as of the last sync.
    pub quests: Vec<Quest>,
    pub total_xp: u64,
    pub level: u32,
    /// Every item you own, equipped or in the bag, in drop order.
    pub items: Vec<Item>,
    /// Slot → item id. Items not here are in the bag.
    pub equipped: BTreeMap<Slot, String>,
    /// None until you make your character.
    pub appearance: Option<Appearance>,
    /// The town has shown level-ups up to this level.
    pub celebrated_level: u32,
    /// The town has shown "quest complete" for issues counted up to this (`Counted::seq`).
    pub acked_seq: u64,
    /// Gold paid in the store, and gold got for selling there.
    pub gold_spent: u64,
    pub gold_from_sales: u64,
    pub store: Stock,
}

/// What the store sells today.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Stock {
    /// The local date it was stocked ("2026-11-19"); a new date restocks it.
    pub day: String,
    pub items: Vec<Item>,
}

#[derive(Debug, PartialEq)]
pub enum TradeError {
    NotInStore,
    NotInBag,
    Worn,
    TooPoor { price: u64, gold: u64 },
}

impl std::fmt::Display for TradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TradeError::NotInStore => write!(f, "The store does not have that any more."),
            TradeError::NotInBag => write!(f, "You do not have that item."),
            TradeError::Worn => write!(f, "Take it off before you sell it."),
            TradeError::TooPoor { price, gold } => write!(f, "That costs {price} gold; you have {gold}."),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SyncState {
    pub last_ok: Option<u64>,
    pub last_attempt: Option<u64>,
    /// Fetch closed issues updated after this (Unix seconds) next time.
    pub cursor: Option<u64>,
    /// The `xp_since` start the cursor was made for; a different start fetches everything again.
    pub cursor_since: Option<u64>,
    pub failures: u32,
    pub last_error: Option<String>,
    /// Like `cursor`, for merged MRs.
    pub mr_cursor: Option<u64>,
    pub mr_cursor_since: Option<u64>,
}

/// An issue that gave XP, as it was when it first counted.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Counted {
    pub project_id: u64,
    /// "group/project".
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub url: String,
    /// Its weight in GitLab, if it had one.
    pub weight: Option<u64>,
    pub closed_at: u64,
    pub counted_at: u64,
    pub xp: u64,
    /// Counting order, from 1.
    pub seq: u64,
    /// Its labels when it counted; None in files from before labels were kept (the next sync fills them).
    pub labels: Option<Vec<String>>,
    /// None for an issue; "mr" for a merged MR you were assigned to, "review" for one you reviewed.
    pub kind: Option<String>,
    /// Gold it gave (closed issues only). None for issues counted before gold existed; the next sync fills it.
    pub gold: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Quest {
    pub id: u64,
    pub project_id: u64,
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub url: String,
    pub weight: Option<u64>,
    /// What closing it would give with today's settings.
    pub xp: u64,
    pub due_date: Option<String>,
    pub labels: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Appearance {
    pub name: String,
    pub skin: u8,
    pub hair_style: u8,
    pub hair_color: u8,
    pub outfit: u8,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance { name: "You".into(), skin: 1, hair_style: 0, hair_color: 0, outfit: 0 }
    }
}

impl Appearance {
    /// Keep every choice inside the palettes the town draws from.
    pub fn clamped(mut self) -> Self {
        self.skin %= 6;
        self.hair_style %= 5;
        self.hair_color %= 8;
        self.outfit %= 8;
        self.name = self.name.chars().filter(|c| !c.is_control()).take(24).collect::<String>().trim().to_string();
        if self.name.is_empty() {
            self.name = "You".into();
        }
        self
    }
}

/// An issue as GitLab reports it, the part counting needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Issue {
    pub id: u64,
    pub project_id: u64,
    pub project: String,
    pub iid: u64,
    pub title: String,
    pub url: String,
    pub weight: Option<u64>,
    pub closed_at: Option<u64>,
    pub updated_at: Option<u64>,
    /// Moved to another project: GitLab closes the original, and the copy counts when it closes.
    pub moved: bool,
    /// Usernames, when GitLab sent them.
    pub assignees: Option<Vec<String>>,
    pub due_date: Option<String>,
    pub labels: Vec<String>,
}

/// How XP is counted; from the config, with `since` resolved to Unix seconds.
#[derive(Clone, Debug)]
pub struct Rules {
    /// Whose assignment counts.
    pub username: String,
    pub xp_per_weight: u64,
    pub default_weight: u64,
    /// Only issues closed at or after this count.
    pub since: u64,
    pub chances: DropChances,
    /// Flat XP for a merged MR you were assigned to, and for one you reviewed.
    pub xp_per_merged_mr: u64,
    pub xp_per_review: u64,
    /// Gold for every closed issue.
    pub gold_per_issue: u64,
}

impl Rules {
    pub fn xp(&self, weight: Option<u64>) -> u64 {
        weight.unwrap_or(self.default_weight) * self.xp_per_weight
    }
}

/// What one batch of closed issues did.
#[derive(Debug, Default, PartialEq)]
pub struct Gains {
    pub issues: Vec<u64>,
    /// "mr:123", "review:123".
    pub mrs: Vec<String>,
    pub xp: u64,
    pub levels: Vec<u32>,
}

/// Which merged MRs: those you were assigned to, or those you reviewed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MrRole {
    Assignee,
    Reviewer,
}

impl MrRole {
    pub fn key(self) -> &'static str {
        match self {
            MrRole::Assignee => "mr",
            MrRole::Reviewer => "review",
        }
    }
}

impl Progress {
    pub fn new(now: u64) -> Self {
        Progress { version: VERSION, started_at: now, level: 1, celebrated_level: 1, ..Default::default() }
    }

    /// Count the closed issues that give XP and drop one item per level reached.
    ///
    /// An issue counts once, by global id: reopening and closing it again, changing its weight or
    /// unassigning you afterwards changes nothing. Moved issues, issues you are not assigned to,
    /// and issues closed before `rules.since` never count.
    pub fn count_closed(&mut self, issues: &[Issue], rules: &Rules, now: u64) -> Gains {
        self.username.get_or_insert_with(|| rules.username.clone());
        // Issues counted before gold existed earn it now, once.
        for c in self.issues.values_mut().filter(|c| c.gold.is_none()) {
            c.gold = Some(rules.gold_per_issue);
        }
        // Issues counted before labels were kept get their labels now; nothing else about them changes.
        for i in issues {
            if let Some(c) = self.issues.get_mut(&i.id).filter(|c| c.labels.is_none()) {
                c.labels = Some(i.labels.clone());
            }
        }
        let mut fresh: Vec<&Issue> = issues
            .iter()
            .filter(|i| !self.issues.contains_key(&i.id))
            .filter(|i| !i.moved)
            .filter(|i| i.assignees.as_ref().is_none_or(|a| a.iter().any(|u| u.eq_ignore_ascii_case(&rules.username))))
            .filter(|i| i.closed_at.is_some_and(|c| c >= rules.since))
            .collect();
        // Oldest first, so the issue that crossed a level is the one that gets credit for its loot.
        fresh.sort_by_key(|i| (i.closed_at, i.id));
        fresh.dedup_by_key(|i| i.id);
        let mut gains = Gains::default();
        for i in fresh {
            let xp = rules.xp(i.weight);
            let seq = self.next_seq();
            self.issues.insert(
                i.id,
                Counted {
                    project_id: i.project_id,
                    project: i.project.clone(),
                    iid: i.iid,
                    title: i.title.clone(),
                    url: i.url.clone(),
                    weight: i.weight,
                    closed_at: i.closed_at.unwrap_or(now),
                    counted_at: now,
                    xp,
                    seq,
                    labels: Some(i.labels.clone()),
                    kind: None,
                    gold: Some(rules.gold_per_issue),
                },
            );
            gains.issues.push(i.id);
            let trigger = self.issues.get(&i.id).map(trigger_of);
            self.add_xp(xp, trigger, &mut gains, &rules.chances);
        }
        self.scale_legendaries();
        gains
    }

    /// Count merged MRs: flat XP for each you were assigned to (or reviewed), once per MR and role,
    /// merged at or after `rules.since`. In `mrs`, `closed_at` is when it was merged and `assignees`
    /// the assignees (or reviewers) GitLab listed.
    pub fn count_merged(&mut self, mrs: &[Issue], role: MrRole, rules: &Rules, now: u64) -> Gains {
        self.username.get_or_insert_with(|| rules.username.clone());
        let xp = match role {
            MrRole::Assignee => rules.xp_per_merged_mr,
            MrRole::Reviewer => rules.xp_per_review,
        };
        let key = |id: u64| format!("{}:{id}", role.key());
        let mut fresh: Vec<&Issue> = mrs
            .iter()
            .filter(|m| !self.mrs.contains_key(&key(m.id)))
            .filter(|m| m.assignees.as_ref().is_none_or(|a| a.iter().any(|u| u.eq_ignore_ascii_case(&rules.username))))
            .filter(|m| m.closed_at.is_some_and(|c| c >= rules.since))
            .collect();
        fresh.sort_by_key(|m| (m.closed_at, m.id));
        fresh.dedup_by_key(|m| m.id);
        let mut gains = Gains::default();
        for m in fresh {
            let seq = self.next_seq();
            let c = Counted {
                project_id: m.project_id,
                project: m.project.clone(),
                iid: m.iid,
                title: m.title.clone(),
                url: m.url.clone(),
                weight: None,
                closed_at: m.closed_at.unwrap_or(now),
                counted_at: now,
                xp,
                seq,
                labels: None,
                kind: Some(role.key().into()),
                gold: None,
            };
            let trigger = Some(trigger_of(&c));
            self.mrs.insert(key(m.id), c);
            gains.mrs.push(key(m.id));
            self.add_xp(xp, trigger, &mut gains, &rules.chances);
        }
        self.scale_legendaries();
        gains
    }

    fn next_seq(&self) -> u64 {
        self.issues.values().chain(self.mrs.values()).map(|c| c.seq).max().unwrap_or(0) + 1
    }

    /// Add XP and level up as far as it reaches, one loot drop per level.
    fn add_xp(&mut self, xp: u64, trigger: Option<loot::Trigger>, gains: &mut Gains, chances: &DropChances) {
        self.total_xp += xp;
        gains.xp += xp;
        self.catch_up(trigger, gains, chances);
    }

    /// Reach the level your XP is worth (after a gentler curve, too), with a drop for each new level.
    pub fn catch_up(&mut self, trigger: Option<loot::Trigger>, gains: &mut Gains, chances: &DropChances) {
        let (level, _, _) = level_for(self.total_xp);
        while self.level.max(1) < level {
            self.level = self.level.max(1) + 1;
            gains.levels.push(self.level);
            self.drop_loot(self.level, trigger.clone(), chances);
        }
        self.scale_legendaries();
    }

    /// The most recent thing that gave XP, as a loot trigger.
    pub fn latest_trigger(&self) -> Option<loot::Trigger> {
        self.issues.values().chain(self.mrs.values()).max_by_key(|c| c.seq).map(trigger_of)
    }

    /// A random item for reaching `level`. Rolled once and stored, so it stays what it was.
    fn drop_loot(&mut self, level: u32, trigger: Option<loot::Trigger>, chances: &DropChances) {
        if self.items.iter().any(|it| it.dropped_at_level == level && !it.bought) {
            return;
        }
        let taken: Vec<String> = self.items.iter().filter(|i| i.rarity == loot::Rarity::Legendary).map(|i| i.name.clone()).collect();
        let mut item = loot::generate(loot::random_seed(), level, chances, &taken);
        item.trigger = trigger;
        self.items.push(item);
    }

    /// The open issues assigned to you become the quest log.
    pub fn set_quests(&mut self, open: &[Issue], rules: &Rules) {
        self.quests = open
            .iter()
            .filter(|i| !i.moved && !self.issues.contains_key(&i.id))
            .map(|i| Quest {
                id: i.id,
                project_id: i.project_id,
                project: i.project.clone(),
                iid: i.iid,
                title: i.title.clone(),
                url: i.url.clone(),
                weight: i.weight,
                xp: rules.xp(i.weight),
                due_date: i.due_date.clone(),
                labels: i.labels.clone(),
            })
            .collect();
        // Quests with a due date first, soonest first; then the biggest reward.
        self.quests.sort_by(|a, b| (a.due_date.is_none(), &a.due_date, std::cmp::Reverse(a.xp)).cmp(&(b.due_date.is_none(), &b.due_date, std::cmp::Reverse(b.xp))));
    }

    /// The issues that gave XP, newest first.
    pub fn recent(&self, n: usize) -> Vec<&Counted> {
        let mut v: Vec<&Counted> = self.issues.values().chain(self.mrs.values()).collect();
        v.sort_by_key(|c| std::cmp::Reverse(c.seq));
        v.truncate(n);
        v
    }

    /// Legendaries grow with you: item level = your level, stats = the legendary budget for it,
    /// in the proportions they dropped with. So does the store's stock: it sells at your item level.
    pub fn scale_legendaries(&mut self) {
        let level = self.level.max(1);
        for it in self.items.iter_mut().filter(|it| it.rarity == loot::Rarity::Legendary).chain(self.store.items.iter_mut()) {
            if it.item_level != level || it.stats.values().sum::<u32>() != loot::stat_budget(level, it.rarity) {
                it.item_level = level;
                it.stats = loot::rescale(&it.stats, loot::stat_budget(level, it.rarity));
            }
        }
    }

    /// Your gold: what closed issues gave, plus sales, minus what you spent.
    pub fn gold(&self) -> u64 {
        let earned: u64 = self.issues.values().filter_map(|c| c.gold).sum();
        (earned + self.gold_from_sales).saturating_sub(self.gold_spent)
    }

    /// A fresh stock when the day has changed: five Common, three Uncommon, two Rare and one Epic, each
    /// for a random slot, at your level. True if it restocked.
    pub fn restock(&mut self, today: &str) -> bool {
        if self.store.day == today {
            return false;
        }
        let level = self.level.max(1);
        let mut items = vec![];
        for (rarity, n) in STOCK {
            let only = DropChances {
                poor: 0.0,
                common: (rarity == loot::Rarity::Common) as u8 as f64,
                uncommon: (rarity == loot::Rarity::Uncommon) as u8 as f64,
                rare: (rarity == loot::Rarity::Rare) as u8 as f64,
                epic: (rarity == loot::Rarity::Epic) as u8 as f64,
                legendary: 0.0,
            };
            for _ in 0..n {
                let mut it = loot::generate(loot::random_seed(), level, &only, &[]);
                it.id = format!("store-{today}-{}", items.len());
                it.bought = true;
                items.push(it);
            }
        }
        self.store = Stock { day: today.to_string(), items };
        true
    }

    /// Buy an item from today's stock: it goes into your bag. Returns its new id.
    pub fn buy(&mut self, store_id: &str, now: u64) -> Result<String, TradeError> {
        let i = self.store.items.iter().position(|it| it.id == store_id).ok_or(TradeError::NotInStore)?;
        let price = self.store.items[i].price();
        let gold = self.gold();
        if price > gold {
            return Err(TradeError::TooPoor { price, gold });
        }
        let mut it = self.store.items.remove(i);
        let n = self.items.iter().filter(|x| x.bought).count();
        it.id = format!("bought-{now}-{n}");
        it.dropped_at_level = self.level.max(1);
        it.trigger = None;
        it.bought = true;
        self.gold_spent += price;
        let id = it.id.clone();
        self.items.push(it);
        Ok(id)
    }

    /// Sell an item from your bag (not one you wear) for a quarter of its price.
    pub fn sell(&mut self, item_id: &str) -> Result<u64, TradeError> {
        if self.equipped.values().any(|id| id == item_id) {
            return Err(TradeError::Worn);
        }
        let i = self.items.iter().position(|it| it.id == item_id).ok_or(TradeError::NotInBag)?;
        let gold = self.items[i].sell_price();
        self.items.remove(i);
        self.gold_from_sales += gold;
        Ok(gold)
    }

    /// Bring a file from an older version up to date. True if anything changed.
    ///
    /// Version 2: items from before keep their kind, rarity, item level and whether they are worn,
    /// but get the generic name ("Broken Mousepad") and stats rolled from the item-level budget.
    /// Version 3: generic names follow the current quality words; stats stay.
    /// Versions 4 and 5: every item is scaled to the current budget (lower Common, flatter scaling).
    /// Version 6: legendaries not on today's list become one that is, for the same slot.
    /// Legendaries keep their names.
    pub fn migrate(&mut self) -> bool {
        if self.version >= VERSION {
            return false;
        }
        if self.version < 2 {
            let mut rng = loot::Rng::new(loot::random_seed());
            for it in &mut self.items {
                it.stats = loot::roll_stats(&mut rng, it.item_level, it.rarity);
            }
        }
        for it in self.items.iter_mut().filter(|it| it.rarity != loot::Rarity::Legendary) {
            it.name = loot::generic_name(it.kind, it.base(), it.rarity);
            it.flavor = None;
        }
        // Versions 4 and 5 changed the budgets: every item rolled before comes to today's total, same proportions.
        if (2..5).contains(&self.version) {
            for it in &mut self.items {
                it.stats = loot::rescale(&it.stats, loot::stat_budget(it.item_level, it.rarity));
            }
        }
        // Version 6: a legendary from the old list becomes an unowned one for its slot, with its lore.
        if self.version < 6 {
            let owned: Vec<String> = self.items.iter().filter(|i| loot::legendary(&i.name).is_some()).map(|i| i.name.clone()).collect();
            let mut used = owned;
            for it in self.items.iter_mut().filter(|it| it.rarity == loot::Rarity::Legendary && loot::legendary(&it.name).is_none()) {
                let slot = it.kind.slot();
                if let Some((name, kind, aura, flavor)) = loot::LEGENDARIES.iter().find(|l| l.1.slot() == slot && !used.iter().any(|u| u == l.0)) {
                    it.name = name.to_string();
                    it.kind = *kind;
                    it.aura = Some(*aura);
                    it.flavor = Some(flavor.to_string());
                    used.push(name.to_string());
                } else {
                    // Every legendary for the slot is owned already: it becomes an Epic of its kind.
                    it.rarity = loot::Rarity::Epic;
                    it.aura = None;
                    it.flavor = None;
                    it.name = loot::generic_name(it.kind, it.base(), it.rarity);
                    it.stats = loot::rescale(&it.stats, loot::stat_budget(it.item_level, it.rarity));
                }
            }
            // Lore for the ones that kept their names.
            for it in self.items.iter_mut() {
                if let Some(l) = loot::legendary(&it.name) {
                    it.flavor = Some(l.3.to_string());
                }
            }
        }
        self.scale_legendaries();
        self.version = VERSION;
        true
    }

    /// Counted issues whose labels are not known yet.
    pub fn missing_labels(&self) -> bool {
        self.issues.values().any(|c| c.labels.is_none())
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
}

fn trigger_of(c: &Counted) -> loot::Trigger {
    loot::Trigger { project: c.project.clone(), iid: c.iid, title: c.title.clone(), kind: c.kind.clone() }
}

// ---------------------------------------------------------------- the file

/// Where progress lives. One writer at a time: every change takes a lock, reads the file again,
/// changes it and writes it back, so the hub, the town and `claude-hub xp sync` never undo each other.
#[derive(Clone)]
pub struct Store {
    pub path: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Store { path: crate::config::state_dir().join("progress.json") }
    }
}

impl Store {
    fn backup(&self) -> PathBuf {
        self.path.with_extension("json.bak")
    }

    fn read(path: &std::path::Path) -> std::io::Result<Option<Progress>> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The saved progress, or None when there is none yet. A damaged file falls back to the backup.
    pub fn load(&self) -> Option<Progress> {
        let p = Self::read(&self.path).ok().flatten().or_else(|| Self::read(&self.backup()).ok().flatten())?;
        if p.version < VERSION {
            // Migrate once, under the lock, so its random rolls are saved and every reader sees the same.
            if self.update(|_| ()).is_ok() {
                return Self::read(&self.path).ok().flatten();
            }
        }
        Some(p)
    }

    pub fn mtime(&self) -> Option<std::time::SystemTime> {
        std::fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }

    /// Change the progress under the lock, making it first if needed. Saves only if `f` changed it.
    ///
    /// Progress outlives every session, so saving is careful: the new file is flushed to disk before
    /// it replaces the old one, the old one is kept as `progress.json.bak`, and a damaged file is set
    /// aside as `progress.json.damaged` and replaced by the backup, never by an empty start.
    pub fn update<R>(&self, f: impl FnOnce(&mut Progress) -> R) -> std::io::Result<R> {
        let dir = self.path.parent().map(PathBuf::from).unwrap_or_else(|| ".".into());
        std::fs::create_dir_all(&dir)?;
        let lock = File::options().create(true).truncate(false).write(true).open(self.path.with_extension("lock"))?;
        lock.lock()?;
        let before = match Self::read(&self.path) {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => match Self::read(&self.backup()) {
                Ok(Some(b)) => {
                    std::fs::rename(&self.path, self.path.with_extension("json.damaged"))?;
                    Some(b)
                }
                // Neither can be read: leave both alone, it holds XP and loot.
                _ => return Err(e),
            },
            Err(e) => return Err(e),
        };
        let mut p = before.clone().unwrap_or_else(|| Progress::new(crate::state::now()));
        p.migrate();
        let out = f(&mut p);
        if before.as_ref() != Some(&p) || !self.path.exists() {
            let tmp = self.path.with_extension("tmp");
            {
                let mut file = File::create(&tmp)?;
                std::io::Write::write_all(&mut file, &serde_json::to_vec_pretty(&p)?)?;
                file.sync_all()?;
            }
            if self.path.exists() {
                std::fs::copy(&self.path, self.backup())?;
            }
            std::fs::rename(&tmp, &self.path)?;
            // The rename itself survives a crash only once the directory is on disk.
            if let Ok(d) = File::open(&dir) {
                let _ = d.sync_all();
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- time

/// Today's local date ("2026-11-19"), when the store restocks. From `date`, since std has no time zones.
pub fn local_today() -> String {
    std::process::Command::new("date")
        .arg("+%F")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|d| d.len() == 10)
        .unwrap_or_else(|| format_time(crate::state::now())[..10].to_string())
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Unix seconds from GitLab's timestamps ("2026-11-10T08:12:33.123Z", "...+01:00") or a date ("2026-01-01").
pub fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.as_bytes().get(4) != Some(&b'-') || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, mo as u32, d as u32) * 86400;
    if s.len() > 10 {
        secs += num(11, 13)? * 3600 + num(14, 16)? * 60 + num(17, 19)?;
        let rest = &s[19..];
        let zone = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
        if let Some(sign @ ('+' | '-')) = zone.chars().next() {
            let off = zone[1..3].parse::<i64>().ok()? * 3600 + zone.get(4..6).and_then(|m| m.parse::<i64>().ok()).unwrap_or(0) * 60;
            secs -= if sign == '+' { off } else { -off };
        }
    }
    u64::try_from(secs).ok()
}

/// "2026-11-10T08:12:33Z".
pub fn format_time(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn rules() -> Rules {
        Rules { username: "osbo".into(), xp_per_weight: 10, default_weight: 1, since: 0, chances: DropChances::default(), xp_per_merged_mr: 25, xp_per_review: 10, gold_per_issue: 25 }
    }

    pub fn issue(id: u64, weight: Option<u64>, closed: &str) -> Issue {
        Issue {
            id,
            project_id: 7,
            project: "group/app".into(),
            iid: id % 1000,
            title: format!("Issue {id}"),
            url: format!("https://git.example.com/group/app/-/issues/{}", id % 1000),
            weight,
            closed_at: parse_time(closed),
            updated_at: parse_time(closed),
            assignees: Some(vec!["osbo".into()]),
            ..Default::default()
        }
    }

    #[test]
    fn level_curve_gets_steeper() {
        assert_eq!(xp_to_next(1), 50);
        assert_eq!(xp_to_next(2), 123);
        assert_eq!(xp_to_next(3), 209);
        assert_eq!(xp_to_next(10), 998);
        for n in 1..80 {
            assert!(xp_to_next(n + 1) > xp_to_next(n), "level {n}");
            // Steeper than linear: twice the level needs more than twice the XP.
            assert!(xp_to_next(2 * n) > 2 * xp_to_next(n), "level {n}");
        }
        assert_eq!(level_for(0), (1, 0, 50));
        assert_eq!(level_for(49), (1, 49, 50));
        assert_eq!(level_for(50), (2, 0, 123));
        assert_eq!(level_for(50 + 123 + 5), (3, 5, 209));
        // About a year and a half of issues, merged MRs and reviews (8880 XP) is level 14.
        assert_eq!(level_for(8880).0, 14);
    }

    #[test]
    fn xp_is_weight_times_multiplier_with_a_default_weight() {
        let mut r = rules();
        assert_eq!(r.xp(Some(3)), 30);
        assert_eq!(r.xp(None), 10);
        r.default_weight = 2;
        r.xp_per_weight = 25;
        assert_eq!(r.xp(None), 50);
        assert_eq!(r.xp(Some(0)), 0, "a weight of 0 is a weight, not a missing one");

        let mut p = Progress::new(0);
        let g = p.count_closed(&[issue(1, Some(3), "2026-11-01T10:00:00Z"), issue(2, None, "2026-11-02T10:00:00Z")], &rules(), 100);
        assert_eq!(g.xp, 40);
        assert_eq!(p.total_xp, 40);
        assert_eq!(p.issues[&1].xp, 30);
        assert_eq!(p.issues[&2].weight, None);
    }

    #[test]
    fn each_issue_counts_once() {
        let mut p = Progress::new(0);
        let a = issue(10, Some(2), "2026-11-01T10:00:00Z");
        // The same issue twice in one batch (pages overlap) and again in the next sync.
        p.count_closed(&[a.clone(), a.clone()], &rules(), 1);
        assert_eq!(p.total_xp, 20);
        // Reopened and closed again, now with a bigger weight: keeps the XP from the first count.
        let mut again = a.clone();
        again.weight = Some(8);
        again.closed_at = parse_time("2026-11-20T10:00:00Z");
        let g = p.count_closed(&[again], &rules(), 2);
        assert_eq!(g, Gains::default());
        assert_eq!(p.total_xp, 20);
        assert_eq!(p.issues[&10].weight, Some(2));
        assert_eq!(p.issues.len(), 1);
    }

    #[test]
    fn only_my_closed_issues_since_the_start_count() {
        let mut r = rules();
        r.since = parse_time("2026-11-01").unwrap();
        let mut someone_elses = issue(1, Some(5), "2026-11-05T00:00:00Z");
        someone_elses.assignees = Some(vec!["vilu".into()]);
        let mut moved = issue(2, Some(5), "2026-11-05T00:00:00Z");
        moved.moved = true;
        let old = issue(3, Some(5), "2026-10-31T23:59:59Z");
        let mut open = issue(4, Some(5), "2026-11-05T00:00:00Z");
        open.closed_at = None;
        let mine = issue(5, Some(1), "2026-11-01T00:00:00Z");
        let mut p = Progress::new(0);
        let g = p.count_closed(&[someone_elses, moved, old, open, mine], &r, 1);
        assert_eq!(g.issues, vec![5]);
        assert_eq!(p.total_xp, 10);
    }

    #[test]
    fn one_item_per_level_credited_to_the_issue_that_crossed_it() {
        let mut p = Progress::new(0);
        // 50 XP for level 2: the first issue (also 50) gets there, the second goes several levels further.
        let g = p.count_closed(&[issue(1, Some(5), "2026-11-01T00:00:00Z"), issue(2, Some(40), "2026-11-02T00:00:00Z")], &rules(), 1);
        assert_eq!(p.total_xp, 450);
        let top = level_for(450).0;
        assert_eq!(g.levels, (2..=top).collect::<Vec<_>>());
        assert_eq!(p.level, top);
        assert_eq!(p.items.iter().map(|i| i.dropped_at_level).collect::<Vec<_>>(), (2..=top).collect::<Vec<_>>());
        assert_eq!(p.items[0].trigger.as_ref().unwrap().title, "Issue 1");
        assert!(p.items[1..].iter().all(|i| i.trigger.as_ref().unwrap().title == "Issue 2"));
        assert_eq!(p.username.as_deref(), Some("osbo"));
    }

    #[test]
    fn times_parse_with_zones_and_round_trip() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-10T08:12:33.123Z"), parse_time("2026-11-10T09:12:33+01:00"));
        assert_eq!(parse_time("2026-01-01"), parse_time("2026-01-01T00:00:00Z"));
        let t = parse_time("2026-11-19T10:20:30Z").unwrap();
        assert_eq!(format_time(t), "2026-11-19T10:20:30Z");
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn merged_mrs_and_reviews_give_flat_xp_once_per_role() {
        let mut p = Progress::new(0);
        let mr = |id: u64, merged: &str, who: &[&str]| Issue { id, project: "group/app".into(), iid: id, title: format!("MR {id}"), closed_at: parse_time(merged), assignees: Some(who.iter().map(|w| w.to_string()).collect()), ..Default::default() };
        let mut r = rules();
        r.since = parse_time("2026-01-01").unwrap();
        let assigned = [mr(1, "2026-03-01T00:00:00Z", &["osbo"]), mr(2, "2026-03-02T00:00:00Z", &["vilu"]), mr(3, "2025-12-31T00:00:00Z", &["osbo"])];
        let g = p.count_merged(&assigned, MrRole::Assignee, &r, 5);
        assert_eq!(g.mrs, vec!["mr:1"], "not mine, and merged before the start, give nothing");
        assert_eq!(p.total_xp, 25);
        // Seen again in the next sync: nothing new.
        assert_eq!(p.count_merged(&assigned, MrRole::Assignee, &r, 6), Gains::default());
        // Reviewing the same MR is a separate credit.
        let g = p.count_merged(&[mr(1, "2026-03-01T00:00:00Z", &["osbo"]), mr(4, "2026-03-03T00:00:00Z", &["osbo"])], MrRole::Reviewer, &r, 7);
        assert_eq!(g.mrs, vec!["review:1", "review:4"]);
        assert_eq!(p.total_xp, 45);
        assert_eq!(p.mrs["review:4"].kind.as_deref(), Some("review"));
        // They show in the recent list, newest first, between issues.
        p.count_closed(&[issue(9, Some(1), "2026-03-04T00:00:00Z")], &r, 8);
        let recent: Vec<Option<&str>> = p.recent(3).iter().map(|c| c.kind.as_deref()).collect();
        assert_eq!(recent, vec![None, Some("review"), Some("review")]);
        // MRs never decide your class.
        assert!(crate::class::class_of(&p).is_none());
    }

    #[test]
    fn a_gentler_curve_catches_up_with_a_drop_per_level() {
        let mut p = Progress::new(0);
        p.total_xp = 8880;
        p.level = 6; // what the old curve gave for less
        let mut g = Gains::default();
        p.catch_up(None, &mut g, &DropChances::default());
        assert_eq!(p.level, 14);
        assert_eq!(g.levels, (7..=14).collect::<Vec<_>>());
        assert_eq!(p.items.len(), 8);
    }

    #[test]
    fn the_store_stocks_once_a_day_and_trades_for_gold() {
        let mut p = Progress::new(0);
        p.level = 10;
        // 40 closed issues at 25 gold each; MRs give none.
        for id in 0..40 {
            p.issues.insert(id, Counted { gold: Some(25), ..Default::default() });
        }
        p.mrs.insert("mr:1".into(), Counted { kind: Some("mr".into()), ..Default::default() });
        assert_eq!(p.gold(), 1000);
        assert!(p.restock("2026-11-19"));
        let count = |r| p.store.items.iter().filter(|i| i.rarity == r).count();
        assert_eq!([count(loot::Rarity::Common), count(loot::Rarity::Uncommon), count(loot::Rarity::Rare), count(loot::Rarity::Epic), count(loot::Rarity::Legendary)], [5, 3, 2, 1, 0]);
        assert!(p.store.items.iter().all(|i| i.item_level == 10));
        let before = p.store.clone();
        assert!(!p.restock("2026-11-19"), "once a day");
        assert_eq!(p.store, before);

        // Buy a Common (165 gold at level 10): into the bag, at your level, gold goes down.
        let common = p.store.items.iter().find(|i| i.rarity == loot::Rarity::Common).unwrap().clone();
        let id = p.buy(&common.id, 5).unwrap();
        assert_eq!(p.gold(), 1000 - 165);
        assert_eq!(p.store.items.len(), 10);
        assert!(p.bag().iter().any(|i| i.id == id && i.bought && i.name == common.name));
        assert_eq!(p.buy(&common.id, 5), Err(TradeError::NotInStore));
        // The Epic (4400) is far too dear.
        let epic = p.store.items.iter().find(|i| i.rarity == loot::Rarity::Epic).unwrap().id.clone();
        assert_eq!(p.buy(&epic, 5), Err(TradeError::TooPoor { price: 4400, gold: 835 }));

        // Sell it back for a quarter (41); a worn item has to come off first.
        assert_eq!(p.sell(&id), Ok(41));
        assert_eq!(p.gold(), 835 + 41);
        let mut other = p.store.items[0].clone();
        other.id = "worn".into();
        p.items.push(other);
        p.equip("worn", None).unwrap();
        assert_eq!(p.sell("worn"), Err(TradeError::Worn));
        assert_eq!(p.sell("nope"), Err(TradeError::NotInBag));

        // The stock follows your level during the day, and a new day restocks.
        p.level = 12;
        p.scale_legendaries();
        assert!(p.store.items.iter().all(|i| i.item_level == 12 && i.stats.values().sum::<u32>() == loot::stat_budget(12, i.rarity)));
        assert!(p.restock("2026-11-20"));
        assert_eq!(p.store.items.len(), 11);
    }

    #[test]
    fn closed_issues_give_gold_and_old_ones_get_it_once() {
        let mut p = Progress::new(0);
        p.count_closed(&[issue(1, Some(3), "2026-11-01T00:00:00Z")], &rules(), 1);
        assert_eq!(p.gold(), 25);
        // An issue counted before gold existed earns it at the next count, once.
        p.issues.insert(99, Counted { gold: None, ..Default::default() });
        let mut r = rules();
        r.gold_per_issue = 40;
        p.count_closed(&[], &r, 2);
        assert_eq!(p.gold(), 65);
        p.count_closed(&[], &r, 3);
        assert_eq!(p.gold(), 65);
        // Merged MRs and reviews give XP, no gold.
        let mr = Issue { id: 5, closed_at: Some(10), ..Default::default() };
        p.count_merged(&[mr], MrRole::Assignee, &r, 4);
        assert_eq!(p.gold(), 65);
    }

    #[test]
    fn a_bought_item_does_not_take_the_place_of_a_level_drop() {
        let mut p = Progress::new(0);
        p.total_xp = 49;
        p.restock("2026-11-19");
        p.gold_from_sales = 1000;
        let id = p.store.items[0].id.clone();
        p.buy(&id, 1).unwrap(); // bought at level 1
        p.total_xp = 49 + 1;
        let mut g = Gains::default();
        p.catch_up(None, &mut g, &DropChances::default());
        assert_eq!(p.items.iter().filter(|i| !i.bought).count(), 1, "level 2 still drops its loot");
    }

    #[test]
    fn legendaries_grow_with_your_level() {
        let only = DropChances { poor: 0.0, common: 0.0, uncommon: 0.0, rare: 0.0, epic: 0.0, legendary: 1.0 };
        let mut p = Progress::new(0);
        // Level 2 drops a legendary at item level 2.
        p.count_closed(&[issue(1, Some(10), "2026-11-01T00:00:00Z")], &Rules { chances: only, ..rules() }, 1);
        let it = p.items.iter().find(|i| i.rarity == loot::Rarity::Legendary).unwrap().clone();
        assert_eq!((it.item_level, it.stats.values().sum::<u32>()), (2, loot::stat_budget(2, loot::Rarity::Legendary)));
        assert!(it.flavor.is_some());
        // Ten levels later it is item level 12, with the legendary budget for 12, same leading stat.
        let mut r = rules();
        r.chances = DropChances { poor: 1.0, ..DropChances::default() };
        p.count_closed(&[issue(2, Some(2000), "2026-11-02T00:00:00Z")], &r, 2);
        let now = p.item(&it.id).unwrap();
        assert_eq!(now.item_level, p.level);
        assert!(p.level > 10);
        assert_eq!(now.stats.values().sum::<u32>(), loot::stat_budget(p.level, loot::Rarity::Legendary));
        let lead = |s: &BTreeMap<loot::Stat, u32>| s.iter().max_by_key(|(_, v)| **v).map(|(k, _)| *k);
        assert_eq!(lead(&now.stats), lead(&it.stats));
        // Other items stay at the level they dropped.
        assert!(p.items.iter().filter(|i| i.rarity != loot::Rarity::Legendary).all(|i| i.item_level == i.dropped_at_level));
    }

    #[test]
    fn old_legendaries_become_ones_from_the_list() {
        let mut p = Progress::new(0);
        p.version = 5;
        p.level = 7;
        let mut old = loot::generate(1, 3, &DropChances { poor: 0.0, common: 0.0, uncommon: 0.0, rare: 0.0, epic: 0.0, legendary: 1.0 }, &[]);
        old.name = "Hearthbrew".into(); // from the old list, not on today's
        old.kind = loot::Kind::Mug;
        p.items.push(old);
        assert!(p.migrate());
        let it = &p.items[0];
        let l = loot::legendary(&it.name).expect("a legendary from the list");
        assert_eq!(l.1.slot(), loot::Kind::Mug.slot());
        assert_eq!(it.flavor.as_deref(), Some(l.3));
        assert_eq!(it.item_level, 7, "and it grew to your level");
    }

    #[test]
    fn old_items_get_generic_names_and_budget_stats_once() {
        let mut p = Progress::new(0);
        p.version = 1;
        let mut old = loot::generate(5, 6, &DropChances { poor: 0.0, common: 0.0, uncommon: 0.0, rare: 1.0, epic: 0.0, legendary: 0.0 }, &[]);
        old.name = "Stalwart Cap of Haste".into();
        old.stats = [(loot::Stat::Speed, 99)].into_iter().collect();
        p.items.push(old.clone());
        p.equipped.insert(old.kind.slot(), old.id.clone());
        assert!(p.migrate());
        let it = &p.items[0];
        assert_eq!(it.name, loot::generic_name(it.kind, it.base(), loot::Rarity::Rare));
        assert!(it.name.starts_with("Excellent "), "{}", it.name);
        assert_eq!(it.stats.values().sum::<u32>(), loot::stat_budget(6, loot::Rarity::Rare));
        assert_eq!((it.kind, it.rarity, it.item_level, &it.id), (old.kind, old.rarity, old.item_level, &old.id));
        assert_eq!(p.equipped[&old.kind.slot()], old.id, "still worn");
        let after = p.clone();
        assert!(!p.migrate(), "only once");
        assert_eq!(p, after);

        // A version 3 file with a Common item rolled at the old budget: scaled down to the new one.
        let mut v3 = after.clone();
        v3.version = 3;
        v3.items[0].rarity = loot::Rarity::Common;
        v3.items[0].stats = [(loot::Stat::Speed, 10), (loot::Stat::Focus, 3)].into_iter().collect(); // 13: level 6 at 1.4
        assert!(v3.migrate());
        assert_eq!(v3.items[0].stats.values().sum::<u32>(), loot::stat_budget(6, loot::Rarity::Common));
        assert!(v3.items[0].stats[&loot::Stat::Speed] > v3.items[0].stats[&loot::Stat::Focus]);

        // A version 2 file: renamed to the new words, stats kept.
        let mut v2 = after.clone();
        v2.version = 2;
        v2.items[0].name = "Exquisite Whatever".into();
        let stats = v2.items[0].stats.clone();
        assert!(v2.migrate());
        assert!(v2.items[0].name.starts_with("Excellent "));
        assert_eq!(v2.items[0].stats, stats);
    }

    #[test]
    fn progress_survives_restarts_and_damage() {
        let dir = std::env::temp_dir().join(format!("claude-hub-progress-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store { path: dir.join("progress.json") };
        assert!(store.load().is_none());
        store.update(|p| p.total_xp = 5).unwrap();
        // A new session: a fresh Store on the same file sees everything.
        let again = Store { path: store.path.clone() };
        assert_eq!(again.load().unwrap().total_xp, 5);
        assert!(again.load().unwrap().started_at > 0);
        again.update(|p| {
            p.total_xp = 7;
            p.appearance = Some(Appearance::default());
        })
        .unwrap();
        assert!(dir.join("progress.json.bak").exists(), "the previous version is kept");

        // The file gets damaged: reading falls back to the backup, and the next change restores from it.
        std::fs::write(&store.path, b"{not json").unwrap();
        assert_eq!(store.load().unwrap().total_xp, 5);
        store.update(|p| p.total_xp += 1).unwrap();
        assert_eq!(store.load().unwrap().total_xp, 6);
        assert_eq!(std::fs::read(dir.join("progress.json.damaged")).unwrap(), b"{not json");

        // Both damaged: refuse to touch either.
        std::fs::write(&store.path, b"{bad").unwrap();
        std::fs::write(dir.join("progress.json.bak"), b"{bad").unwrap();
        assert!(store.update(|p| p.total_xp = 9).is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), b"{bad");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

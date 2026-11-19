//! Loot: one item per level, the same item for the same (user, level) every time.
//! All names and flavour text are original to claude-hub.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::DropChances;
use crate::gear::Slot;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    Poor,
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub const ALL: [Rarity; 6] = [Rarity::Poor, Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Epic, Rarity::Legendary];

    fn chance(self, c: &DropChances) -> f64 {
        match self {
            Rarity::Poor => c.poor,
            Rarity::Common => c.common,
            Rarity::Uncommon => c.uncommon,
            Rarity::Rare => c.rare,
            Rarity::Epic => c.epic,
            Rarity::Legendary => c.legendary,
        }
        .max(0.0)
    }

    /// The most different stats an item of this rarity can have.
    pub fn max_stats(self) -> usize {
        match self {
            Rarity::Poor | Rarity::Common => 2,
            Rarity::Uncommon | Rarity::Rare => 3,
            Rarity::Epic | Rarity::Legendary => 4,
        }
    }

    /// How much more stats this rarity gives than Poor at the same item level.
    pub fn multiplier(self) -> f64 {
        match self {
            Rarity::Poor => 1.0,
            Rarity::Common => 1.2,
            Rarity::Uncommon => 1.9,
            Rarity::Rare => 2.5,
            Rarity::Epic => 3.2,
            Rarity::Legendary => 4.0,
        }
    }

    /// The word in front of the base name: "Broken Mousepad", "Excellent Helm". Legendaries have names of their own.
    pub fn quality(self) -> &'static str {
        match self {
            Rarity::Poor => "Broken",
            Rarity::Common => "Working",
            Rarity::Uncommon => "Good",
            Rarity::Rare => "Excellent",
            Rarity::Epic => "Masterwork",
            Rarity::Legendary => "",
        }
    }
}

/// What an item costs in the store: (item level + 1) times a rate by rarity. The one place to tune prices.
/// Dear on purpose: at level 14 an Epic is 6000 gold, while a closed issue gives 25.
pub fn price(item_level: u32, rarity: Rarity) -> u64 {
    let rate = match rarity {
        Rarity::Poor => 5,
        Rarity::Common => 15,
        Rarity::Uncommon => 40,
        Rarity::Rare => 120,
        Rarity::Epic => 400,
        Rarity::Legendary => 800,
    };
    (item_level.max(1) as u64 + 1) * rate
}

/// The total of all stats on an item: item level + 1 for Poor, times the rarity's multiplier.
/// A Poor item at level 1 has 2; the one place to tune how strong items get.
pub fn stat_budget(item_level: u32, rarity: Rarity) -> u32 {
    let base = (item_level.max(1) + 1) as f64;
    (base * rarity.multiplier()).round().max(1.0) as u32
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Weapon,
    Helmet,
    Headphones,
    Chest,
    Gloves,
    Boots,
    Cape,
    Shield,
    Mug,
    Keyboard,
    Trinket,
}

impl Kind {
    pub const ALL: [Kind; 11] = [Kind::Weapon, Kind::Helmet, Kind::Headphones, Kind::Chest, Kind::Gloves, Kind::Boots, Kind::Cape, Kind::Shield, Kind::Mug, Kind::Keyboard, Kind::Trinket];

    /// The one slot this kind of item goes in.
    pub fn slot(self) -> Slot {
        match self {
            Kind::Helmet | Kind::Headphones => Slot::Head,
            Kind::Chest => Slot::Chest,
            Kind::Gloves => Slot::Hands,
            Kind::Boots => Slot::Feet,
            Kind::Weapon => Slot::MainHand,
            Kind::Shield | Kind::Mug | Kind::Keyboard => Slot::OffHand,
            Kind::Cape => Slot::Back,
            Kind::Trinket => Slot::Trinket,
        }
    }

    /// Base names; an item's `look` picks one, and the town draws the matching shape.
    fn bases(self) -> &'static [&'static str] {
        match self {
            Kind::Weapon => &["Sword", "Axe", "Mace", "Spear", "Staff"],
            Kind::Helmet => &["Helm", "Cap", "Hood", "Circlet"],
            Kind::Headphones => &["Headphones", "Headset", "Earmuffs"],
            Kind::Chest => &["Tunic", "Hauberk", "Hoodie", "Vest"],
            Kind::Gloves => &["Gloves", "Gauntlets", "Mitts"],
            Kind::Boots => &["Boots", "Treads", "Sandals"],
            Kind::Cape => &["Cape", "Cloak", "Mantle"],
            Kind::Shield => &["Shield", "Buckler", "Kite Shield", "Mousepad"],
            Kind::Mug => &["Mug", "Tankard", "Thermos"],
            Kind::Keyboard => &["Keyboard", "Keypad", "Split Keyboard"],
            Kind::Trinket => &["Charm", "Amulet", "Ring", "Rubber Duck"],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Stat {
    Focus,
    Speed,
    Insight,
    Stamina,
}

impl Stat {
    pub const ALL: [Stat; 4] = [Stat::Focus, Stat::Speed, Stat::Insight, Stat::Stamina];

}

/// The particle effect of a legendary, matching its name.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Aura {
    Ember,
    Frost,
    Sparkle,
    Storm,
    Leaf,
    Shadow,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Trigger {
    pub project: String,
    pub iid: u64,
    pub title: String,
    /// None for an issue, "mr" or "review" for a merged MR.
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Item {
    /// "lvl-7": one item per level.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub rarity: Rarity,
    pub item_level: u32,
    pub stats: BTreeMap<Stat, u32>,
    pub flavor: Option<String>,
    pub dropped_at_level: u32,
    /// The issue that made you reach that level.
    pub trigger: Option<Trigger>,
    /// Picks the base shape and trim pattern the town draws.
    pub look: u32,
    pub aura: Option<Aura>,
    /// Bought in the store, not dropped by a level-up.
    #[serde(default)]
    pub bought: bool,
}

impl Item {
    /// What the store asks for it.
    pub fn price(&self) -> u64 {
        price(self.item_level, self.rarity)
    }

    /// What the store pays for it: a quarter of its price.
    pub fn sell_price(&self) -> u64 {
        (self.price() / 4).max(1)
    }

    /// Which base shape (index into the kind's base names) the item has; the town draws that shape.
    pub fn base(&self) -> usize {
        self.look as usize % self.kind.bases().len()
    }
}

// ---------------------------------------------------------------- randomness

/// SplitMix64: tiny, and the same on every platform and Rust version, so a seed always gives the same item.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub fn pick<'a, T>(&mut self, list: &'a [T]) -> &'a T {
        &list[self.below(list.len())]
    }
}

/// A fresh random seed for a drop, from the OS. Every drop is its own roll.
pub fn random_seed() -> u64 {
    use std::io::Read;
    let mut buf = [0u8; 8];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_ok() {
        return u64::from_le_bytes(buf);
    }
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    t ^ ((std::process::id() as u64) << 32)
}

// ---------------------------------------------------------------- legendaries

/// (name, kind, aura, flavour): three legendaries for every slot, hand-written for claude-hub.
/// Each drops at most once, and grows with you: its item level and stats follow your level.
pub const LEGENDARIES: &[(&str, Kind, Aura, &str)] = &[
    // Head
    ("Rimecrown", Kind::Helmet, Aura::Frost, "Keeps a cool head through any incident call."),
    ("Embercrest", Kind::Helmet, Aura::Ember, "Lit from within by every bug it has outlasted."),
    ("Quietkeep", Kind::Headphones, Aura::Shadow, "Open-plan offices fall silent around its wearer."),
    // Chest
    ("Nightshift", Kind::Chest, Aura::Shadow, "Woven between midnight and the deploy window."),
    ("Thousandfold", Kind::Chest, Aura::Sparkle, "Each fold remembers a ticket closed in it."),
    ("Stillwater Vest", Kind::Chest, Aura::Frost, "Calm enough to read a stack trace twice."),
    // Hands
    ("Glimmerthread", Kind::Gloves, Aura::Sparkle, "Stitches tidy commits out of messy afternoons."),
    ("The Patient Hand", Kind::Gloves, Aura::Leaf, "Never pushes on a Friday."),
    ("Copperdawn", Kind::Gloves, Aura::Ember, "Warm to the touch whenever a build goes green."),
    // Feet
    ("Zero Downtime", Kind::Boots, Aura::Storm, "Its wearer has never been seen to stop."),
    ("Coldstart", Kind::Boots, Aura::Frost, "Slow on the first step, unstoppable after."),
    ("Mosswalker", Kind::Boots, Aura::Leaf, "Leaves small green footprints through old codebases."),
    // Main hand
    ("The Last Merge", Kind::Weapon, Aura::Sparkle, "Every conflict ends here, one way or another."),
    ("Sunderquill", Kind::Weapon, Aura::Storm, "Writes the one-line fix after three days of reading."),
    ("The Long Undo", Kind::Weapon, Aura::Shadow, "Cuts back through time to the last good commit."),
    // Off-hand
    ("The Heisenmug", Kind::Mug, Aura::Sparkle, "The coffee is hot until someone looks at it."),
    ("Emberforge Keys", Kind::Keyboard, Aura::Ember, "Each keystroke leaves a small glowing mark."),
    ("The Unbroken Build", Kind::Shield, Aura::Storm, "Polished by a hundred passing test suites."),
    // Back
    ("Cindermantle", Kind::Cape, Aura::Ember, "Trails sparks down every corridor of the office."),
    ("Rebasewind", Kind::Cape, Aura::Frost, "History bends quietly wherever this cloak passes."),
    ("The Green Pipeline", Kind::Cape, Aura::Leaf, "Nothing fails while it flutters."),
    // Trinket
    ("The Quiet Linter", Kind::Trinket, Aura::Leaf, "Points out your mistakes, kindly and only once."),
    ("Lanternfall", Kind::Trinket, Aura::Ember, "Found at the bottom of a very long log file."),
    ("Wanderlight", Kind::Trinket, Aura::Sparkle, "Always points toward the oldest open issue."),
];

/// The legendary with this name, if it is one.
pub fn legendary(name: &str) -> Option<&'static (&'static str, Kind, Aura, &'static str)> {
    LEGENDARIES.iter().find(|l| l.0 == name)
}

// ---------------------------------------------------------------- generation

/// The first roll of every drop: which slot it is for. Equal weights; change them here to favour a slot.
pub const SLOT_TABLE: [(Slot, f64); 8] = [
    (Slot::Head, 1.0),
    (Slot::Chest, 1.0),
    (Slot::Hands, 1.0),
    (Slot::Feet, 1.0),
    (Slot::MainHand, 1.0),
    (Slot::OffHand, 1.0),
    (Slot::Back, 1.0),
    (Slot::Trinket, 1.0),
];

fn roll_slot(rng: &mut Rng) -> Slot {
    let total: f64 = SLOT_TABLE.iter().map(|(_, w)| w).sum();
    let mut x = rng.unit() * total;
    for (slot, w) in SLOT_TABLE {
        x -= w;
        if x < 0.0 {
            return slot;
        }
    }
    SLOT_TABLE[SLOT_TABLE.len() - 1].0
}

fn roll_rarity(rng: &mut Rng, chances: &DropChances) -> Rarity {
    let total: f64 = Rarity::ALL.iter().map(|r| r.chance(chances)).sum();
    if total <= 0.0 {
        return Rarity::Common;
    }
    let mut x = rng.unit() * total;
    for r in Rarity::ALL {
        x -= r.chance(chances);
        if x < 0.0 {
            return r;
        }
    }
    Rarity::Common
}

/// Random stats that add up to the budget for this item level and rarity, over a random set of stats
/// (at most `Rarity::max_stats`), each at least 1.
pub fn roll_stats(rng: &mut Rng, item_level: u32, rarity: Rarity) -> BTreeMap<Stat, u32> {
    let budget = stat_budget(item_level, rarity);
    let count = (1 + rng.below(rarity.max_stats())).min(budget as usize).max(1);
    let mut pool = Stat::ALL.to_vec();
    let chosen: Vec<Stat> = (0..count).map(|_| pool.remove(rng.below(pool.len()))).collect();
    // Uneven weights, so one stat can lead without the others vanishing.
    let weights: Vec<f64> = chosen.iter().map(|_| 0.25 + rng.unit()).collect();
    let total: f64 = weights.iter().sum();
    let mut stats: BTreeMap<Stat, u32> = chosen.iter().map(|s| (*s, 1)).collect();
    for _ in count as u32..budget {
        let mut x = rng.unit() * total;
        let mut pick = chosen[chosen.len() - 1];
        for (s, w) in chosen.iter().zip(&weights) {
            x -= w;
            if x < 0.0 {
                pick = *s;
                break;
            }
        }
        *stats.get_mut(&pick).unwrap() += 1;
    }
    stats
}

/// The same stats scaled to a new total, keeping their proportions; every stat keeps at least 1.
pub fn rescale(stats: &BTreeMap<Stat, u32>, total: u32) -> BTreeMap<Stat, u32> {
    let old: u32 = stats.values().sum();
    if old == 0 || stats.is_empty() {
        return stats.clone();
    }
    let total = total.max(stats.len() as u32);
    let mut out: BTreeMap<Stat, u32> = stats.iter().map(|(s, v)| (*s, ((*v as f64 * total as f64 / old as f64).floor() as u32).max(1))).collect();
    // Hand out what rounding left over (or take back what the minimum of 1 added), biggest stats first.
    let mut order: Vec<Stat> = stats.keys().copied().collect();
    order.sort_by_key(|s| std::cmp::Reverse(stats[s]));
    let mut i = 0;
    while out.values().sum::<u32>() != total {
        let s = order[i % order.len()];
        if out.values().sum::<u32>() < total {
            *out.get_mut(&s).unwrap() += 1;
        } else if out[&s] > 1 {
            *out.get_mut(&s).unwrap() -= 1;
        }
        i += 1;
    }
    out
}

/// "Broken Mousepad": the rarity's quality word and the item's base name.
pub fn generic_name(kind: Kind, base: usize, rarity: Rarity) -> String {
    let bases = kind.bases();
    format!("{} {}", rarity.quality(), bases[base % bases.len()])
}

/// The item for reaching `level`, rolled from `seed` (use `random_seed()` for a real drop):
/// first the slot (`SLOT_TABLE`), then the rarity (`chances`), then the item: a kind that fits the
/// slot and a base, or for a legendary a name for that slot. `taken` are the legendary names already
/// owned, so none drops twice.
pub fn generate(seed: u64, level: u32, chances: &DropChances, taken: &[String]) -> Item {
    let mut rng = Rng::new(seed);
    let slot = roll_slot(&mut rng);
    let mut rarity = roll_rarity(&mut rng, chances);
    let look = (rng.next() & 0xffff) as u32;
    let free: Vec<_> = LEGENDARIES.iter().filter(|(n, k, ..)| k.slot() == slot && !taken.iter().any(|t| t == n)).collect();
    // You own every legendary for this slot: the best you can get is an Epic.
    if rarity == Rarity::Legendary && free.is_empty() {
        rarity = Rarity::Epic;
    }
    let (legend, kind) = if rarity == Rarity::Legendary {
        let (n, k, a, f) = **rng.pick(&free);
        (Some((n.to_string(), a, f.to_string())), k)
    } else {
        (None, *rng.pick(&slot.kinds()))
    };
    let stats = roll_stats(&mut rng, level, rarity);
    let mut item = Item { id: format!("lvl-{level}"), name: String::new(), kind, rarity, item_level: level, stats, flavor: None, dropped_at_level: level, trigger: None, look, aura: None, bought: false };
    match legend {
        Some((name, aura, flavor)) => {
            item.name = name;
            item.aura = Some(aura);
            item.flavor = Some(flavor);
        }
        None => item.name = generic_name(kind, item.base(), rarity),
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(r: Rarity) -> DropChances {
        let mut c = DropChances { poor: 0.0, common: 0.0, uncommon: 0.0, rare: 0.0, epic: 0.0, legendary: 0.0 };
        match r {
            Rarity::Poor => c.poor = 1.0,
            Rarity::Common => c.common = 1.0,
            Rarity::Uncommon => c.uncommon = 1.0,
            Rarity::Rare => c.rare = 1.0,
            Rarity::Epic => c.epic = 1.0,
            Rarity::Legendary => c.legendary = 1.0,
        }
        c
    }

    #[test]
    fn a_fixed_seed_gives_the_same_item_and_random_seeds_differ() {
        let c = DropChances::default();
        for seed in 0..200 {
            assert_eq!(generate(seed, 7, &c, &[]), generate(seed, 7, &c, &[]));
        }
        let mut rng = Rng::new(42);
        assert_eq!([rng.next(), rng.next()], [13679457532755275413, 2949826092126892291]);
        // Drops are random: two real drops for the same level are rarely the same item.
        let a: Vec<Item> = (0..20).map(|_| generate(random_seed(), 7, &c, &[])).collect();
        assert!(a.iter().any(|x| x != &a[0]));
    }

    #[test]
    fn the_stat_budget_scales_with_item_level_and_rarity() {
        assert_eq!(stat_budget(1, Rarity::Poor), 2, "a Poor item at level 1 has 2 points");
        assert_eq!(stat_budget(1, Rarity::Legendary), 8);
        for level in 1..60 {
            assert_eq!(stat_budget(level, Rarity::Poor), level + 1, "Poor is item level + 1");
        }
        assert_eq!(stat_budget(10, Rarity::Epic), 35);
        assert_eq!(stat_budget(10, Rarity::Legendary), 44);
        assert_eq!(stat_budget(6, Rarity::Common), 8, "Common is only a little above Poor (7)");
        assert_eq!(stat_budget(10, Rarity::Common), 13);
        for level in 1..60 {
            // Common may tie Poor at the lowest levels; from Common up, each rarity is stronger.
            assert!(stat_budget(level, Rarity::Common) >= stat_budget(level, Rarity::Poor));
            for w in Rarity::ALL[1..].windows(2) {
                assert!(stat_budget(level, w[1]) > stat_budget(level, w[0]), "{:?} at {level}", w[1]);
            }
            for r in Rarity::ALL {
                assert!(stat_budget(level + 1, r) > stat_budget(level, r) || level == 0);
            }
        }
    }

    #[test]
    fn stats_add_up_to_the_budget_over_random_stats() {
        for r in Rarity::ALL {
            for level in [1, 2, 6, 15, 40] {
                let mut counts = std::collections::BTreeSet::new();
                let mut seen = std::collections::BTreeSet::new();
                for seed in 0..300 {
                    let it = generate(seed, level, &only(r), &[]);
                    assert_eq!(it.stats.values().sum::<u32>(), stat_budget(level, r), "{r:?} level {level}");
                    assert!(it.stats.len() <= r.max_stats() && !it.stats.is_empty());
                    assert!(it.stats.values().all(|v| *v >= 1));
                    counts.insert(it.stats.len());
                    seen.extend(it.stats.keys().copied());
                }
                // Not always the same stats or the same number of them.
                assert_eq!(seen.len(), 4, "{r:?} level {level}: every stat shows up");
                if stat_budget(level, r) >= r.max_stats() as u32 {
                    assert_eq!(counts.len(), r.max_stats(), "{r:?} level {level}: {counts:?}");
                }
            }
        }
        // The example: a Poor level 1 item has 2 points, e.g. 1 Speed and 1 Stamina, or 2 of one.
        let it = generate(3, 1, &only(Rarity::Poor), &[]);
        assert_eq!(it.stats.values().sum::<u32>(), 2);
    }

    #[test]
    fn generic_names_by_rarity_and_unique_names_only_for_legendaries() {
        for seed in 0..500 {
            let it = generate(seed, 5, &DropChances::default(), &[]);
            if it.rarity == Rarity::Legendary {
                assert!(LEGENDARIES.iter().any(|l| l.0 == it.name), "{}", it.name);
                assert!(it.flavor.is_some() && it.aura.is_some());
            } else {
                assert_eq!(it.name, generic_name(it.kind, it.base(), it.rarity));
                assert!(it.name.starts_with(it.rarity.quality()));
                assert!(it.flavor.is_none() && it.aura.is_none());
            }
        }
        assert_eq!(generic_name(Kind::Shield, 3, Rarity::Poor), "Broken Mousepad");
        assert_eq!(generic_name(Kind::Shield, 3, Rarity::Common), "Working Mousepad");
        assert_eq!(generic_name(Kind::Shield, 3, Rarity::Uncommon), "Good Mousepad");
        assert_eq!(generic_name(Kind::Shield, 3, Rarity::Rare), "Excellent Mousepad");
        assert_eq!(generic_name(Kind::Helmet, 0, Rarity::Epic), "Masterwork Helm");
    }

    #[test]
    fn rescaling_keeps_proportions_and_hits_the_total() {
        let s: BTreeMap<Stat, u32> = [(Stat::Speed, 10), (Stat::Stamina, 3)].into_iter().collect();
        let r = rescale(&s, 11);
        assert_eq!(r.values().sum::<u32>(), 11);
        assert!(r[&Stat::Speed] > r[&Stat::Stamina]);
        assert_eq!(rescale(&s, 1).values().sum::<u32>(), 2, "every stat keeps at least 1");
    }

    #[test]
    fn prices_scale_with_item_level_and_rarity() {
        assert_eq!(price(14, Rarity::Common), 225);
        assert_eq!(price(14, Rarity::Epic), 6000);
        for level in 1..60 {
            assert!(price(level + 1, Rarity::Rare) > price(level, Rarity::Rare));
            for w in Rarity::ALL.windows(2) {
                assert!(price(level, w[1]) > price(level, w[0]));
            }
        }
        let it = generate(1, 10, &only(Rarity::Uncommon), &[]);
        assert_eq!(it.price(), 440);
        assert_eq!(it.sell_price(), 110, "a quarter of the price");
    }

    #[test]
    fn rarity_follows_the_drop_chances() {
        let c = DropChances::default();
        let mut counts: BTreeMap<Rarity, u32> = BTreeMap::new();
        for seed in 0..20000 {
            *counts.entry(generate(seed, 5, &c, &[]).rarity).or_default() += 1;
        }
        let pct = |r| counts.get(&r).copied().unwrap_or(0) as f64 / 200.0;
        assert!((pct(Rarity::Common) - 40.0).abs() < 2.0, "{counts:?}");
        assert!((pct(Rarity::Poor) - 20.0).abs() < 2.0, "{counts:?}");
        assert!((pct(Rarity::Legendary) - 1.0).abs() < 0.5, "{counts:?}");
        assert_eq!(generate(1, 3, &only(Rarity::Legendary), &[]).rarity, Rarity::Legendary);
    }

    #[test]
    fn slot_first_then_rarity_then_the_item() {
        let mut slots: BTreeMap<Slot, u32> = BTreeMap::new();
        let mut kinds: BTreeMap<Kind, u32> = BTreeMap::new();
        for seed in 0..40000 {
            let it = generate(seed, 5, &DropChances::default(), &[]);
            *slots.entry(it.kind.slot()).or_default() += 1;
            *kinds.entry(it.kind).or_default() += 1;
        }
        // Every slot equally likely, however many kinds it has.
        for (slot, n) in &slots {
            assert!((*n as f64 / 400.0 - 12.5).abs() < 1.0, "{slot:?}: {slots:?}");
        }
        assert_eq!(slots.len(), 8);
        // Within the off-hand, shield, mug and keyboard share its eighth.
        for k in [Kind::Shield, Kind::Mug, Kind::Keyboard] {
            assert!((kinds[&k] as f64 / 400.0 - 12.5 / 3.0).abs() < 0.8, "{k:?}: {kinds:?}");
        }
        // The slot is rolled before the rarity: the same seed gives the same slot whatever the rarity.
        for seed in 0..200 {
            let a = generate(seed, 5, &only(Rarity::Poor), &[]).kind.slot();
            let b = generate(seed, 5, &only(Rarity::Epic), &[]).kind.slot();
            let c = generate(seed, 5, &only(Rarity::Legendary), &[]).kind.slot();
            assert!(a == b && b == c, "seed {seed}");
        }
    }

    #[test]
    fn a_legendary_never_drops_twice_and_then_the_slot_gives_epics() {
        let mut taken: Vec<String> = vec![];
        for seed in 0..400 {
            let it = generate(seed, 5, &only(Rarity::Legendary), &taken);
            if it.rarity == Rarity::Legendary {
                assert!(!taken.contains(&it.name), "{} dropped twice", it.name);
                let l = legendary(&it.name).expect("only hand-written legendaries");
                assert_eq!((it.kind, it.aura, it.flavor.as_deref()), (l.1, Some(l.2), Some(l.3)));
                taken.push(it.name);
            } else {
                // Only once every legendary for the slot is owned.
                assert_eq!(it.rarity, Rarity::Epic);
                let slot = it.kind.slot();
                assert!(LEGENDARIES.iter().filter(|l| l.1.slot() == slot).all(|l| taken.iter().any(|t| t == l.0)), "{slot:?}");
            }
        }
        assert_eq!(taken.len(), LEGENDARIES.len(), "all of them drop in time");
    }

    #[test]
    fn two_or_three_legendaries_for_every_slot_each_with_flavour() {
        for slot in Slot::ALL {
            let n = LEGENDARIES.iter().filter(|l| l.1.slot() == slot).count();
            assert!((2..=3).contains(&n), "{slot:?} has {n}");
        }
        let mut names: Vec<&str> = LEGENDARIES.iter().map(|l| l.0).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), LEGENDARIES.len());
        assert!(LEGENDARIES.iter().all(|l| l.3.len() > 10 && l.3.ends_with('.')));
    }
}

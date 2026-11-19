//! Equipment: which item goes in which slot, stat totals, and the layers your character is drawn from.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::loot::{Item, Kind, Rarity, Stat};
use crate::progress::Progress;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Head,
    Chest,
    Hands,
    Feet,
    MainHand,
    OffHand,
    Back,
    Trinket,
}

impl Slot {
    pub const ALL: [Slot; 8] = [Slot::Head, Slot::Chest, Slot::Hands, Slot::Feet, Slot::MainHand, Slot::OffHand, Slot::Back, Slot::Trinket];

    pub fn parse(s: &str) -> Option<Slot> {
        serde_json::from_value(serde_json::Value::String(s.into())).ok()
    }

    /// The item kinds that fit this slot.
    pub fn kinds(self) -> Vec<Kind> {
        Kind::ALL.into_iter().filter(|k| k.slot() == self).collect()
    }
}

#[derive(Debug, PartialEq)]
pub enum EquipError {
    NoSuchItem,
    WrongSlot { kind: Kind, slot: Slot },
}

impl std::fmt::Display for EquipError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EquipError::NoSuchItem => write!(f, "You do not have that item."),
            EquipError::WrongSlot { kind, slot } => write!(f, "A {kind:?} does not go in the {slot:?} slot."),
        }
    }
}

impl Progress {
    /// Put an item on. `slot` is where it was dropped, if dragged; it must be the item's own slot.
    /// Returns the item that was in the slot before, which goes back to the bag.
    pub fn equip(&mut self, item_id: &str, slot: Option<Slot>) -> Result<Option<String>, EquipError> {
        let kind = self.item(item_id).ok_or(EquipError::NoSuchItem)?.kind;
        let own = kind.slot();
        if let Some(s) = slot.filter(|s| *s != own) {
            return Err(EquipError::WrongSlot { kind, slot: s });
        }
        let old = self.equipped.insert(own, item_id.to_string());
        Ok(old.filter(|o| o != item_id))
    }

    /// Take off what is in this slot; it goes back to the bag.
    pub fn unequip(&mut self, slot: Slot) -> Option<String> {
        self.equipped.remove(&slot)
    }

    /// Unequip an item wherever it is worn.
    pub fn unequip_item(&mut self, item_id: &str) -> Option<Slot> {
        let slot = self.equipped.iter().find(|(_, id)| *id == item_id).map(|(s, _)| *s)?;
        self.equipped.remove(&slot);
        Some(slot)
    }

    pub fn worn(&self, slot: Slot) -> Option<&Item> {
        self.equipped.get(&slot).and_then(|id| self.item(id))
    }

    /// Items in the bag: owned and not worn, in drop order.
    pub fn bag(&self) -> Vec<&Item> {
        self.items.iter().filter(|i| !self.equipped.values().any(|id| *id == i.id)).collect()
    }

    /// All stats from what you wear.
    pub fn totals(&self) -> BTreeMap<Stat, i64> {
        let mut out: BTreeMap<Stat, i64> = Stat::ALL.iter().map(|s| (*s, 0)).collect();
        for slot in Slot::ALL {
            for (s, v) in self.worn(slot).map(|i| &i.stats).into_iter().flatten() {
                *out.entry(*s).or_default() += *v as i64;
            }
        }
        out
    }

    /// How the totals change if you put this item on instead of what is in its slot.
    pub fn compare(&self, item: &Item) -> BTreeMap<Stat, i64> {
        let current = self.worn(item.kind.slot()).filter(|w| w.id != item.id);
        Stat::ALL
            .iter()
            .map(|s| {
                let new = item.stats.get(s).copied().unwrap_or(0) as i64;
                let old = current.and_then(|c| c.stats.get(s)).copied().unwrap_or(0) as i64;
                (*s, new - old)
            })
            .filter(|(_, d)| *d != 0)
            .collect()
    }
}

/// One layer of the character, drawn back to front.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Layer {
    pub part: &'static str,
    /// The worn item, for item layers.
    pub item: Option<String>,
}

/// The helmet base (see `Kind::bases`) that is a circlet.
const CIRCLET: usize = 3;

/// The draw order of every part, back to front. The town paints exactly these layers in this order.
pub const ORDER: [&str; 11] = ["back", "body", "feet", "chest", "trinket", "hands", "hair", "helmet", "weapon", "off_hand", "effects"];

/// The layers your character is made of, back to front: the cape behind the body, hair under a helmet,
/// the weapon and off-hand in front, and the glow of worn legendaries on top.
pub fn layers(p: &Progress) -> Vec<Layer> {
    let item = |slot: Slot| p.worn(slot).map(|i| i.id.clone());
    let head = p.worn(Slot::Head);
    // A helmet covers the hair; a circlet and headphones sit on top of it.
    let hair_hidden = head.is_some_and(|h| h.kind == Kind::Helmet && h.base() != CIRCLET);
    let legendary = Slot::ALL.iter().any(|s| p.worn(*s).is_some_and(|i| i.rarity == Rarity::Legendary));
    let mut out = vec![];
    for part in ORDER {
        let layer = match part {
            "back" => item(Slot::Back).map(Some),
            "body" => Some(None),
            "feet" => item(Slot::Feet).map(Some),
            "chest" => item(Slot::Chest).map(Some),
            "trinket" => item(Slot::Trinket).map(Some),
            "hands" => item(Slot::Hands).map(Some),
            "hair" => (!hair_hidden).then_some(None),
            "helmet" => item(Slot::Head).map(Some),
            "weapon" => item(Slot::MainHand).map(Some),
            "off_hand" => item(Slot::OffHand).map(Some),
            "effects" => legendary.then_some(None),
            _ => None,
        };
        if let Some(item) = layer {
            out.push(Layer { part, item });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loot::Aura;

    fn item(id: &str, kind: Kind, rarity: Rarity, stats: &[(Stat, u32)]) -> Item {
        Item {
            id: id.into(),
            name: id.into(),
            kind,
            rarity,
            item_level: 1,
            stats: stats.iter().copied().collect(),
            flavor: None,
            dropped_at_level: 1,
            trigger: None,
            look: 0,
            aura: (rarity == Rarity::Legendary).then_some(Aura::Ember),
        }
    }

    fn player() -> Progress {
        let mut p = Progress::new(0);
        p.items = vec![
            item("helm", Kind::Helmet, Rarity::Common, &[(Stat::Stamina, 4)]),
            item("phones", Kind::Headphones, Rarity::Rare, &[(Stat::Focus, 9), (Stat::Insight, 2)]),
            item("mug", Kind::Mug, Rarity::Uncommon, &[(Stat::Focus, 3)]),
            item("shield", Kind::Shield, Rarity::Poor, &[(Stat::Stamina, 1)]),
            item("sword", Kind::Weapon, Rarity::Legendary, &[(Stat::Speed, 10), (Stat::Focus, 5)]),
            item("cape", Kind::Cape, Rarity::Epic, &[(Stat::Insight, 6)]),
        ];
        p
    }

    #[test]
    fn items_only_go_in_their_own_slot() {
        let mut p = player();
        assert_eq!(p.equip("helm", Some(Slot::Feet)), Err(EquipError::WrongSlot { kind: Kind::Helmet, slot: Slot::Feet }));
        assert_eq!(p.equip("mug", Some(Slot::MainHand)), Err(EquipError::WrongSlot { kind: Kind::Mug, slot: Slot::MainHand }));
        assert!(p.equipped.is_empty());
        assert_eq!(p.equip("helm", Some(Slot::Head)), Ok(None));
        assert_eq!(p.equip("mug", None), Ok(None), "double-click equips into the item's own slot");
        assert_eq!(p.equipped[&Slot::OffHand], "mug");
        assert_eq!(p.equip("nope", None), Err(EquipError::NoSuchItem));
        assert_eq!(Slot::OffHand.kinds(), vec![Kind::Shield, Kind::Mug, Kind::Keyboard]);
        assert_eq!(Slot::Head.kinds(), vec![Kind::Helmet, Kind::Headphones]);
    }

    #[test]
    fn equipping_swaps_the_old_item_back_to_the_bag() {
        let mut p = player();
        p.equip("mug", None).unwrap();
        assert!(!p.bag().iter().any(|i| i.id == "mug"));
        assert_eq!(p.equip("shield", None), Ok(Some("mug".into())));
        assert_eq!(p.equipped[&Slot::OffHand], "shield");
        assert!(p.bag().iter().any(|i| i.id == "mug"));
        assert_eq!(p.bag().len(), 5);
        // Equipping what is already worn changes nothing.
        assert_eq!(p.equip("shield", None), Ok(None));
        assert_eq!(p.unequip(Slot::OffHand), Some("shield".into()));
        assert_eq!(p.bag().len(), 6);
        p.equip("cape", None).unwrap();
        assert_eq!(p.unequip_item("cape"), Some(Slot::Back));
        assert!(p.equipped.is_empty());
    }

    #[test]
    fn stat_totals_and_comparison() {
        let mut p = player();
        assert!(p.totals().values().all(|v| *v == 0));
        p.equip("helm", None).unwrap();
        p.equip("sword", None).unwrap();
        p.equip("mug", None).unwrap();
        let t = p.totals();
        assert_eq!((t[&Stat::Focus], t[&Stat::Speed], t[&Stat::Insight], t[&Stat::Stamina]), (8, 10, 0, 4));
        // Headphones instead of the helmet: +9 focus, +2 insight, -4 stamina.
        let phones = p.item("phones").unwrap().clone();
        assert_eq!(p.compare(&phones), BTreeMap::from([(Stat::Focus, 9), (Stat::Insight, 2), (Stat::Stamina, -4)]));
        // A worn item compares against an empty slot: all its stats.
        let sword = p.item("sword").unwrap().clone();
        assert_eq!(p.compare(&sword), BTreeMap::from([(Stat::Speed, 10), (Stat::Focus, 5)]));
    }

    #[test]
    fn layers_follow_the_fixed_order() {
        let mut p = player();
        let parts = |p: &Progress| layers(p).iter().map(|l| l.part).collect::<Vec<_>>();
        assert_eq!(parts(&p), ["body", "hair"], "nothing worn: just you");
        for id in ["helm", "sword", "mug", "cape"] {
            p.equip(id, None).unwrap();
        }
        // Cape behind the body, hair hidden under the helmet, the legendary sword's glow on top.
        assert_eq!(parts(&p), ["back", "body", "helmet", "weapon", "off_hand", "effects"]);
        assert_eq!(layers(&p)[0].item.as_deref(), Some("cape"));
        // Headphones and a circlet go over the hair.
        p.equip("phones", None).unwrap();
        assert!(parts(&p).contains(&"hair"));
        let mut circlet = item("circlet", Kind::Helmet, Rarity::Rare, &[]);
        circlet.look = 3;
        assert_eq!(circlet.base(), CIRCLET);
        p.items.push(circlet);
        p.equip("circlet", None).unwrap();
        assert!(parts(&p).contains(&"hair") && parts(&p).contains(&"helmet"));
        let order: Vec<usize> = parts(&p).iter().map(|x| ORDER.iter().position(|o| o == x).unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
        assert_eq!(ORDER, ["back", "body", "feet", "chest", "trinket", "hands", "hair", "helmet", "weapon", "off_hand", "effects"]);
    }
}

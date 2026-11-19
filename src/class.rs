//! Your class: what you close most of, by label. Only these six labels count, matched exactly
//! (ignoring case), so "Severity::Non-Critical" is not "Severity::Critical".

use serde::Serialize;

use crate::progress::Progress;

pub struct Class {
    pub key: &'static str,
    pub name: &'static str,
    /// What the class is about, in the words of the label.
    pub calling: &'static str,
    pub flavor: &'static str,
    /// The labels that count for it.
    pub labels: &'static [&'static str],
}

pub const CLASSES: &[Class] = &[
    Class { key: "critical", name: "Beacon Warden", calling: "first responder", flavor: "First to answer when the alarm bells ring.", labels: &["Severity::Critical", "Critical"] },
    Class { key: "tech_debt", name: "Debtreaver", calling: "debt collector", flavor: "Collects what the codebase owes, with interest.", labels: &["Type::Tech Debt", "Tech Debt"] },
    Class { key: "low_hanging", name: "Orchard Forager", calling: "fruit picker", flavor: "Knows which fruit is ripe and within reach.", labels: &["Low hanging fruit"] },
    Class { key: "bug", name: "Verminbane", calling: "exterminator", flavor: "Hunts the creatures that nest in the code.", labels: &["Type::Bug", "Bug"] },
    Class { key: "enhancement", name: "Enchanter", calling: "enhancer", flavor: "Makes good things better with a touch of craft.", labels: &["Type::Enhancement", "Enhancement"] },
    Class { key: "documentation", name: "Lorescribe", calling: "scribe", flavor: "Writes down what others will need to know.", labels: &["Documentation", "Type::Documentation"] },
];

/// Before any issue with one of the labels is closed.
pub const NO_CLASS: (&str, &str) = ("Wanderer", "Close issues labelled Bug, Enhancement, Tech Debt, Low hanging fruit, Critical or Documentation to find your calling.");

#[derive(Serialize, Debug, PartialEq)]
pub struct Standing {
    pub key: &'static str,
    pub name: &'static str,
    pub calling: &'static str,
    pub count: usize,
}

impl Class {
    fn matches(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l.eq_ignore_ascii_case(label.trim()))
    }
}

/// Closed issues per class, most first. An issue with two of the labels counts for both.
pub fn standings(p: &Progress) -> Vec<Standing> {
    let mut out: Vec<(Standing, u64)> = CLASSES
        .iter()
        .map(|c| {
            let hits: Vec<u64> = p.issues.values().filter(|i| i.labels.iter().flatten().any(|l| c.matches(l))).map(|i| i.closed_at).collect();
            (Standing { key: c.key, name: c.name, calling: c.calling, count: hits.len() }, hits.into_iter().max().unwrap_or(0))
        })
        .collect();
    // A tie goes to the class you closed an issue for most recently.
    out.sort_by(|a, b| b.0.count.cmp(&a.0.count).then(b.1.cmp(&a.1)));
    out.into_iter().map(|(s, _)| s).collect()
}

/// Your class: the label you have closed the most issues of. None before the first one.
pub fn class_of(p: &Progress) -> Option<&'static Class> {
    standings(p).into_iter().find(|s| s.count > 0).and_then(|s| CLASSES.iter().find(|c| c.key == s.key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::Counted;

    fn closed(p: &mut Progress, id: u64, at: u64, labels: &[&str]) {
        p.issues.insert(id, Counted { closed_at: at, labels: Some(labels.iter().map(|l| l.to_string()).collect()), ..Default::default() });
    }

    #[test]
    fn the_most_closed_label_is_your_class() {
        let mut p = Progress::new(0);
        assert!(class_of(&p).is_none());
        closed(&mut p, 1, 10, &["Type::Bug", "Team::Core"]);
        closed(&mut p, 2, 11, &["type::bug"]);
        closed(&mut p, 3, 12, &["Type::Enhancement"]);
        assert_eq!(class_of(&p).unwrap().name, "Verminbane");
        closed(&mut p, 4, 13, &["Type::Enhancement", "Initiative::DevEx"]);
        // 2 bugs, 2 enhancements: the most recent one wins the tie.
        assert_eq!(class_of(&p).unwrap().name, "Enchanter");
        let s = standings(&p);
        assert_eq!((s[0].key, s[0].count, s[1].key, s[1].count), ("enhancement", 2, "bug", 2));
    }

    #[test]
    fn only_the_six_labels_count_exactly() {
        let mut p = Progress::new(0);
        for (i, l) in ["Severity::Non-Critical", "Initiative::DevEx", "Phase::Ready", "Documentation-ish", "Bugfix"].iter().enumerate() {
            closed(&mut p, i as u64, 1, &[l]);
        }
        assert!(class_of(&p).is_none(), "none of these is a class label");
        closed(&mut p, 9, 2, &["Severity::Critical"]);
        assert_eq!(class_of(&p).unwrap().name, "Beacon Warden");
        closed(&mut p, 10, 3, &["Low hanging fruit"]);
        closed(&mut p, 11, 4, &["low hanging fruit"]);
        assert_eq!(class_of(&p).unwrap().name, "Orchard Forager");
        closed(&mut p, 12, 5, &["Type::Tech Debt"]);
        closed(&mut p, 13, 6, &["Type::Tech Debt"]);
        closed(&mut p, 14, 7, &["Type::Tech Debt"]);
        assert_eq!(class_of(&p).unwrap().name, "Debtreaver");
        for i in 20..24 {
            closed(&mut p, i, 8, &["Documentation"]);
        }
        assert_eq!(class_of(&p).unwrap().name, "Lorescribe");
    }

    #[test]
    fn issues_without_known_labels_do_not_count() {
        let mut p = Progress::new(0);
        p.issues.insert(1, Counted { labels: None, ..Default::default() });
        assert!(class_of(&p).is_none());
        assert!(p.missing_labels());
    }
}

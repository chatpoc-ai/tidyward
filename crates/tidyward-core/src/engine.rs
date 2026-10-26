//! Layers 2 and 3: from rules and answers to a decision for one item.
//!
//! 1. [`candidates`]: drop every rule whose exact part fails (local, free). Stop at the first
//!    rule left with no semantic part: nothing after it can win.
//! 2. [`needed`]: the semantic conditions of those candidates, the only things asked of the model.
//! 3. [`decide`]: the first candidate that applies wins. A semantic condition applies at
//!    probability >= `hit`. If the first candidate is only a weak match (`weak`..`hit`), or no
//!    rule applies, the item is left to the user.
//!
//! The model is asked "does this rule apply?" per rule, never "which rule?": a decision model
//! rarely picks "none of these", so "nothing applies" must come from the yes/no answers.

use crate::rules::{Action, Rule, exact_ok};
use crate::item::Item;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap};

pub const HIT: f64 = 0.7;
pub const WEAK: f64 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    /// An exact-only rule: decided by local code.
    Code,
    /// A rule with a semantic part: the model's answer decided.
    Model,
    /// A rule with a semantic part, answered on this Mac in local-only mode.
    Local,
    /// No rule applied.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub action: Action,
    pub rule: Option<String>,
    pub why: String,
    pub decided_by: DecidedBy,
    /// Other rules that also applied (with probability >= hit), for the plan view.
    pub also_hit: Vec<String>,
    /// How sure the deciding rule is: 1 for exact-only rules, else the model's probability.
    #[serde(default)]
    pub probability: f64,
    /// A rule matched only weakly (between `weak` and `hit`), so the item is left where it is.
    #[serde(default)]
    pub weak: bool,
}

pub fn candidates<'a>(rules: &'a [Rule], item: &Item, now: i64) -> Vec<&'a Rule> {
    let mut out = Vec::new();
    for r in rules {
        if exact_ok(r, item, now) {
            out.push(r);
            if r.cond.is_empty() {
                break;
            }
        }
    }
    out
}

pub fn needed(cands: &[&Rule]) -> BTreeSet<String> {
    cands.iter().filter(|r| !r.cond.is_empty()).map(|r| r.cond.clone()).collect()
}

/// `sem` maps condition text to the model's probability; every needed condition must be present.
pub fn decide(cands: &[&Rule], sem: &HashMap<String, f64>, hit: f64, weak: f64) -> Decision {
    let p = |r: &Rule| if r.cond.is_empty() { 1.0 } else { sem.get(&r.cond).copied().unwrap_or(0.0) };
    let first = cands.iter().find(|r| p(r) >= weak);
    let hits: Vec<String> = cands.iter().filter(|r| p(r) >= hit).map(|r| r.id.clone()).collect();
    let Some(first) = first else {
        return Decision { action: Action::Ask, rule: None, why: "no rule applies".into(), decided_by: DecidedBy::None, also_hit: hits, probability: 0.0, weak: false };
    };
    let prob = p(first);
    let also_hit = hits.into_iter().filter(|h| h != &first.id).collect();
    let decided_by = if first.cond.is_empty() { DecidedBy::Code } else { DecidedBy::Model };
    if prob < hit {
        // Unsure: leave the file where it is rather than ask. Nothing is moved on a guess, and
        // "ask" stays for files that clearly need the person (a strong match on an ask rule).
        return Decision {
            action: Action::Keep,
            rule: Some(first.id.clone()),
            why: format!("weak match {} ({prob:.2})", first.id),
            decided_by,
            also_hit,
            probability: prob,
            weak: true,
        };
    }
    Decision { action: first.action.clone(), rule: Some(first.id.clone()), why: format!("{} ({prob:.2})", first.id), decided_by, also_hit, probability: prob, weak: false }
}

/// Where the items sit, as said to the model: "Downloads folder", or just "folder".
pub fn place(folder_label: &str) -> String {
    if folder_label == "a user folder" { "folder".into() } else { format!("{folder_label} folder") }
}

/// The yes/no question for one semantic condition. `place` comes from [`place`].
pub fn condition_question(cond: &str, place: &str) -> Value {
    json!({"type": "boolean", "instructions": format!(
        "You see one item from a person's {place}, described by metadata and a masked file name. \
         Is the item this? {cond}")})
}

/// Stable question keys: one per distinct condition across all rules, in sorted order.
pub fn condition_keys(rules: &[Rule]) -> Vec<(String, String)> {
    let conds: BTreeSet<&str> = rules.iter().filter(|r| !r.cond.is_empty()).map(|r| r.cond.as_str()).collect();
    conds.into_iter().enumerate().map(|(i, c)| (format!("S{i:03}"), c.to_string())).collect()
}

pub fn questions_for(conds: &BTreeSet<String>, keys: &[(String, String)], place: &str) -> Map<String, Value> {
    keys.iter()
        .filter(|(_, c)| conds.contains(c))
        .map(|(k, c)| (k.clone(), condition_question(c, place)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tests::item;
    use crate::rules::parse;

    const RULES: &str = "
ask     | passwords or recovery codes |
trash   | a screenshot                | arrived>90 days
keep    | *                           | arrived<7 days
archive:Docs | a document             |
";

    #[test]
    fn exact_only_rule_stops_the_search() {
        let r = parse(RULES).unwrap();
        let mut i = item("x.png");
        i.added = Some(0);
        let c = candidates(&r, &i, 3 * 86_400);
        assert_eq!(c.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["R001", "R003"]);
        let d = decide(&c, &HashMap::from([("passwords or recovery codes".into(), 0.1)]), HIT, WEAK);
        assert_eq!((d.action, d.decided_by), (Action::Keep, DecidedBy::Code));
    }

    #[test]
    fn protection_first_and_unsure_stays_put() {
        let r = parse(RULES).unwrap();
        let mut i = item("x.png");
        i.added = Some(0);
        let c = candidates(&r, &i, 200 * 86_400);
        let ask = |pw: f64, shot: f64| {
            let sem = HashMap::from([
                ("passwords or recovery codes".to_string(), pw),
                ("a screenshot".to_string(), shot),
                ("a document".to_string(), 0.0),
            ]);
            decide(&c, &sem, HIT, WEAK)
        };
        assert_eq!(ask(0.9, 0.95).action, Action::Ask, "protective rule wins");
        assert_eq!(ask(0.1, 0.95).action, Action::Trash);
        let weak = ask(0.1, 0.5);
        assert_eq!((weak.action, weak.weak), (Action::Keep, true), "unsure: nothing moves");
        assert!(weak.why.starts_with("weak match R002"));
        let unsure_pw = ask(0.5, 0.95);
        assert_eq!((unsure_pw.action, unsure_pw.weak), (Action::Keep, true), "an unsure ask rule does not ask either");
        assert_eq!(ask(0.1, 0.1).why, "no rule applies");
    }

    #[test]
    fn keys_are_stable() {
        let r = parse(RULES).unwrap();
        let k = condition_keys(&r);
        assert_eq!(k.len(), 3);
        assert_eq!(k[0], ("S000".into(), "a document".into()));
    }
}

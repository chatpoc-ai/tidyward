//! Printing plans.

use tidyward::{Plan, PlanItem};
use std::collections::BTreeMap;
use tidyward_core::{Action, DecidedBy};

pub fn human(bytes: u64) -> String {
    let b = bytes as f64;
    match () {
        _ if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        _ if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        _ if b >= 1e3 => format!("{:.0} KB", b / 1e3),
        _ => format!("{bytes} B"),
    }
}

/// Group key in display order: Trash first, then ask, archive folders, keep.
fn group(a: &Action) -> (u8, String) {
    match a {
        Action::Trash => (0, "trash".into()),
        Action::Ask => (1, "ask you".into()),
        Action::Archive(d) => (2, format!("archive → {d}")),
        Action::Keep => (3, "keep".into()),
    }
}

pub fn groups(plan: &Plan) -> BTreeMap<(u8, String), Vec<&PlanItem>> {
    let mut g: BTreeMap<(u8, String), Vec<&PlanItem>> = BTreeMap::new();
    for it in &plan.items {
        g.entry(group(&it.decision.action)).or_default().push(it);
    }
    g
}

pub fn summary(plan: &Plan) {
    let parts: Vec<String> = groups(plan)
        .iter()
        .map(|((_, name), items)| format!("{name}: {} ({})", items.len(), human(items.iter().map(|i| i.size).sum())))
        .collect();
    println!("  plan: {}", parts.join(", "));
}

fn why(plan: &Plan, it: &PlanItem) -> String {
    let d = &it.decision;
    let rule_text = d.rule.as_ref().and_then(|id| plan.rules.iter().find(|r| &r.id == id)).map(|r| r.text.as_str());
    let by = match d.decided_by {
        DecidedBy::Code => "code",
        DecidedBy::Model => "model",
        DecidedBy::Local => "this Mac",
        DecidedBy::None => "-",
    };
    match (d.weak, rule_text) {
        (false, Some(t)) if d.why != "no rule applies" => format!("{} {t} [{by}]", d.why),
        _ => format!("{} [{by}]", d.why),
    }
}

pub fn full(plan: &Plan) {
    println!(
        "Plan for {} ({}), {} items. Nothing has been moved.",
        plan.folder.display(),
        chrono::DateTime::from_timestamp(plan.created, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default(),
        plan.items.len()
    );
    for ((_, name), items) in groups(plan) {
        println!("\n## {name} — {} items, {}", items.len(), human(items.iter().map(|i| i.size).sum()));
        for it in items {
            println!("  {:<48} {:>9}  {}", it.name, human(it.size), why(plan, it));
        }
    }
}

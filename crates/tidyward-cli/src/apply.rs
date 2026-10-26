//! `tidyward apply` and `tidyward undo`.
//!
//! Apply asks before anything moves: one question per archive folder, and one per item for the
//! Trash. Items that changed since the plan was made are skipped. Every operation is logged
//! with its source and destination before it is reported, so `undo` can reverse it.

use tidyward::config::{Config, expand};
use tidyward::{Outcome, Plan, PlanItem, apply_item};
use crate::show::{groups, human};
use anyhow::{Result, bail};
use std::io::{BufRead, Write};
use std::path::Path;
use tidyward_core::Action;
use tidyward_store::Store;

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok();
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

pub fn apply(cfg: &Config, store: &Store, plan: &Plan, plan_id: i64) -> Result<()> {
    let mut done = 0;
    let mut skipped = 0;
    let mut one = |it: &PlanItem| -> Result<()> {
        match apply_item(cfg, store, plan_id, it, None, None)? {
            Outcome::Done { .. } => done += 1,
            Outcome::Changed => {
                println!("  skipped (changed since the plan): {}", it.name);
                skipped += 1;
            }
            Outcome::Nothing => {}
        }
        Ok(())
    };
    for ((_, name), items) in groups(plan) {
        match &items[0].decision.action {
            Action::Keep | Action::Ask => continue,
            Action::Archive(dir) => {
                let dest = expand(&cfg.archive_root).join(dir);
                println!("\n{name}: {} items, {}", items.len(), human(items.iter().map(|i| i.size).sum()));
                for it in &items {
                    println!("  {}", it.name);
                }
                if confirm(&format!("Move these {} items to {}?", items.len(), dest.display())) {
                    for it in items {
                        one(it)?;
                    }
                }
            }
            Action::Trash => {
                println!("\n{name}: {} items, {}. Each one is confirmed on its own.", items.len(), human(items.iter().map(|i| i.size).sum()));
                for it in items {
                    if confirm(&format!("  Move to the Trash: {} ({}, {})?", it.name, human(it.size), it.decision.why)) {
                        one(it)?;
                    }
                }
            }
        }
    }
    println!("\n{done} items moved, {skipped} skipped. `tidyward undo` puts them back.");
    Ok(())
}

pub fn undo(store: &Store, id: Option<i64>) -> Result<()> {
    let ops = store.operations(100_000)?;
    if let Some(id) = id {
        match ops.iter().find(|o| o.id == id) {
            None => bail!("no such operation"),
            Some(o) if o.undone => {
                println!("  already undone: #{id}");
                return Ok(());
            }
            _ => {}
        }
    }
    let r = tidyward::undo(store, id)?;
    if r.undone.is_empty() && r.failed.is_empty() {
        bail!("nothing to undo");
    }
    for op in ops.iter().filter(|o| r.undone.contains(&o.id)) {
        println!("  #{} {} → back to {}", op.id, op.action, op.src.display());
    }
    for (id, e) in &r.failed {
        println!("  #{id} not undone: {e}");
    }
    println!("{} operations undone.", r.undone.len());
    Ok(())
}

pub fn history(store: &Store) -> Result<()> {
    for op in store.operations(50)? {
        let when = chrono::DateTime::from_timestamp(op.ts, 0)
            .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let name = Path::new(&op.src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        println!("#{:<5} {when}  {:<5} {:<40} → {}{}", op.id, op.action, name, op.dst.display(), if op.undone { "  (undone)" } else { "" });
    }
    Ok(())
}

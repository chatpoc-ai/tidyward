//! `tidyward apply` and `tidyward undo`.
//!
//! Apply asks before anything moves: one question per archive folder, and one per item for the
//! Trash. Items that changed since the plan was made are skipped. Every operation is logged
//! with its source and destination before it is reported, so `undo` can reverse it.

use crate::config::{Config, expand};
use crate::scan::{Plan, PlanItem};
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

/// The item is still what the plan saw: same size and modification time.
fn unchanged(it: &PlanItem) -> bool {
    let Ok(md) = std::fs::metadata(&it.path) else { return false };
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
    let size_ok = md.is_dir() || md.len() == it.size;
    let mtime_ok = mtime.map(|m| it.fingerprint.split(':').nth(1) == Some(&m.to_string())).unwrap_or(false);
    size_ok && mtime_ok
}

pub fn apply(cfg: &Config, store: &Store, plan: &Plan, plan_id: i64) -> Result<()> {
    let root = expand(&cfg.archive_root);
    let mut done = 0;
    let mut skipped = 0;
    for ((_, name), items) in groups(plan) {
        let action = &items[0].decision.action;
        match action {
            Action::Keep | Action::Ask => continue,
            Action::Archive(dir) => {
                let dest = root.join(dir);
                println!("\n{name}: {} items, {}", items.len(), human(items.iter().map(|i| i.size).sum()));
                for it in &items {
                    println!("  {}", it.name);
                }
                if !confirm(&format!("Move these {} items to {}?", items.len(), dest.display())) {
                    continue;
                }
                for it in items {
                    if !unchanged(it) {
                        println!("  skipped (changed since the plan): {}", it.name);
                        skipped += 1;
                        continue;
                    }
                    let to = tidyward_exec::move_into(&it.path, &dest)?;
                    store.log_operation(Some(plan_id), chrono::Utc::now().timestamp(), "move", &it.path, &to)?;
                    done += 1;
                }
            }
            Action::Trash => {
                println!("\n{name}: {} items, {}. Each one is confirmed on its own.", items.len(), human(items.iter().map(|i| i.size).sum()));
                for it in items {
                    if !confirm(&format!("  Move to the Trash: {} ({}, {})?", it.name, human(it.size), it.decision.why)) {
                        continue;
                    }
                    if !unchanged(it) {
                        println!("  skipped (changed since the plan): {}", it.name);
                        skipped += 1;
                        continue;
                    }
                    let to = tidyward_exec::trash(&it.path)?;
                    store.log_operation(Some(plan_id), chrono::Utc::now().timestamp(), "trash", &it.path, &to)?;
                    done += 1;
                }
            }
        }
    }
    println!("\n{done} items moved, {skipped} skipped. `tidyward undo` puts them back.");
    Ok(())
}

/// Undo one operation by id, or every not-yet-undone operation of the newest plan that has any.
pub fn undo(store: &Store, id: Option<i64>) -> Result<()> {
    let ops = store.operations(100_000)?;
    let todo: Vec<_> = match id {
        Some(id) => ops.into_iter().filter(|o| o.id == id).collect(),
        None => {
            let Some(last) = ops.iter().find(|o| !o.undone) else { bail!("nothing to undo") };
            let plan = last.plan_id;
            ops.into_iter().filter(|o| !o.undone && o.plan_id == plan).collect()
        }
    };
    if todo.is_empty() {
        bail!("no such operation");
    }
    let mut ok = 0;
    for op in todo {
        if op.undone {
            println!("  already undone: #{}", op.id);
            continue;
        }
        match tidyward_exec::restore(&op.dst, &op.src) {
            Ok(()) => {
                store.mark_undone(op.id)?;
                println!("  #{} {} → back to {}", op.id, op.action, op.src.display());
                ok += 1;
            }
            Err(e) => println!("  #{} not undone: {e}", op.id),
        }
    }
    println!("{ok} operations undone.");
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

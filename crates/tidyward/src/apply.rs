//! Carry out plan items and undo them.
//!
//! [`apply_item`] does one item and nothing else, so the caller decides what was confirmed:
//! the command line asks per group and per Trash item, an app uses what the person ticked.
//! Items that changed since the plan was made are skipped. Every operation is logged with its
//! source and destination before it is reported, so [`undo`] can reverse it.

use crate::config::{Config, expand};
use crate::scan::PlanItem;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tidyward_core::Action;
use tidyward_store::Store;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// Moved or put in the Trash; `op` is the operation id for undo.
    Done { op: i64, to: PathBuf },
    /// The file changed (or is gone) since the plan was made.
    Changed,
    /// The plan says keep or ask: nothing to do.
    Nothing,
}

/// The item is still what the plan saw: same size and modification time.
pub fn unchanged(it: &PlanItem) -> bool {
    let Ok(md) = std::fs::metadata(&it.path) else { return false };
    let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
    let size_ok = md.is_dir() || md.len() == it.size;
    let mtime_ok = mtime.map(|m| it.fingerprint.split(':').nth(1) == Some(&m.to_string())).unwrap_or(false);
    size_ok && mtime_ok
}

/// Carry out the plan's action for one item. `action` overrides the plan's own (an app may let
/// the person pick a different one); archive folders are relative to `archive_root`. `rename`
/// gives an archived item a new name (without extension; the extension never changes). Undo
/// puts it back under its old name, since the log holds both paths.
pub fn apply_item(cfg: &Config, store: &Store, plan_id: i64, it: &PlanItem, action: Option<&Action>, rename: Option<&str>) -> Result<Outcome> {
    let action = action.unwrap_or(&it.decision.action);
    if matches!(action, Action::Keep | Action::Ask) {
        return Ok(Outcome::Nothing);
    }
    if !unchanged(it) {
        return Ok(Outcome::Changed);
    }
    let now = chrono::Utc::now().timestamp();
    let (kind, to) = match action {
        Action::Archive(dir) => ("move", tidyward_exec::move_into_as(&it.path, &expand(&cfg.archive_root).join(dir), rename)?),
        Action::Trash => ("trash", tidyward_exec::trash(&it.path)?),
        Action::Keep | Action::Ask => unreachable!(),
    };
    let op = store.log_operation(Some(plan_id), now, kind, &it.path, &to)?;
    Ok(Outcome::Done { op, to })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UndoReport {
    /// Operation ids put back.
    pub undone: Vec<i64>,
    /// Operation id and why it could not be put back.
    pub failed: Vec<(i64, String)>,
}

/// Undo one operation by id, or every not-yet-undone operation of the newest plan that has any.
pub fn undo(store: &Store, id: Option<i64>) -> Result<UndoReport> {
    let ops = store.operations(100_000)?;
    let todo: Vec<_> = match id {
        Some(id) => ops.into_iter().filter(|o| o.id == id && !o.undone).collect(),
        None => match ops.iter().find(|o| !o.undone).map(|o| o.plan_id) {
            Some(plan) => ops.into_iter().filter(|o| !o.undone && o.plan_id == plan).collect(),
            None => Vec::new(),
        },
    };
    let mut report = UndoReport::default();
    for op in todo {
        match tidyward_exec::restore(&op.dst, &op.src) {
            Ok(()) => {
                store.mark_undone(op.id)?;
                report.undone.push(op.id);
            }
            Err(e) => report.failed.push((op.id, e.to_string())),
        }
    }
    Ok(report)
}

/// Undo every not-yet-undone operation of one plan.
pub fn undo_plan(store: &Store, plan_id: i64) -> Result<UndoReport> {
    let mut report = UndoReport::default();
    for op in store.operations(100_000)?.into_iter().filter(|o| o.plan_id == Some(plan_id) && !o.undone) {
        let r = undo(store, Some(op.id))?;
        report.undone.extend(r.undone);
        report.failed.extend(r.failed);
    }
    Ok(report)
}

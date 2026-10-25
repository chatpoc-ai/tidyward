//! The local database (SQLite). Nothing in it leaves the machine.
//!
//! - `files` / `semantic`: the file memory. What the model already said about each file, so an
//!   unchanged file is never sent again. A file counts as unchanged while its fingerprint
//!   (size + modification time + file count) and its masked name both match; the masked name
//!   is part of it so a change to the masking rules counts as a change. A file that moved keeps
//!   its memory when its content fingerprint and masked name are the same.
//!   Only answers are kept, never decisions: exact conditions change with time.
//! - `usage`: every model call with its token count.
//! - `plans` / `operations`: what was proposed and what was done, so it can be undone.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tidyward_core::{Identity, Item};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
  path TEXT PRIMARY KEY, fp TEXT NOT NULL, masked TEXT NOT NULL, qhash TEXT, size INTEGER NOT NULL,
  identity TEXT, updated INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS files_content ON files(qhash, masked);
CREATE TABLE IF NOT EXISTS semantic (
  path TEXT NOT NULL, cond TEXT NOT NULL, prob REAL NOT NULL, PRIMARY KEY (path, cond));
CREATE TABLE IF NOT EXISTS usage (
  id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, run TEXT NOT NULL, step TEXT NOT NULL, provider TEXT NOT NULL,
  items INTEGER NOT NULL, questions INTEGER NOT NULL, input_tokens INTEGER NOT NULL, ms INTEGER NOT NULL,
  full INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS runs (
  run TEXT PRIMARY KEY, ts INTEGER NOT NULL, items INTEGER NOT NULL, new_items INTEGER NOT NULL, full INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS plans (
  id INTEGER PRIMARY KEY, created INTEGER NOT NULL, folder TEXT NOT NULL, body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS operations (
  id INTEGER PRIMARY KEY, plan_id INTEGER, ts INTEGER NOT NULL, action TEXT NOT NULL,
  src TEXT NOT NULL, dst TEXT NOT NULL, undone INTEGER NOT NULL DEFAULT 0);
";

pub struct Store {
    conn: Connection,
}

/// What memory says about one item in this scan.
#[derive(Debug, Clone, Default)]
pub struct Remembered {
    pub identity: Option<Identity>,
    pub semantic: HashMap<String, f64>,
    /// Found under another path with the same content and masked name.
    pub moved_from: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRow {
    pub ts: i64,
    pub run: String,
    pub step: String,
    pub provider: String,
    pub items: i64,
    pub questions: i64,
    pub input_tokens: i64,
    pub ms: i64,
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub run: String,
    pub ts: i64,
    pub items: i64,
    pub new_items: i64,
    pub full: bool,
    pub calls: i64,
    pub input_tokens: i64,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub id: i64,
    pub plan_id: Option<i64>,
    pub ts: i64,
    pub action: String,
    pub src: PathBuf,
    pub dst: PathBuf,
    pub undone: bool,
}

fn p(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    pub fn in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    // ----- file memory -----

    /// Look the item up. Unchanged: its stored answers. Moved: the answers are copied to the new
    /// path. Changed or new: the old answers for this path are dropped.
    pub fn recall(&self, item: &Item, masked: &str) -> Result<Remembered> {
        let fp = item.fingerprint();
        let row: Option<(String, String, Option<String>)> = self
            .conn
            .query_row("SELECT fp, masked, identity FROM files WHERE path = ?1", [p(&item.path)], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        if let Some((old_fp, old_masked, identity)) = row {
            if old_fp == fp && old_masked == masked {
                return Ok(Remembered { identity: parse_identity(identity), semantic: self.semantic(&item.path)?, moved_from: None });
            }
        }
        self.forget(&item.path)?;
        if let Some(h) = &item.qhash {
            let other: Option<(String, Option<String>)> = self
                .conn
                .query_row(
                    "SELECT path, identity FROM files WHERE qhash = ?1 AND masked = ?2 AND size = ?3 AND path != ?4 LIMIT 1",
                    params![h, masked, item.size as i64, p(&item.path)],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((from, identity)) = other {
                let from = PathBuf::from(from);
                let semantic = self.semantic(&from)?;
                let rem = Remembered { identity: parse_identity(identity), semantic, moved_from: Some(from) };
                self.remember(item, masked, rem.identity.as_ref())?;
                for (c, pr) in &rem.semantic {
                    self.put_semantic(&item.path, c, *pr)?;
                }
                return Ok(rem);
            }
        }
        Ok(Remembered::default())
    }

    fn semantic(&self, path: &Path) -> Result<HashMap<String, f64>> {
        let mut st = self.conn.prepare_cached("SELECT cond, prob FROM semantic WHERE path = ?1")?;
        let rows = st.query_map([p(path)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn forget(&self, path: &Path) -> Result<()> {
        self.conn.execute("DELETE FROM files WHERE path = ?1", [p(path)])?;
        self.conn.execute("DELETE FROM semantic WHERE path = ?1", [p(path)])?;
        Ok(())
    }

    pub fn remember(&self, item: &Item, masked: &str, identity: Option<&Identity>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files (path, fp, masked, qhash, size, identity, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%s','now'))
             ON CONFLICT(path) DO UPDATE SET fp = excluded.fp, masked = excluded.masked, qhash = excluded.qhash,
               size = excluded.size, identity = excluded.identity, updated = excluded.updated",
            params![p(&item.path), item.fingerprint(), masked, item.qhash, item.size as i64, identity.map(|i| serde_json::to_string(i).unwrap())],
        )?;
        Ok(())
    }

    pub fn put_semantic(&self, path: &Path, cond: &str, prob: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO semantic (path, cond, prob) VALUES (?1, ?2, ?3) ON CONFLICT(path, cond) DO UPDATE SET prob = excluded.prob",
            params![p(path), cond, prob],
        )?;
        Ok(())
    }

    /// Drop memory for files that are gone from `folder` (direct children only).
    pub fn prune(&self, folder: &Path, seen: &[PathBuf]) -> Result<usize> {
        let mut st = self.conn.prepare("SELECT path FROM files")?;
        let all: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let seen: std::collections::HashSet<String> = seen.iter().map(|s| p(s)).collect();
        let mut n = 0;
        for path in all {
            let pb = PathBuf::from(&path);
            if pb.parent() == Some(folder) && !seen.contains(&path) {
                self.forget(&pb)?;
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn clear_memory(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM files; DELETE FROM semantic;")?;
        Ok(())
    }

    pub fn begin(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    // ----- token ledger -----

    pub fn log_usage(&self, u: &UsageRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO usage (ts, run, step, provider, items, questions, input_tokens, ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![u.ts, u.run, u.step, u.provider, u.items, u.questions, u.input_tokens, u.ms],
        )?;
        Ok(())
    }

    pub fn log_run(&self, run: &str, ts: i64, items: i64, new_items: i64, full: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO runs (run, ts, items, new_items, full) VALUES (?1,?2,?3,?4,?5)",
            params![run, ts, items, new_items, full as i64],
        )?;
        Ok(())
    }

    /// Oldest first.
    pub fn runs(&self) -> Result<Vec<RunSummary>> {
        let mut st = self.conn.prepare(
            "SELECT r.run, r.ts, r.items, r.new_items, r.full, COUNT(u.id), COALESCE(SUM(u.input_tokens), 0)
             FROM runs r LEFT JOIN usage u ON u.run = r.run GROUP BY r.run ORDER BY r.ts",
        )?;
        let rows = st.query_map([], |r| {
            Ok(RunSummary {
                run: r.get(0)?,
                ts: r.get(1)?,
                items: r.get(2)?,
                new_items: r.get(3)?,
                full: r.get::<_, i64>(4)? != 0,
                calls: r.get(5)?,
                input_tokens: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn total_tokens(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COALESCE(SUM(input_tokens), 0) FROM usage", [], |r| r.get(0))?)
    }

    // ----- plans and operations -----

    pub fn save_plan(&self, created: i64, folder: &Path, body: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO plans (created, folder, body) VALUES (?1, ?2, ?3)", params![created, p(folder), body])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// (id, created, folder, body) of the newest plan.
    pub fn latest_plan(&self) -> Result<Option<(i64, i64, PathBuf, String)>> {
        Ok(self
            .conn
            .query_row("SELECT id, created, folder, body FROM plans ORDER BY id DESC LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, PathBuf::from(r.get::<_, String>(2)?), r.get(3)?))
            })
            .optional()?)
    }

    pub fn log_operation(&self, plan_id: Option<i64>, ts: i64, action: &str, src: &Path, dst: &Path) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO operations (plan_id, ts, action, src, dst) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![plan_id, ts, action, p(src), p(dst)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn mark_undone(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE operations SET undone = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Newest first.
    pub fn operations(&self, limit: i64) -> Result<Vec<Operation>> {
        let mut st = self.conn.prepare(
            "SELECT id, plan_id, ts, action, src, dst, undone FROM operations ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = st.query_map([limit], |r| {
            Ok(Operation {
                id: r.get(0)?,
                plan_id: r.get(1)?,
                ts: r.get(2)?,
                action: r.get(3)?,
                src: PathBuf::from(r.get::<_, String>(4)?),
                dst: PathBuf::from(r.get::<_, String>(5)?),
                undone: r.get::<_, i64>(6)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn parse_identity(s: Option<String>) -> Option<Identity> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidyward_core::{Kind, Purpose};

    fn item(path: &str, size: u64, mtime: i64) -> Item {
        Item {
            path: PathBuf::from(path),
            name: Path::new(path).file_name().unwrap().to_string_lossy().into(),
            is_dir: false,
            kind: Kind::Document,
            ext: ".pdf".into(),
            size,
            files_inside: 1,
            mtime,
            added: None,
            last_used: None,
            domain: None,
            camera: None,
            screen_capture: false,
            width: None,
            height: None,
            creator: None,
            pages: None,
            content: None,
            qhash: Some("h1".into()),
            dupes: 0,
            redundant_copy: false,
            app_installed: None,
        }
    }

    fn id() -> Identity {
        Identity { purpose: Purpose::Finance, confidence: 0.9, source: "model".into(), reason: "model".into() }
    }

    #[test]
    fn memory_lifecycle() {
        let s = Store::in_memory().unwrap();
        let a = item("/f/a.pdf", 10, 100);
        assert!(s.recall(&a, "m").unwrap().identity.is_none(), "new file");
        s.remember(&a, "m", Some(&id())).unwrap();
        s.put_semantic(&a.path, "an invoice", 0.8).unwrap();

        let r = s.recall(&a, "m").unwrap();
        assert_eq!((r.identity, r.semantic.get("an invoice").copied()), (Some(id()), Some(0.8)), "unchanged");

        assert!(s.recall(&a, "other mask").unwrap().identity.is_none(), "masking changed counts as changed");
        s.remember(&a, "m", Some(&id())).unwrap();
        s.put_semantic(&a.path, "an invoice", 0.8).unwrap();

        let moved = item("/g/a.pdf", 10, 100);
        let r = s.recall(&moved, "m").unwrap();
        assert_eq!(r.moved_from, Some(PathBuf::from("/f/a.pdf")));
        assert_eq!(r.semantic.get("an invoice").copied(), Some(0.8), "moved keeps answers");

        let edited = item("/f/a.pdf", 11, 200);
        let r = s.recall(&edited, "m").unwrap();
        assert!(r.identity.is_none() && r.moved_from.is_none(), "changed file starts over");
    }

    #[test]
    fn prune_only_direct_children() {
        let s = Store::in_memory().unwrap();
        for path in ["/f/a", "/f/b", "/f/sub/c", "/g/d"] {
            s.remember(&item(path, 1, 1), "m", None).unwrap();
        }
        assert_eq!(s.prune(Path::new("/f"), &[PathBuf::from("/f/a")]).unwrap(), 1);
    }

    #[test]
    fn ledger_and_operations() {
        let s = Store::in_memory().unwrap();
        s.log_run("r1", 1, 5, 5, true).unwrap();
        let u = UsageRow { ts: 1, run: "r1".into(), step: "identify".into(), provider: "typesafe".into(), items: 5, questions: 5, input_tokens: 1000, ms: 80 };
        s.log_usage(&u).unwrap();
        s.log_run("r2", 2, 5, 0, false).unwrap();
        let runs = s.runs().unwrap();
        assert_eq!((runs[0].calls, runs[0].input_tokens, runs[1].calls), (1, 1000, 0));
        let op = s.log_operation(None, 3, "trash", Path::new("/a"), Path::new("/t/a")).unwrap();
        s.mark_undone(op).unwrap();
        assert!(s.operations(10).unwrap()[0].undone);
    }
}

//! `tidyward scan`: scan → identify → rules → plan, with the file memory so unchanged files are
//! not sent again. Nothing is moved here; the plan is saved for `tidyward apply`.

use crate::config::{Config, Paths, api_key, expand};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tidyward_core::engine::{self, Decision};
use tidyward_core::identify::{self, Identity, Purpose};
use tidyward_core::state::{self, StateOptions};
use tidyward_core::{Item, Rule, mask, rules};
use tidyward_jev::{Answer, CallStat, Client, Job};
use tidyward_store::{Store, UsageRow};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub path: PathBuf,
    pub name: String,
    pub masked: String,
    pub size: u64,
    pub fingerprint: String,
    pub identity: Identity,
    pub decision: Decision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRule {
    pub id: String,
    pub action: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub created: i64,
    pub folder: PathBuf,
    pub run: String,
    pub rules: Vec<PlanRule>,
    pub items: Vec<PlanItem>,
}

pub struct Totals {
    pub items: usize,
    pub unchanged: usize,
    pub moved: usize,
    pub local: usize,
    pub identify_items: usize,
    pub rule_questions: usize,
    pub reused_answers: usize,
    pub calls: usize,
    pub tokens: u64,
}

const IDENTIFY_NOTE: &str = "Each key below is one item from a person's {place}, described by metadata only and a masked file name.";
const RULES_NOTE: &str = "Each key below is one item from a person's {place}, described by metadata and a masked file name. \
                          Answer each question only about the item it names.";

pub fn load_rules(paths: &Paths) -> Result<Vec<Rule>> {
    let path = paths.rules();
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no rules at {}; run `tidyward init`", path.display()))?;
    Ok(rules::parse(&text)?)
}

fn log_calls(store: &Store, run: &str, step: &str, provider: &str, stats: &[CallStat]) -> Result<()> {
    for s in stats {
        store.log_usage(&UsageRow {
            ts: chrono::Utc::now().timestamp(),
            run: run.into(),
            step: step.into(),
            provider: provider.into(),
            items: s.items as i64,
            questions: s.questions as i64,
            input_tokens: s.input_tokens as i64,
            ms: s.ms as i64,
        })?;
    }
    Ok(())
}

/// A client is only created when something needs asking, so offline runs need no key.
struct LazyClient<'a> {
    cfg: &'a Config,
    client: Option<Client>,
}

impl LazyClient<'_> {
    fn get(&mut self) -> Result<&Client> {
        if self.client.is_none() {
            let p = self.cfg.provider()?;
            self.client = Some(Client::new(p, api_key(p)?));
        }
        Ok(self.client.as_ref().unwrap())
    }
}

pub fn scan_folder(cfg: &Config, paths: &Paths, store: &Store, folder: &Path, full: bool, run: &str) -> Result<(Plan, Totals)> {
    let rules = load_rules(paths)?;
    let now = chrono::Utc::now().timestamp();
    let today = chrono::Local::now().date_naive();
    let items: Vec<Item> =
        tidyward_platform::scan_folder(folder, &tidyward_platform::ScanOptions { magika: cfg.magika })?;
    let folder_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let label = state::folder_label(&folder_name);
    let place = engine::place(label);
    let opts = StateOptions { folder: &folder_name, send_domain: cfg.send_domain, now };
    let mut lazy = LazyClient { cfg, client: None };
    let provider = cfg.provider()?.name();

    let mut t = Totals { items: items.len(), unchanged: 0, moved: 0, local: 0, identify_items: 0, rule_questions: 0, reused_answers: 0, calls: 0, tokens: 0 };
    let masked: Vec<String> = items.iter().map(|i| mask::mask(&i.name, today)).collect();
    let keys: Vec<String> = (1..=items.len()).map(|n| format!("F{n:04}")).collect();
    let states: Vec<Map<String, Value>> = items.iter().zip(&masked).map(|(i, m)| state::build(i, m, &opts)).collect();

    store.begin()?;
    // memory
    let mut remembered = Vec::new();
    for (item, m) in items.iter().zip(&masked) {
        let r = if full { store.forget(&item.path)?; Default::default() } else { store.recall(item, m)? };
        if r.identity.is_some() {
            if r.moved_from.is_some() { t.moved += 1 } else { t.unchanged += 1 }
        }
        remembered.push(r);
    }

    // layer 1: identify
    let mut identities: Vec<Option<Identity>> = remembered.iter().map(|r| r.identity.clone()).collect();
    let mut jobs = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if identities[i].is_some() {
            continue;
        }
        match identify::local_identify(item) {
            Some(id) => {
                identities[i] = Some(id);
                t.local += 1;
            }
            None => {
                let q: Map<String, Value> = serde_json::from_value(identify::purpose_question())?;
                jobs.push(Job { key: keys[i].clone(), state: states[i].clone(), questions: q });
            }
        }
    }
    if !jobs.is_empty() {
        let client = lazy.get()?;
        let (answers, stats) = client.ask_batched(&jobs, cfg.batch, &IDENTIFY_NOTE.replace("{place}", &place))?;
        log_calls(store, run, "identify", provider, &stats)?;
        t.calls += stats.len();
        t.tokens += stats.iter().map(|s| s.input_tokens).sum::<u64>();
        t.identify_items = jobs.len();
        for (i, k) in keys.iter().enumerate() {
            if let Some(Answer::Choice { choice, confidence }) = answers.get(k).and_then(|a| a.get("purpose")) {
                let purpose = Purpose::from_key(choice).unwrap_or(Purpose::Other);
                identities[i] = Some(Identity { purpose, confidence: *confidence, source: "model".into(), reason: "model".into() });
            }
        }
    }
    let identities: Vec<Identity> = identities.into_iter().map(|i| i.expect("every item identified")).collect();
    for ((item, m), id) in items.iter().zip(&masked).zip(&identities) {
        store.remember(item, m, Some(id))?;
    }

    // layers 2 and 3: exact conditions locally, then only the semantic ones still needed
    let cond_keys = engine::condition_keys(&rules);
    let key_of: HashMap<&str, &str> = cond_keys.iter().map(|(k, c)| (k.as_str(), c.as_str())).collect();
    let mut cands_all = Vec::new();
    let mut jobs = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let cands = engine::candidates(&rules, item, now);
        let mut need = engine::needed(&cands);
        t.reused_answers += need.iter().filter(|c| remembered[i].semantic.contains_key(*c)).count();
        need.retain(|c| !remembered[i].semantic.contains_key(c));
        if !need.is_empty() {
            let mut st = states[i].clone();
            st.insert("what it is (from a first pass)".into(), json!(identify::describe(&identities[i])));
            t.rule_questions += need.len();
            jobs.push(Job { key: keys[i].clone(), state: st, questions: engine::questions_for(&need, &cond_keys, &place) });
        }
        cands_all.push(cands);
    }
    let mut semantic: Vec<HashMap<String, f64>> = remembered.iter().map(|r| r.semantic.clone()).collect();
    if !jobs.is_empty() {
        let client = lazy.get()?;
        let (answers, stats) = client.ask_batched(&jobs, cfg.batch, &RULES_NOTE.replace("{place}", &place))?;
        log_calls(store, run, "rules", provider, &stats)?;
        t.calls += stats.len();
        t.tokens += stats.iter().map(|s| s.input_tokens).sum::<u64>();
        for (i, k) in keys.iter().enumerate() {
            for (qk, a) in answers.get(k).into_iter().flatten() {
                if let Some(cond) = key_of.get(qk.as_str()) {
                    semantic[i].insert(cond.to_string(), a.probability());
                    store.put_semantic(&items[i].path, cond, a.probability())?;
                }
            }
        }
    }
    let seen: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
    store.prune(folder, &seen)?;
    store.commit()?;

    let plan_items = items
        .iter()
        .enumerate()
        .map(|(i, item)| PlanItem {
            path: item.path.clone(),
            name: item.name.clone(),
            masked: masked[i].clone(),
            size: item.size,
            fingerprint: item.fingerprint(),
            identity: identities[i].clone(),
            decision: engine::decide(&cands_all[i], &semantic[i], cfg.hit, cfg.weak),
        })
        .collect();
    let plan = Plan {
        created: now,
        folder: folder.to_path_buf(),
        run: run.into(),
        rules: rules.iter().map(|r| PlanRule { id: r.id.clone(), action: r.action.to_string(), text: r.describe() }).collect(),
        items: plan_items,
    };
    Ok((plan, t))
}

pub fn run(cfg: &Config, paths: &Paths, store: &Store, folders: &[String], full: bool) -> Result<Vec<(Plan, i64)>> {
    let run = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut out = Vec::new();
    for f in folders {
        let folder = expand(f);
        let key = format!("{run} {}", folder.display());
        let (plan, t) = scan_folder(cfg, paths, store, &folder, full, &key)?;
        let new_items = t.items - t.unchanged - t.moved;
        store.log_run(&key, plan.created, t.items as i64, new_items as i64, full)?;
        let id = store.save_plan(plan.created, &folder, &serde_json::to_string(&plan)?)?;
        println!(
            "{}: {} items ({} unchanged, {} moved, {} new or changed)",
            folder.display(),
            t.items,
            t.unchanged,
            t.moved,
            new_items
        );
        println!(
            "  identify: {} locally, {} by the model;  rules: {} questions asked, {} answers from memory",
            t.local, t.identify_items, t.rule_questions, t.reused_answers
        );
        println!(
            "  model: {} calls, {} input tokens (${:.4})",
            t.calls,
            t.tokens,
            t.tokens as f64 / 1e6 * cfg.price_per_million
        );
        crate::show::summary(&plan);
        out.push((plan, id));
    }
    Ok(out)
}

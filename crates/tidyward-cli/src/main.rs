//! `tidyward`: a private AI housekeeper for your computer. See docs/design.md.

mod apply;
mod show;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use tidyward::config::{self, Config, KeySource, Paths};
use tidyward::{Plan, scan};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use tidyward_store::Store;

#[derive(Parser)]
#[command(name = "tidyward", version, about = "A private AI housekeeper: it suggests, you confirm.")]
struct Cli {
    /// Data folder (config, rules, database). Default: $TIDYWARD_HOME or the platform data folder.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create config.toml and rules.txt in the data folder.
    Init,
    /// Scan, identify, apply your rules and save a plan. Nothing is moved.
    Scan {
        /// Folders to scan instead of the ones in config.toml.
        folders: Vec<String>,
        /// Ignore the file memory and ask about every file again.
        #[arg(long)]
        full: bool,
    },
    /// Show the latest plan.
    Plan,
    /// Carry out the latest plan, asking before each group (and each Trash item).
    Apply,
    /// Undo the last applied plan, or one operation by its number.
    Undo { id: Option<i64> },
    /// List recent operations.
    History,
    /// Check the rules file and show which rules the latest plan used.
    Rules,
    /// Tokens and cost: per run, in total, and an estimate per week.
    Usage,
    /// A short report on the latest plan.
    Report,
    /// Store or check the provider key (kept in the system keychain).
    Key {
        #[command(subcommand)]
        cmd: KeyCmd,
    },
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Read the key from standard input and store it in the keychain.
    Set,
    /// Say where the key would come from, without printing it.
    Status,
}

fn latest_plan(store: &Store) -> Result<(i64, Plan)> {
    scan::latest_plan(store)?.context("no plan yet: run `tidyward scan`")
}

fn usage(cfg: &Config, store: &Store) -> Result<()> {
    let price = |t: i64| t as f64 / 1e6 * cfg.price_per_million;
    let runs = store.runs()?;
    println!("price: ${}/M input tokens (output is free)\n", cfg.price_per_million);
    println!("{:<60} {:>6} {:>5} {:>6} {:>10} {:>9}", "run", "items", "new", "calls", "tokens", "cost");
    for r in runs.iter().rev().take(15).rev() {
        let name = if r.full { format!("{} (full)", r.run) } else { r.run.clone() };
        println!("{name:<60} {:>6} {:>5} {:>6} {:>10} {:>9.4}", r.items, r.new_items, r.calls, r.input_tokens, price(r.input_tokens));
    }
    let total = store.total_tokens()?;
    println!("\ntotal: {total} tokens = ${:.4}", price(total));
    // tokens per new item, from runs that asked about new items
    let (tok, new): (i64, i64) =
        runs.iter().filter(|r| r.new_items > 0).fold((0, 0), |(t, n), r| (t + r.input_tokens, n + r.new_items));
    if new > 0 {
        let per = tok as f64 / new as f64;
        println!("about {per:.0} tokens per new or changed file; unchanged files cost nothing");
        for n in [50.0, 300.0, 1000.0] {
            println!("  {n:>5} new files a week: ${:.4}/month", per * n / 1e6 * cfg.price_per_million * 52.0 / 12.0);
        }
    }
    Ok(())
}

fn report(plan: &Plan) {
    use tidyward_core::Action;
    let total: u64 = plan.items.iter().map(|i| i.size).sum();
    println!("Tidyward report: {} ({} items, {})", plan.folder.display(), plan.items.len(), show::human(total));
    let mut by_purpose: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    for it in &plan.items {
        let e = by_purpose.entry(it.identity.purpose.key()).or_default();
        e.0 += 1;
        e.1 += it.size;
    }
    println!("\nWhat is in it:");
    for (p, (n, s)) in &by_purpose {
        println!("  {p:<16} {n:>5}  {}", show::human(*s));
    }
    let trash: u64 = plan.items.iter().filter(|i| i.decision.action == Action::Trash).map(|i| i.size).sum();
    let ask = plan.items.iter().filter(|i| i.decision.action == Action::Ask).count();
    println!("\nProposed: {} to the Trash ({}), {} left to you.", plan.items.iter().filter(|i| i.decision.action == Action::Trash).count(), show::human(trash), ask);
    let mut big: Vec<_> = plan.items.iter().collect();
    big.sort_by_key(|i| std::cmp::Reverse(i.size));
    println!("\nLargest items:");
    for it in big.iter().take(5) {
        println!("  {:>9}  {}  → {}", show::human(it.size), it.name, it.decision.action);
    }
}

fn rules_check(paths: &Paths, store: &Store) -> Result<()> {
    let rules = scan::load_rules(paths).context("run `tidyward init` first")?;
    let used: HashSet<String> = match latest_plan(store) {
        Ok((_, p)) => p.items.iter().filter_map(|i| i.decision.rule.clone()).collect(),
        Err(_) => HashSet::new(),
    };
    println!("{} rules in {} — OK", rules.len(), paths.rules().display());
    for r in &rules {
        let mark = if used.is_empty() || used.contains(&r.id) { " " } else { "·" };
        println!("{mark} {} line {:>3}  {:<28} {}", r.id, r.line, r.action.to_string(), r.describe());
    }
    if !used.is_empty() {
        println!("\n· = not used by the latest plan");
    }
    Ok(())
}

fn key(cfg: &Config, cmd: KeyCmd) -> Result<()> {
    let p = cfg.provider()?;
    match cmd {
        KeyCmd::Set => {
            print!("Paste your {} key and press Return: ", p.name());
            std::io::stdout().flush().ok();
            let mut k = String::new();
            std::io::stdin().lock().read_line(&mut k)?;
            let k = k.trim();
            if k.is_empty() {
                bail!("no key given");
            }
            config::set_key(p, k)?;
            println!("Stored in the keychain (service \"tidyward\", account \"{}\").", p.name());
        }
        KeyCmd::Status => {
            match config::key_source(p) {
                KeySource::Env(v) => println!("{}: key from the environment variable {v}", p.name()),
                KeySource::Keychain => println!("{}: key from the keychain", p.name()),
                KeySource::Missing => println!("{}: no key. Run `tidyward key set`, or set {}.", p.name(), p.key_env().join(" or ")),
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::resolve(cli.home.as_deref())?;
    let cfg = Config::load(&paths)?;
    if let Cmd::Init = cli.cmd {
        let made = config::init(&paths)?;
        if made.is_empty() {
            println!("Already set up in {}", paths.home.display());
        }
        for m in made {
            println!("Created {}", m.display());
        }
        println!("\nrules.txt starts with a short, careful set of rules; change it to your habits any time.");
        println!("Next: `tidyward key set`, then `tidyward scan` and `tidyward plan` (nothing is moved until `tidyward apply`).");
        return Ok(());
    }
    if let Cmd::Key { cmd } = cli.cmd {
        return key(&cfg, cmd);
    }
    let store = Store::open(&paths.db())?;
    match cli.cmd {
        Cmd::Scan { folders, full } => {
            let folders = if folders.is_empty() { cfg.folders.clone() } else { folders };
            for r in scan::run(&cfg, &paths, &store, &folders, full).context("run `tidyward init` first if this is new")? {
                let t = &r.totals;
                println!(
                    "{}: {} items ({} unchanged, {} moved, {} new or changed)",
                    r.plan.folder.display(),
                    t.items,
                    t.unchanged,
                    t.moved,
                    t.new_items()
                );
                println!(
                    "  identify: {} locally, {} by the model;  rules: {} questions asked, {} answers from memory",
                    t.local, t.identify_items, t.rule_questions, t.reused_answers
                );
                println!("  model: {} calls, {} input tokens (${:.4})", t.calls, t.tokens, t.tokens as f64 / 1e6 * cfg.price_per_million);
                show::summary(&r.plan);
            }
            println!("\n`tidyward plan` shows the details; `tidyward apply` carries it out after asking.");
        }
        Cmd::Plan => show::full(&latest_plan(&store)?.1),
        Cmd::Apply => {
            let (id, plan) = latest_plan(&store)?;
            apply::apply(&cfg, &store, &plan, id)?;
        }
        Cmd::Undo { id } => apply::undo(&store, id)?,
        Cmd::History => apply::history(&store)?,
        Cmd::Rules => rules_check(&paths, &store)?,
        Cmd::Usage => usage(&cfg, &store)?,
        Cmd::Report => report(&latest_plan(&store)?.1),
        Cmd::Init | Cmd::Key { .. } => unreachable!(),
    }
    Ok(())
}

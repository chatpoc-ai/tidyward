//! The rules file: one rule per line, `action | semantic condition | exact conditions`.
//! Order is priority; the first rule that applies wins.
//!
//! Exact conditions are checked here in code, never by the model: comparing "3-6 months ago"
//! with "older than 90 days" is unreliable for a model and trivial for code. Every keyword has
//! an English and a Chinese form:
//!
//! | English                 | 中文           |
//! |-------------------------|----------------|
//! | `arrived>N days`        | `到达>N天`      |
//! | `opened>N days`         | `打开>N天`      |
//! | `never opened`          | `从未打开`      |
//! | `size>N MB`             | `大小>NMB`      |
//! | `extension=.a/.b`       | `扩展名=.a/.b`  |
//! | `has duplicate`         | `有副本`        |
//! | `app installed`         | `App已安装`     |
//! | `name starts with X`    | `名称以X开头`    |
//! | `name contains X`       | `名称含X`       |
//! | `from example.com`      | `来自example.com` |
//!
//! `<` works wherever `>` does. "opened>N days" also holds for files never opened.
//! "has duplicate" holds only for the extra copies, never for the original of a group.
//! "from X" holds when the file was downloaded from site X or one of its subdomains.

use crate::item::Item;
use crate::words::DAY;
use fancy_regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Keep,
    /// Folder relative to the archive root (the home folder by default).
    Archive(String),
    Trash,
    Ask,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Action::Keep => write!(f, "keep"),
            Action::Archive(d) => write!(f, "archive:{d}"),
            Action::Trash => write!(f, "trash"),
            Action::Ask => write!(f, "ask"),
        }
    }
}

impl Action {
    pub fn parse(s: &str) -> Option<Action> {
        match s {
            "keep" => Some(Action::Keep),
            "trash" => Some(Action::Trash),
            "ask" => Some(Action::Ask),
            _ => s.strip_prefix("archive:").filter(|d| !d.trim().is_empty()).map(|d| Action::Archive(d.trim().into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    Arrived { more: bool, days: f64 },
    Opened { more: bool, days: f64 },
    NeverOpened,
    Size { more: bool, mb: f64 },
    Extension(Vec<String>),
    HasDuplicate,
    AppInstalled,
    NameStarts(String),
    NameContains(String),
    /// Downloaded from this site (or a subdomain of it).
    Domain(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// R001, R002, ... in file order.
    pub id: String,
    pub line: usize,
    pub action: Action,
    /// Plain-language condition for the model; empty means "any".
    pub cond: String,
    pub exact: Vec<Filter>,
    /// The exact part as written, for showing in plans.
    pub exact_text: Vec<String>,
}

impl Rule {
    pub fn describe(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if !self.cond.is_empty() {
            parts.push(&self.cond);
        }
        parts.extend(self.exact_text.iter().map(String::as_str));
        if parts.is_empty() {
            return "(any)".into();
        }
        parts.join(" + ")
    }
}

static AGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(到达|arrived|打开|opened)\s*([<>])\s*(\d+(?:\.\d+)?)\s*(天|days?)?$").unwrap()
});
static SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(大小|size)\s*([<>])\s*(\d+(?:\.\d+)?)\s*MB$").unwrap());
static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(扩展名|extension)\s*=\s*(\S+)$").unwrap());
static STARTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:名称以(\S+)开头|name starts with\s+(\S+))$").unwrap());
static CONTAINS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:名称含(\S+)|name contains\s+(\S+))$").unwrap());
static DOMAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:来自\s*([a-z0-9.-]+\.[a-z]{2,})|from\s+([a-z0-9.-]+\.[a-z]{2,}))$").unwrap());

pub fn parse_filter(f: &str) -> Option<Filter> {
    let lower = f.to_lowercase();
    match lower.as_str() {
        "从未打开" | "never opened" => return Some(Filter::NeverOpened),
        "有副本" | "has duplicate" => return Some(Filter::HasDuplicate),
        "app已安装" | "app installed" => return Some(Filter::AppInstalled),
        _ => {}
    }
    if let Ok(Some(c)) = AGE.captures(f) {
        let more = &c[2] == ">";
        let days: f64 = c[3].parse().ok()?;
        return Some(match &c[1].to_lowercase()[..] {
            "到达" | "arrived" => Filter::Arrived { more, days },
            _ => Filter::Opened { more, days },
        });
    }
    if let Ok(Some(c)) = SIZE.captures(f) {
        return Some(Filter::Size { more: &c[2] == ">", mb: c[3].parse().ok()? });
    }
    if let Ok(Some(c)) = EXT.captures(f) {
        return Some(Filter::Extension(c[2].split('/').map(|e| e.to_lowercase()).collect()));
    }
    if let Ok(Some(c)) = STARTS.captures(f) {
        return Some(Filter::NameStarts(c.get(1).or(c.get(2))?.as_str().to_lowercase()));
    }
    if let Ok(Some(c)) = CONTAINS.captures(f) {
        return Some(Filter::NameContains(c.get(1).or(c.get(2))?.as_str().to_lowercase()));
    }
    if let Ok(Some(c)) = DOMAIN.captures(f) {
        return Some(Filter::Domain(c.get(1).or(c.get(2))?.as_str().to_lowercase().trim_start_matches("www.").to_string()));
    }
    None
}

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "rules line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(text: &str) -> Result<Vec<Rule>, ParseError> {
    let mut rules = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |message: String| ParseError { line: i + 1, message };
        let parts: Vec<&str> = line.split('|').map(str::trim).collect();
        if parts.len() < 2 || parts.len() > 3 {
            return Err(err(format!("expected 'action | condition | exact conditions', got: {line}")));
        }
        let action = Action::parse(parts[0]).ok_or_else(|| err(format!("unknown action '{}'", parts[0])))?;
        if parts[1].is_empty() {
            return Err(err("empty condition; write * for any".into()));
        }
        let cond = if parts[1] == "*" { String::new() } else { parts[1].to_string() };
        let exact_text: Vec<String> = parts
            .get(2)
            .map(|e| e.split([',', '，']).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
            .unwrap_or_default();
        let exact = exact_text
            .iter()
            .map(|f| parse_filter(f).ok_or_else(|| err(format!("unknown exact condition '{f}'"))))
            .collect::<Result<Vec<_>, _>>()?;
        rules.push(Rule { id: format!("R{:03}", rules.len() + 1), line: i + 1, action, cond, exact, exact_text });
    }
    Ok(rules)
}

/// Whether every exact condition of the rule holds for the item at time `now`.
pub fn exact_ok(rule: &Rule, item: &Item, now: i64) -> bool {
    let days = |ts: Option<i64>| ts.map(|t| (now - t) as f64 / DAY as f64).unwrap_or(f64::INFINITY);
    let cmp = |more: bool, x: f64, lim: f64| if more { x > lim } else { x < lim };
    let name = item.name.to_lowercase();
    rule.exact.iter().all(|f| match f {
        Filter::Arrived { more, days: d } => cmp(*more, days(Some(item.arrived())), *d),
        // never opened counts as older than any N, newer than none
        Filter::Opened { more, days: d } => cmp(*more, days(item.last_used), *d),
        Filter::NeverOpened => item.last_used.is_none(),
        Filter::Size { more, mb } => cmp(*more, item.size as f64 / 1e6, *mb),
        Filter::Extension(exts) => exts.contains(&item.ext),
        Filter::HasDuplicate => item.redundant_copy,
        Filter::AppInstalled => item.app_installed == Some(true),
        // checked on the real name, here on this machine
        Filter::NameStarts(p) => name.starts_with(p.as_str()),
        Filter::NameContains(p) => name.contains(p.as_str()),
        Filter::Domain(d) => item.domain.as_deref().is_some_and(|x| {
            let x = x.to_lowercase();
            x == *d || x.ends_with(&format!(".{d}"))
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tests::item;

    const RULES: &str = "
# comment
ask                        | passwords or recovery codes |
trash                      | a screenshot | 到达>90天, 从未打开
trash                      | *            | arrived>90 days, never opened, extension=.png
archive:Pictures/Camera    | *            | name starts with IMG_, 扩展名=.heic/.jpg
keep                       | *            | opened<7 days
";

    #[test]
    fn parses_both_languages() {
        let r = parse(RULES).unwrap();
        assert_eq!(r.len(), 5);
        assert_eq!(r[0].id, "R001");
        assert_eq!(r[1].exact, r[2].exact[..2].to_vec());
        assert_eq!(r[3].action, Action::Archive("Pictures/Camera".into()));
        assert_eq!(r[3].exact[1], Filter::Extension(vec![".heic".into(), ".jpg".into()]));
        assert_eq!(r[4].cond, "");
    }

    #[test]
    fn rejects_bad_lines() {
        assert_eq!(parse("delete | x |").unwrap_err().line, 1);
        assert!(parse("trash | x | older than a year").is_err());
        assert!(parse("trash |  |").is_err());
    }

    #[test]
    fn exact_conditions() {
        let r = parse(RULES).unwrap();
        let now = 200 * DAY;
        let mut i = item("Screenshot.png");
        i.added = Some(0);
        assert!(exact_ok(&r[2], &i, now));
        i.last_used = Some(150 * DAY);
        assert!(!exact_ok(&r[2], &i, now), "opened once: not 'never opened'");
        assert!(!exact_ok(&r[4], &i, now), "opened 50 days ago is not within 7");
        let mut c = item("img_0001.HEIC");
        c.ext = ".heic".into();
        assert!(exact_ok(&r[3], &c, now), "name and extension checks ignore case");
    }

    #[test]
    fn source_site() {
        let r = parse("archive:Bills | * | from mybank.com\ntrash | * | 来自 example.org").unwrap();
        assert_eq!(r[0].exact[0], Filter::Domain("mybank.com".into()));
        assert_eq!(r[1].exact[0], Filter::Domain("example.org".into()));
        let mut i = item("statement.pdf");
        i.domain = Some("secure.mybank.com".into());
        assert!(exact_ok(&r[0], &i, 0), "subdomains count");
        i.domain = Some("notmybank.com".into());
        assert!(!exact_ok(&r[0], &i, 0), "a different site does not");
        i.domain = None;
        assert!(!exact_ok(&r[0], &i, 0));
    }

    #[test]
    fn duplicates_spare_the_original() {
        let r = parse("trash | * | has duplicate").unwrap();
        let mut i = item("a.pdf");
        i.dupes = 1;
        assert!(!exact_ok(&r[0], &i, 0), "the original is never a duplicate");
        i.redundant_copy = true;
        assert!(exact_ok(&r[0], &i, 0));
    }
}

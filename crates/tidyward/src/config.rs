//! Where Tidyward keeps its files, and the settings in `config.toml`.
//!
//! Data folder: `$TIDYWARD_HOME`, else the platform data folder
//! (`~/Library/Application Support/Tidyward` on macOS). It holds `config.toml`, `rules.txt`
//! and `tidyward.db`.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tidyward_jev::Provider;

pub const STARTER_RULES: &str = include_str!("../../../rules/starter.txt");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Folders to tidy; `~` is the home folder.
    pub folders: Vec<String>,
    /// "typesafe", "vercel", or "local" for no model at all.
    pub provider: String,
    /// Items per model call (about 60 fit).
    pub batch: usize,
    /// A semantic condition applies at this probability or above.
    pub hit: f64,
    /// Between `weak` and `hit` the item is left to you.
    pub weak: f64,
    /// USD per million input tokens, for cost estimates.
    pub price_per_million: f64,
    /// Send the download source domain (domain only).
    pub send_domain: bool,
    /// `archive:<folder>` is relative to this; `~` is the home folder.
    pub archive_root: String,
    /// Ask Magika for content types when its command-line tool is installed.
    pub magika: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            folders: vec!["~/Downloads".into()],
            provider: "typesafe".into(),
            batch: 40,
            hit: tidyward_core::engine::HIT,
            weak: tidyward_core::engine::WEAK,
            price_per_million: 0.042,
            send_domain: true,
            archive_root: "~".into(),
            magika: true,
        }
    }
}

pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix('~') {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest.trim_start_matches('/')),
        None => PathBuf::from(p),
    }
}

pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn resolve(explicit: Option<&Path>) -> Result<Paths> {
        let home = match explicit {
            Some(p) => p.to_path_buf(),
            None => match std::env::var_os("TIDYWARD_HOME") {
                Some(h) => PathBuf::from(h),
                None => dirs::data_dir().ok_or_else(|| anyhow!("no data folder on this system"))?.join("Tidyward"),
            },
        };
        Ok(Paths { home })
    }
    pub fn config(&self) -> PathBuf {
        self.home.join("config.toml")
    }
    pub fn rules(&self) -> PathBuf {
        self.home.join("rules.txt")
    }
    pub fn db(&self) -> PathBuf {
        self.home.join("tidyward.db")
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Config> {
        match fs::read_to_string(paths.config()) {
            Ok(t) => toml::from_str(&t).with_context(|| format!("bad {}", paths.config().display())),
            Err(_) => Ok(Config::default()),
        }
    }

    /// No model: "what it is" conditions are answered on this Mac where they can be
    /// ([`tidyward_core::local`]), and left unanswered otherwise.
    pub fn local_only(&self) -> bool {
        self.provider == "local"
    }

    pub fn provider(&self) -> Result<Provider> {
        Provider::parse(&self.provider).ok_or_else(|| anyhow!("unknown provider '{}' (use typesafe or vercel)", self.provider))
    }
}

/// Create config.toml and rules.txt if missing. Returns what was created.
pub fn init(paths: &Paths) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(&paths.home)?;
    let mut made = Vec::new();
    if !paths.config().exists() {
        fs::write(paths.config(), toml::to_string_pretty(&Config::default())?)?;
        made.push(paths.config());
    }
    if !paths.rules().exists() {
        fs::write(paths.rules(), STARTER_RULES)?;
        made.push(paths.rules());
    }
    Ok(made)
}

impl Config {
    pub fn save(&self, paths: &Paths) -> Result<()> {
        fs::create_dir_all(&paths.home)?;
        fs::write(paths.config(), toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// Where the provider key would come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "var", rename_all = "snake_case")]
pub enum KeySource {
    Env(String),
    Keychain,
    Missing,
}

pub fn key_source(p: Provider) -> KeySource {
    if let Some(v) = p.key_env().iter().find(|v| std::env::var(v).map(|x| !x.trim().is_empty()).unwrap_or(false)) {
        return KeySource::Env(v.to_string());
    }
    match keyring::Entry::new("tidyward", p.name()).and_then(|e| e.get_password()) {
        Ok(_) => KeySource::Keychain,
        Err(_) => KeySource::Missing,
    }
}

/// Keys read from the keychain in this process. macOS may ask the person before an app reads
/// the keychain; keeping the key after the first read means that happens once per launch at
/// most, not on every scan or test.
static KEYS: std::sync::Mutex<Vec<(&'static str, String)>> = std::sync::Mutex::new(Vec::new());

fn cached(p: Provider) -> Option<String> {
    KEYS.lock().ok()?.iter().find(|(n, _)| *n == p.name()).map(|(_, k)| k.clone())
}

fn remember(p: Provider, key: &str) {
    if let Ok(mut keys) = KEYS.lock() {
        keys.retain(|(n, _)| *n != p.name());
        keys.push((p.name(), key.to_string()));
    }
}

/// Store the key in the system keychain (service "tidyward", account = provider name).
pub fn set_key(p: Provider, key: &str) -> Result<()> {
    keyring::Entry::new("tidyward", p.name())?.set_password(key.trim())?;
    remember(p, key.trim());
    Ok(())
}

/// The provider key: environment first, then the system keychain.
pub fn api_key(p: Provider) -> Result<String> {
    for var in p.key_env() {
        if let Ok(v) = std::env::var(var) {
            if !v.trim().is_empty() {
                return Ok(v.trim().to_string());
            }
        }
    }
    if let Some(k) = cached(p) {
        return Ok(k);
    }
    let entry = keyring::Entry::new("tidyward", p.name())?;
    let key = entry.get_password().map_err(|_| {
        anyhow!("no {} key: run `tidyward key set`, or set {}", p.name(), p.key_env().join(" or "))
    })?;
    remember(p, &key);
    Ok(key)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shipped_rules_parse() {
        tidyward_core::rules::parse(super::STARTER_RULES).unwrap();
        tidyward_core::rules::parse(include_str!("../../../rules/more-examples.txt")).unwrap();
    }
}

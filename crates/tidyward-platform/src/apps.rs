//! Installed apps, to tell whether an installer's app is already on the machine.

use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::sync::LazyLock;

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

/// Normalized names of the apps in /Applications and ~/Applications.
pub fn installed() -> HashSet<String> {
    let mut dirs = vec![std::path::PathBuf::from("/Applications")];
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join("Applications"));
    }
    dirs.iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".app").map(normalize))
        .collect()
}

/// The part of an installer name before its version or platform: "Foo-1.2-arm64.dmg" -> "foo".
static CUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s_\-.]\d|[-_ ](mac|osx|arm|x64|universal)").unwrap());

pub fn matches(installer_name: &str, installed: &HashSet<String>) -> bool {
    let stem = installer_name.rsplit_once('.').map(|(s, _)| s).unwrap_or(installer_name).to_lowercase();
    let head = CUT.find(&stem).map(|m| &stem[..m.start()]).unwrap_or(&stem);
    let key = normalize(head);
    !key.is_empty() && installed.iter().filter(|a| a.len() > 2).any(|a| a.contains(&key) || key.contains(a.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        let apps: HashSet<String> = ["windowsapp", "firefox"].iter().map(|s| s.to_string()).collect();
        assert!(matches("Windows_App_11.0.9_installer.pkg", &apps));
        assert!(matches("Firefox 131.0.dmg", &apps));
        assert!(!matches("Unknown-Tool-2.0.dmg", &apps));
    }
}

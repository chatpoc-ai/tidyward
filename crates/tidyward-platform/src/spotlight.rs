//! Spotlight metadata (macOS): dates, download source, camera, screenshot flag, pixel size,
//! the app that made the file, page count.
//!
//! Reads many files per `mdls` run: with `-raw` it prints the values of every file one after
//! another, separated by NUL, attributes in name order. If the count does not add up (a file
//! vanished, say), that batch falls back to one run per file.

use chrono::DateTime;
use regex::Regex;
use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;
use tidyward_core::Item;

/// Sorted: mdls prints attributes in name order, whatever order they are asked in.
const NAMES: [&str; 9] = [
    "kMDItemAcquisitionModel",
    "kMDItemCreator",
    "kMDItemDateAdded",
    "kMDItemIsScreenCapture",
    "kMDItemLastUsedDate",
    "kMDItemNumberOfPages",
    "kMDItemPixelHeight",
    "kMDItemPixelWidth",
    "kMDItemWhereFroms",
];
const BATCH: usize = 100;

static URL_HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://([^/"\s]+)"#).unwrap());

fn run(paths: &[PathBuf]) -> Option<Vec<String>> {
    let mut cmd = Command::new("mdls");
    cmd.args(["-raw", "-nullMarker", ""]);
    for n in NAMES {
        cmd.args(["-name", n]);
    }
    let out = cmd.args(paths).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let vals: Vec<String> = text.split('\0').map(String::from).collect();
    (vals.len() == paths.len() * NAMES.len()).then_some(vals)
}

fn ts(s: &str) -> Option<i64> {
    DateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S %z").ok().map(|d| d.timestamp())
}

fn num(s: &str) -> Option<u32> {
    s.trim().parse().ok()
}

fn apply(item: &mut Item, v: &[String]) {
    let get = |n: &str| v[NAMES.iter().position(|x| *x == n).unwrap()].trim().to_string();
    let some = |s: String| (!s.is_empty()).then_some(s);
    item.camera = some(get("kMDItemAcquisitionModel"));
    item.creator = some(get("kMDItemCreator"));
    item.added = ts(&get("kMDItemDateAdded"));
    item.screen_capture = get("kMDItemIsScreenCapture") == "1";
    item.last_used = ts(&get("kMDItemLastUsedDate"));
    item.pages = num(&get("kMDItemNumberOfPages"));
    item.height = num(&get("kMDItemPixelHeight"));
    item.width = num(&get("kMDItemPixelWidth"));
    item.domain = URL_HOST
        .captures(&get("kMDItemWhereFroms"))
        .map(|c| c[1].to_lowercase().trim_start_matches("www.").to_string());
}

/// Fill the Spotlight fields of every item. A no-op where `mdls` does not exist.
pub fn fill(items: &mut [Item]) {
    if !cfg!(target_os = "macos") {
        return;
    }
    for chunk in items.chunks_mut(BATCH) {
        let paths: Vec<PathBuf> = chunk.iter().map(|i| i.path.clone()).collect();
        match run(&paths) {
            Some(vals) => {
                for (item, v) in chunk.iter_mut().zip(vals.chunks(NAMES.len())) {
                    apply(item, v);
                }
            }
            None => {
                for item in chunk.iter_mut() {
                    if let Some(v) = run(std::slice::from_ref(&item.path)) {
                        apply(item, &v);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dates() {
        assert_eq!(ts("2026-10-25 04:08:04 +0000"), Some(1_792_901_284));
        assert_eq!(ts(""), None);
    }
}

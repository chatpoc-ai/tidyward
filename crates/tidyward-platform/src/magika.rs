//! Content type from Google Magika (https://github.com/google/magika), via its command-line
//! tool when it is installed (`brew install magika`).
//!
//! Magika reads container formats as their wrapper: HEIC as MP4, 3MF as DOCX or ZIP, G-code as
//! INI. So a disagreement is a "mismatch" only outside those known groups, with a confident
//! score, and the extension is never overridden; the result is a hint in the state.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use tidyward_core::{Content, Item};

const SAME_CONTAINER: [&[&str]; 3] = [
    &["heic", "heif", "mp4", "mov", "qt", "m4v", "m4a"],
    &["zip", "docx", "xlsx", "pptx", "3mf", "curaprofile", "epub", "pages", "numbers", "key"],
    &["txt", "ini", "gcode", "md", "markdown", "step", "csv", "json", "xml"],
];
const BATCH: usize = 200;

struct Out {
    label: String,
    description: String,
    score: f64,
    exts: HashSet<String>,
}

fn run(paths: &[PathBuf]) -> HashMap<PathBuf, Out> {
    let Ok(out) = Command::new("magika").arg("--jsonl").args(paths).output() else { return HashMap::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|r| r["result"]["status"] == "ok")
        .filter_map(|r| {
            let v = &r["result"]["value"];
            let o = &v["output"];
            let label = o["label"].as_str()?.to_string();
            let mut exts: HashSet<String> =
                o["extensions"].as_array()?.iter().filter_map(|e| e.as_str().map(String::from)).collect();
            exts.insert(label.clone());
            Some((
                PathBuf::from(r["path"].as_str()?),
                Out { label, description: o["description"].as_str().unwrap_or("").into(), score: v["score"].as_f64()?, exts },
            ))
        })
        .collect()
}

pub fn mismatch(ext: &str, label: &str, exts: &HashSet<String>, score: f64) -> bool {
    let ext = ext.trim_start_matches('.');
    let pkg_is_xar = ext == "pkg" && label == "xar"; // .pkg installers are xar archives
    !ext.is_empty()
        && !exts.contains(ext)
        && !SAME_CONTAINER.iter().any(|g| g.contains(&ext) && g.contains(&label))
        && !pkg_is_xar
        && score >= 0.9
        && label != "unknown"
        && label != "txt"
}

/// Fill `content` for files; a no-op when the tool is missing.
pub fn fill(items: &mut [Item]) {
    if Command::new("magika").arg("--version").output().is_err() {
        return;
    }
    let files: Vec<usize> = (0..items.len()).filter(|&i| !items[i].is_dir).collect();
    for chunk in files.chunks(BATCH) {
        let paths: Vec<PathBuf> = chunk.iter().map(|&i| items[i].path.clone()).collect();
        let found = run(&paths);
        for &i in chunk {
            if let Some(o) = found.get(&items[i].path) {
                let m = mismatch(&items[i].ext, &o.label, &o.exts, o.score);
                items[i].content =
                    Some(Content { label: o.label.clone(), description: o.description.clone(), score: o.score, mismatch: m });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn container_confusions_are_not_mismatches() {
        assert!(!mismatch(".heic", "mp4", &set(&["mp4"]), 0.99));
        assert!(!mismatch(".3mf", "docx", &set(&["docx"]), 0.95));
        assert!(!mismatch(".pkg", "xar", &set(&["xar"]), 0.99));
        assert!(!mismatch(".jpg", "jpeg", &set(&["jpg", "jpeg"]), 0.99));
    }

    #[test]
    fn real_mismatch() {
        assert!(mismatch(".pdf", "html", &set(&["html", "htm"]), 0.98));
        assert!(!mismatch(".pdf", "html", &set(&["html"]), 0.6), "low score is not enough");
    }
}

//! Scanning: turn the top-level entries of a folder into [`Item`]s. Read-only; nothing here
//! writes to the scanned folder.
//!
//! Besides size and dates it reads what the system already knows, for free and on this machine:
//! Spotlight metadata on macOS ([`spotlight`]) and the content type from Google Magika when its
//! command-line tool is installed ([`magika`]).

pub mod apps;
pub mod magika;
pub mod spotlight;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;
use tidyward_core::{Item, Kind};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Ask Magika for content types (skipped quietly when the tool is not installed).
    pub magika: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { magika: true }
    }
}

fn unix(t: std::io::Result<std::time::SystemTime>) -> i64 {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Total size and file count of a folder, without following symlinks.
fn du(path: &Path) -> (u64, u64) {
    let (mut size, mut count) = (0, 0);
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(md) = e.path().symlink_metadata() else { continue };
            if md.is_dir() {
                stack.push(e.path());
            } else {
                size += md.len();
                count += 1;
            }
        }
    }
    (size, count)
}

/// Size + first and last 64 KB: cheap and good enough to find exact duplicates in one folder.
fn quick_hash(path: &Path, size: u64) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    h.update(size.to_string().as_bytes());
    let mut buf = vec![0u8; 65536];
    let n = f.read(&mut buf).ok()?;
    h.update(&buf[..n]);
    if size > 131_072 {
        f.seek(SeekFrom::End(-65536)).ok()?;
        let n = f.read(&mut buf).ok()?;
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn ext_of(name: &str, is_dir: bool) -> String {
    let p = Path::new(name);
    match p.extension() {
        Some(e) if !e.is_empty() && (!is_dir || e.eq_ignore_ascii_case("app")) => {
            format!(".{}", e.to_string_lossy().to_lowercase())
        }
        _ => String::new(),
    }
}

/// Scan the top-level, non-hidden entries of `root`, sorted by name.
pub fn scan_folder(root: &Path, opts: &ScanOptions) -> Result<Vec<Item>> {
    let mut entries: Vec<_> = fs::read_dir(root)
        .with_context(|| format!("cannot read {}", root.display()))?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut items = Vec::new();
    for e in entries {
        let path = e.path();
        let Ok(md) = fs::metadata(&path) else { continue }; // broken symlink
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = md.is_dir();
        let ext = ext_of(&name, is_dir);
        let (size, files_inside) = if is_dir { du(&path) } else { (md.len(), 1) };
        let kind = Kind::of(&ext, is_dir);
        let qhash = if !is_dir && size > 0 { quick_hash(&path, size) } else { None };
        items.push(Item {
            name,
            is_dir,
            kind,
            ext,
            size,
            files_inside,
            mtime: unix(md.modified()),
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
            qhash,
            dupes: 0,
            redundant_copy: false,
            app_installed: None,
            path,
        });
    }

    spotlight::fill(&mut items);
    if opts.magika {
        magika::fill(&mut items);
    }
    let installed = apps::installed();
    for it in items.iter_mut().filter(|i| i.kind == Kind::Installer) {
        it.app_installed = Some(apps::matches(&it.name, &installed));
    }
    mark_duplicates(&mut items);
    Ok(items)
}

/// Each group of identical files keeps one original (earliest to arrive, then shortest name);
/// only the others are "redundant copies".
pub fn mark_duplicates(items: &mut [Item]) {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if let Some(h) = &it.qhash {
            groups.entry(h.clone()).or_default().push(i);
        }
    }
    for idx in groups.values().filter(|g| g.len() > 1) {
        let keeper = *idx.iter().min_by_key(|&&i| (items[i].arrived(), items[i].name.chars().count())).unwrap();
        for &i in idx {
            items[i].dupes = idx.len() - 1;
            items[i].redundant_copy = i != keeper;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_and_finds_duplicates() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("report.pdf"), b"same content").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(d.path().join("report (1).pdf"), b"same content").unwrap();
        fs::write(d.path().join("other.txt"), b"different").unwrap();
        fs::write(d.path().join(".hidden"), b"x").unwrap();
        fs::create_dir(d.path().join("folder")).unwrap();
        fs::write(d.path().join("folder/a"), b"12345").unwrap();

        let items = scan_folder(d.path(), &ScanOptions { magika: false }).unwrap();
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["folder", "other.txt", "report (1).pdf", "report.pdf"]);
        let by = |n: &str| items.iter().find(|i| i.name == n).unwrap();
        assert_eq!((by("folder").kind, by("folder").size, by("folder").files_inside), (Kind::Folder, 5, 1));
        assert!(by("report (1).pdf").redundant_copy, "the later copy is the extra one");
        assert!(!by("report.pdf").redundant_copy && by("report.pdf").dupes == 1);
        assert_eq!(by("other.txt").dupes, 0);
    }
}

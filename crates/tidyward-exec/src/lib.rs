//! Execution: the only code that changes files. Two operations, both reversible:
//!
//! - [`move_into`]: move into a folder, never overwriting (a name clash gets " (2)", " (3)", ...).
//! - [`trash`]: move to the system Trash, never a hard delete. Returns where the item landed in
//!   the Trash, so [`restore`] can put it back.
//!
//! Callers log every operation (source and destination) before reporting success.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// A path inside `dir` named like `name` that does not exist yet.
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// Move `src` into `dest_dir` (created if needed). Returns the new path.
pub fn move_into(src: &Path, dest_dir: &Path) -> Result<PathBuf> {
    if !src.exists() {
        bail!("{} no longer exists", src.display());
    }
    fs::create_dir_all(dest_dir).with_context(|| format!("cannot create {}", dest_dir.display()))?;
    let name = src.file_name().context("no file name")?.to_string_lossy();
    let dst = free_name(dest_dir, &name);
    fs::rename(src, &dst).with_context(|| format!("cannot move {} to {}", src.display(), dst.display()))?;
    Ok(dst)
}

/// Move `src` to the Trash. Returns its path inside the Trash.
#[cfg(target_os = "macos")]
pub fn trash(src: &Path) -> Result<PathBuf> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    if !src.exists() {
        bail!("{} no longer exists", src.display());
    }
    let url = NSURL::fileURLWithPath(&NSString::from_str(&src.to_string_lossy()));
    let fm = NSFileManager::defaultManager();
    let mut landed: Option<Retained<NSURL>> = None;
    fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed))
        .map_err(|e| anyhow::anyhow!("cannot move {} to the Trash: {}", src.display(), e.localizedDescription()))?;
    let path = landed.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()));
    path.context("the Trash did not say where the item went")
}

#[cfg(not(target_os = "macos"))]
pub fn trash(src: &Path) -> Result<PathBuf> {
    bail!("moving to the Trash is only supported on macOS so far ({})", src.display())
}

/// Put an item back from `from` (where an operation left it) to `to` (where it was).
/// Refuses to overwrite: if something now sits at `to`, nothing is changed.
pub fn restore(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        bail!("{} is gone (Trash emptied, or moved by hand)", from.display());
    }
    if to.exists() {
        bail!("{} exists again; not overwriting it", to.display());
    }
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::rename(from, to).with_context(|| format!("cannot move {} back to {}", from.display(), to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_never_overwrites_and_restores() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.txt");
        let dest = d.path().join("Archive");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("a.txt"), "already there").unwrap();
        fs::write(&src, "mine").unwrap();

        let moved = move_into(&src, &dest).unwrap();
        assert_eq!(moved, dest.join("a (2).txt"));
        assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "already there");

        restore(&moved, &src).unwrap();
        assert_eq!(fs::read_to_string(&src).unwrap(), "mine");

        fs::write(&moved, "new").unwrap();
        assert!(restore(&moved, &src).is_err(), "never overwrites");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn trash_and_restore() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("tidyward-test-trash.txt");
        fs::write(&src, "x").unwrap();
        let landed = trash(&src).unwrap();
        assert!(!src.exists() && landed.exists(), "in the Trash at {}", landed.display());
        restore(&landed, &src).unwrap();
        assert!(src.exists() && !landed.exists());
    }
}

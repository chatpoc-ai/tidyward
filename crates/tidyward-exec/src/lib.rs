//! Execution: the only code that changes files. Two operations, both reversible:
//!
//! - [`move_into`]: move into a folder, never overwriting (a name clash gets " (2)", " (3)", ...),
//!   optionally under a new name ([`move_into_as`]); the extension never changes.
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
    move_into_as(src, dest_dir, None)
}

/// A new name for `src`: `stem` plus the original extension. Refuses names that would leave the
/// folder or hide the file (path separators, a leading dot, empty).
pub fn renamed(src: &Path, stem: &str) -> Result<String> {
    let stem = stem.trim();
    if stem.is_empty() || stem.starts_with('.') || stem.contains(['/', ':', '\\', '\0']) {
        bail!("not a usable file name: {stem:?}");
    }
    Ok(match src.extension() {
        Some(ext) if src.is_file() => format!("{stem}.{}", ext.to_string_lossy()),
        _ => stem.to_string(),
    })
}

/// Folders that are really an app's or a library's insides: never move anything into them.
const PACKAGES: &[&str] = &[
    "app", "photoslibrary", "photolibrary", "aplibrary", "musiclibrary", "tvlibrary", "imovielibrary",
    "fcpbundle", "logicx", "band", "bundle", "framework", "plugin", "kext", "pkg", "mpkg", "xcodeproj", "xcworkspace",
];

fn inside_package(dir: &Path) -> Option<String> {
    dir.components().find_map(|c| {
        let name = c.as_os_str().to_string_lossy();
        let ext = Path::new(name.as_ref()).extension()?.to_string_lossy().to_lowercase();
        PACKAGES.contains(&ext.as_str()).then(|| name.into_owned())
    })
}

/// Like [`move_into`], under the new name `stem` (the extension stays) when given.
pub fn move_into_as(src: &Path, dest_dir: &Path, stem: Option<&str>) -> Result<PathBuf> {
    if !src.exists() {
        bail!("{} no longer exists", src.display());
    }
    if let Some(p) = inside_package(dest_dir) {
        bail!("{} is a package or library ({p}); files are never moved into one", dest_dir.display());
    }
    let name = match stem {
        Some(s) => renamed(src, s)?,
        None => src.file_name().context("no file name")?.to_string_lossy().into_owned(),
    };
    fs::create_dir_all(dest_dir).with_context(|| format!("cannot create {}", dest_dir.display()))?;
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
    fn never_into_a_package() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("IMG_1.jpg");
        fs::write(&src, b"x").unwrap();
        let lib = t.path().join("Pictures/Photos Library.photoslibrary");
        assert!(move_into(&src, &lib).is_err());
        assert!(move_into(&src, &lib.join("originals")).is_err(), "nor anywhere inside one");
        assert!(src.exists(), "the file stays put");
        assert!(move_into(&src, &t.path().join("Pictures/Photos")).is_ok());
    }

    #[test]
    fn rename_keeps_the_extension_and_undoes() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("Screenshot 2025-03-02 at 10.14.55.png");
        fs::write(&src, b"x").unwrap();
        assert!(renamed(&src, "../x").is_err() && renamed(&src, ".hidden").is_err() && renamed(&src, " ").is_err());
        let dst = move_into_as(&src, &t.path().join("Shots"), Some("2025-03-02 Screenshot")).unwrap();
        assert_eq!(dst.file_name().unwrap(), "2025-03-02 Screenshot.png");
        restore(&dst, &src).unwrap();
        assert!(src.exists() && !dst.exists(), "undo brings back the old name");
    }

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

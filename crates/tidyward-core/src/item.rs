//! What the scanner knows about one top-level item in a folder.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Folder,
    Installer,
    Archive,
    Image,
    Video,
    Audio,
    Document,
    Data,
    Code,
    Certificate,
    Font,
    Model3d,
    Other,
}

/// Extension groups, checked in this order (".key" is a Keynote document before it is a key).
const KINDS: &[(Kind, &[&str])] = &[
    (Kind::Installer, &[".dmg", ".pkg", ".mpkg", ".exe", ".msi", ".app"]),
    (Kind::Archive, &[".zip", ".rar", ".7z", ".tar", ".gz", ".tgz", ".bz2", ".xz"]),
    (Kind::Image, &[".png", ".jpg", ".jpeg", ".heic", ".gif", ".webp", ".bmp", ".tiff", ".svg"]),
    (Kind::Video, &[".mov", ".mp4", ".m4v", ".avi", ".mkv", ".webm"]),
    (Kind::Audio, &[".mp3", ".m4a", ".wav", ".aac", ".flac"]),
    (Kind::Document, &[".pdf", ".doc", ".docx", ".pages", ".txt", ".md", ".rtf", ".key", ".ppt", ".pptx", ".epub"]),
    (Kind::Data, &[".xls", ".xlsx", ".csv", ".numbers", ".json", ".xml", ".sql", ".db"]),
    (Kind::Code, &[".py", ".js", ".ts", ".cs", ".java", ".sh", ".ipynb", ".html", ".css"]),
    (Kind::Certificate, &[".pem", ".p12", ".cer", ".crt", ".key", ".mobileprovision"]),
    (Kind::Font, &[".ttf", ".otf", ".woff", ".woff2"]),
    (Kind::Model3d, &[".stl", ".3mf", ".step", ".stp", ".obj", ".f3d", ".gcode", ".curaprofile"]),
];

impl Kind {
    /// `ext` is lowercase with the dot, or empty. Directories are folders, except `.app` bundles.
    pub fn of(ext: &str, is_dir: bool) -> Kind {
        if is_dir && ext != ".app" {
            return Kind::Folder;
        }
        KINDS
            .iter()
            .find(|(_, exts)| exts.contains(&ext))
            .map(|(k, _)| *k)
            .unwrap_or(Kind::Other)
    }

    /// Words used in the state sent to the model.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::Installer => "installer",
            Kind::Archive => "archive",
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Document => "document",
            Kind::Data => "spreadsheet or data",
            Kind::Code => "code",
            Kind::Certificate => "certificate or key",
            Kind::Font => "font",
            Kind::Model3d => "3D model or print file",
            Kind::Other => "other",
        }
    }
}

/// Content type from Magika, used only when it adds something the extension does not say.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub label: String,
    pub description: String,
    pub score: f64,
    /// The extension clearly contradicts the content (known container confusions excluded).
    pub mismatch: bool,
}

/// What a folder or archive holds, read on this Mac. A model is told only the mix of file types;
/// the names are read here and never leave the machine (see [`crate::state::build`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Inside {
    /// Files, with an app or other package counted as one.
    pub files: u64,
    /// Extensions and how many files have each, most common first ("" for none).
    pub exts: Vec<(String, u64)>,
    /// Real names of some of the files.
    pub names: Vec<String>,
}

/// Folders that are one thing to the person: an app, a library, a bundle.
pub const PACKAGES: &[&str] = &[
    ".app", ".photoslibrary", ".musiclibrary", ".tvlibrary", ".bundle", ".framework", ".plugin", ".pkg", ".mpkg",
    ".xcodeproj", ".xcworkspace", ".rtfd", ".pages", ".numbers", ".key", ".band", ".imovielibrary", ".fcpbundle",
];
const NAMES_KEPT: usize = 40;

impl Inside {
    /// From the paths of the files inside, relative to the folder or archive. Hidden files and
    /// macOS resource forks are left out; anything inside a package counts as the package.
    pub fn from_paths<I: IntoIterator<Item = String>>(paths: I) -> Inside {
        let mut seen_pkg = std::collections::HashSet::new();
        let mut counts: Vec<(String, u64)> = Vec::new();
        let mut ins = Inside::default();
        for p in paths {
            let parts: Vec<&str> = p.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
            if parts.is_empty() || parts.iter().any(|c| c.starts_with('.') || *c == "__MACOSX") {
                continue;
            }
            let pkg = parts.iter().position(|c| PACKAGES.iter().any(|e| c.to_lowercase().ends_with(e)));
            let name = match pkg {
                Some(i) if !seen_pkg.insert(parts[..=i].join("/")) => continue,
                Some(i) => parts[i],
                None => parts[parts.len() - 1],
            };
            let ext = name.rfind('.').filter(|i| *i > 0).map(|i| name[i..].to_lowercase()).unwrap_or_default();
            match counts.iter_mut().find(|(e, _)| *e == ext) {
                Some((_, n)) => *n += 1,
                None => counts.push((ext, 1)),
            }
            ins.files += 1;
            if ins.names.len() < NAMES_KEPT {
                ins.names.push(name.to_string());
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ins.exts = counts;
        ins
    }

    /// The kind most files are, when at least four in five are.
    pub fn mostly(&self) -> Option<Kind> {
        let mut kinds: Vec<(Kind, u64)> = Vec::new();
        for (e, n) in &self.exts {
            let k = Kind::of(e, false);
            match kinds.iter_mut().find(|(x, _)| *x == k) {
                Some((_, m)) => *m += n,
                None => kinds.push((k, *n)),
            }
        }
        let (k, n) = kinds.into_iter().max_by_key(|(_, n)| *n)?;
        (n * 5 >= self.files * 4).then_some(k)
    }

    /// The type mix in words, e.g. "40 files: 32 image (.jpg), 6 document (.pdf), 2 other".
    pub fn describe(&self) -> String {
        if self.files == 0 {
            return "nothing".into();
        }
        let mut parts: Vec<String> = self
            .exts
            .iter()
            .take(4)
            .map(|(e, n)| {
                let k = Kind::of(e, false);
                if e.is_empty() { format!("{n} without an extension") } else { format!("{n} {} ({e})", k.label()) }
            })
            .collect();
        let rest: u64 = self.exts.iter().skip(4).map(|(_, n)| n).sum();
        if rest > 0 {
            parts.push(format!("{rest} more of other types"));
        }
        let files = if self.files == 1 { "1 file".to_string() } else { format!("{} files", self.files) };
        format!("{files}: {}", parts.join(", "))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub kind: Kind,
    /// Lowercase with the dot, or empty (folders have none).
    pub ext: String,
    pub size: u64,
    pub files_inside: u64,
    /// Unix seconds.
    pub mtime: i64,
    pub added: Option<i64>,
    pub last_used: Option<i64>,
    pub domain: Option<String>,
    pub camera: Option<String>,
    pub screen_capture: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub creator: Option<String>,
    pub pages: Option<u32>,
    pub content: Option<Content>,
    /// Content fingerprint for duplicate detection and for finding a file again after a move.
    pub qhash: Option<String>,
    /// How many other items in the folder have the same content.
    pub dupes: usize,
    /// One of a duplicate group that is not the original (the original is the earliest to arrive).
    pub redundant_copy: bool,
    /// For installers: whether a matching app is already installed.
    pub app_installed: Option<bool>,
    /// For folders and archives: what they hold.
    #[serde(default)]
    pub inside: Option<Inside>,
}

impl Item {
    pub fn arrived(&self) -> i64 {
        self.added.unwrap_or(self.mtime)
    }

    /// Changes whenever the file does; with the masked name it decides whether memory is still valid.
    pub fn fingerprint(&self) -> String {
        format!("{}:{}:{}", self.size, self.mtime, self.files_inside)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(Kind::of(".key", false), Kind::Document);
        assert_eq!(Kind::of(".app", true), Kind::Installer);
        assert_eq!(Kind::of("", true), Kind::Folder);
        assert_eq!(Kind::of(".gcode", false), Kind::Model3d);
        assert_eq!(Kind::of(".xyz", false), Kind::Other);
    }

    #[test]
    fn inside() {
        let paths = ["a.jpg", "b.JPG", "sub/c.jpg", "sub/d.jpg", "notes.txt", ".DS_Store", "__MACOSX/._a.jpg",
                     "Tool.app/Contents/Info.plist", "Tool.app/Contents/MacOS/tool"];
        let ins = Inside::from_paths(paths.iter().map(|s| s.to_string()));
        assert_eq!(ins.files, 6, "hidden files left out, the app counted once");
        assert_eq!(ins.exts[0], (".jpg".to_string(), 4));
        assert_eq!(ins.names, ["a.jpg", "b.JPG", "c.jpg", "d.jpg", "notes.txt", "Tool.app"]);
        assert_eq!(ins.describe(), "6 files: 4 image (.jpg), 1 installer (.app), 1 document (.txt)");
        assert_eq!(ins.mostly(), None, "4 of 6 is not most");
        let app = Inside::from_paths(["Tool.app/Contents/Info.plist".to_string(), "Tool.app/x".to_string()]);
        assert_eq!(app.mostly(), Some(Kind::Installer));
    }
}

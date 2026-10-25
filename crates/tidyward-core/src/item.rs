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
}

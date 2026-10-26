//! The state sent to the model for one item. This is the whole of what leaves the machine about a
//! file: a masked name and bucketed words. No path, no real name, no contents. For a folder or
//! an archive, the mix of file types inside, but not the names of the files in it.

use crate::item::{Item, Kind};
use crate::words::{age_words, bucket, creator_kind, name_features, size_words};
use serde_json::{Map, Value};

pub struct StateOptions<'a> {
    /// How the folder is described, e.g. "Downloads"; other folders become "a user folder".
    pub folder: &'a str,
    /// Send the download source domain (domain only, never the full URL).
    pub send_domain: bool,
    pub now: i64,
}

pub const MASK_NOTE: &str =
    "  (<date> <time> <n> <id> <version> stand for numbers; <word>, <中文>, <かな>, <한글> and <text> stand for hidden words such as names)";

pub fn folder_label(folder_name: &str) -> &'static str {
    match folder_name {
        "Downloads" => "Downloads",
        "Desktop" => "Desktop",
        "Documents" => "Documents",
        _ => "a user folder",
    }
}

pub fn build(item: &Item, masked: &str, o: &StateOptions) -> Map<String, Value> {
    let mut s = Map::new();
    let mut put = |k: &str, v: String| {
        s.insert(k.to_string(), Value::String(v));
    };
    put("file name, masked", format!("{masked}{MASK_NOTE}"));
    put("folder it sits in", folder_label(o.folder).to_string());
    let ty = if item.ext.is_empty() { item.kind.label().to_string() } else { format!("{} ({})", item.kind.label(), item.ext) };
    put("type", ty);
    put("size", size_words(item.size).into());
    put("arrived in the folder", age_words(Some(item.arrived()), o.now).into());
    put(
        "last opened",
        match item.last_used {
            None => "never opened since it arrived".into(),
            Some(t) => age_words(Some(t), o.now).into(),
        },
    );
    put("last modified", age_words(Some(item.mtime), o.now).into());
    if item.kind == Kind::Folder {
        let w = bucket(
            item.files_inside as f64,
            &[2.0, 20.0, 500.0, 10000.0],
            &["empty", "a few", "dozens to hundreds", "thousands", "tens of thousands"],
        );
        put("files inside", w.into());
    }
    if let Some(ins) = item.inside.as_ref().filter(|_| matches!(item.kind, Kind::Folder | Kind::Archive)) {
        put("what is inside, by file type", ins.describe());
    }
    if let Some(installed) = item.app_installed {
        put("matching app already installed", if installed { "yes" } else { "no or unknown" }.into());
    }
    if o.send_domain {
        put("downloaded from", item.domain.clone().unwrap_or_else(|| "unknown".into()));
    }
    put(
        "identical copy in the same folder",
        if item.redundant_copy {
            "yes, and this one is the extra copy"
        } else if item.dupes > 0 {
            "yes, and this one is the original"
        } else {
            "no"
        }
        .into(),
    );
    let f = name_features(&item.name);
    put("clues from the name", if f.is_empty() { "none".into() } else { f.join("; ") });
    if let Some(c) = creator_kind(item.creator.as_deref()) {
        put("made by", c.into());
    }
    if let Some(p) = item.pages.filter(|p| *p > 0) {
        put("pages", bucket(p as f64, &[2.0, 6.0, 51.0], &["1 page", "2-5 pages", "6-50 pages", "over 50 pages"]).into());
    }
    if let Some(c) = &item.content {
        if item.ext.is_empty() {
            put("content looks like", c.description.clone());
        } else if c.mismatch {
            put("content check", format!("the content does not match the extension; it looks like {}", c.description));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tests::item;
    use crate::item::Inside;

    #[test]
    fn inside_names_never_leave() {
        let mut zip = item("photos.zip");
        zip.inside = Some(Inside::from_paths(["Jane at the lake.jpg".to_string(), "Jane passport.jpg".to_string()]));
        let s = build(&zip, "photos.zip", &StateOptions { folder: "Downloads", send_domain: false, now: 0 });
        assert_eq!(s["what is inside, by file type"], "2 files: 2 image (.jpg)");
        let all = serde_json::to_string(&s).unwrap();
        assert!(!all.contains("Jane") && !all.contains("lake"), "{all}");
    }
}

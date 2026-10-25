//! Layer 1, identify: say what an item is. Local signals first; the first sure one wins.
//! Anything not covered goes to the model as one choice question ([`purpose_question`]).
//!
//! | signal                                              | purpose             |
//! |-----------------------------------------------------|---------------------|
//! | camera model                                        | photo (camera video: media) |
//! | system screenshot flag                              | screenshot          |
//! | no camera + exact phone/tablet screen size          | screenshot          |
//! | name says screenshot / screen recording             | screenshot          |
//! | extension: partial download, installer, video/audio, 3D/print, code | that purpose |

use crate::item::{Item, Kind};
use crate::words::name_features;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Screenshot,
    Photo,
    Installer,
    Finance,
    PersonalLegal,
    WorkProject,
    Reference,
    Media,
    Temporary,
    Other,
}

impl Purpose {
    pub const ALL: [Purpose; 10] = [
        Purpose::Screenshot,
        Purpose::Photo,
        Purpose::Installer,
        Purpose::Finance,
        Purpose::PersonalLegal,
        Purpose::WorkProject,
        Purpose::Reference,
        Purpose::Media,
        Purpose::Temporary,
        Purpose::Other,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Purpose::Screenshot => "screenshot",
            Purpose::Photo => "photo",
            Purpose::Installer => "installer",
            Purpose::Finance => "finance",
            Purpose::PersonalLegal => "personal_legal",
            Purpose::WorkProject => "work_project",
            Purpose::Reference => "reference",
            Purpose::Media => "media",
            Purpose::Temporary => "temporary",
            Purpose::Other => "other",
        }
    }

    pub fn from_key(k: &str) -> Option<Purpose> {
        Purpose::ALL.into_iter().find(|p| p.key() == k)
    }

    /// The option text in the identify question.
    fn criterion(self) -> &'static str {
        match self {
            Purpose::Screenshot => "a screenshot or screen recording",
            Purpose::Photo => "a photo or picture saved from somewhere",
            Purpose::Installer => "an app installer or disk image",
            Purpose::Finance => "an invoice, receipt, bill or statement",
            Purpose::PersonalLegal => "an ID, contract, tax, school or other personal paper",
            Purpose::WorkProject => "a work or project file, code, data or design",
            Purpose::Reference => "a manual, article, ebook or other reading material",
            Purpose::Media => "a video or audio file",
            Purpose::Temporary => "a temporary, generated or partial file",
            Purpose::Other => "something else",
        }
    }

    /// How layer 1's answer is described to the model in layer 3.
    pub fn described(self) -> &'static str {
        match self {
            Purpose::Screenshot => "a screenshot or screen recording",
            Purpose::Photo => "a photo or picture",
            Purpose::Installer => "an app installer or disk image",
            Purpose::Finance => "an invoice, receipt, bill or statement",
            Purpose::PersonalLegal => "an ID, contract, tax, school or other personal paper",
            Purpose::WorkProject => "a work or project file",
            Purpose::Reference => "reading material",
            Purpose::Media => "a video or audio file",
            Purpose::Temporary => "a temporary or generated file",
            Purpose::Other => "something else",
        }
    }
}

/// Layer 1's answer for one item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub purpose: Purpose,
    /// 1.0 when found locally.
    pub confidence: f64,
    /// "local" or "model".
    pub source: String,
    pub reason: String,
}

/// Portrait pixel sizes of iPhone and iPad screenshots. Left out on purpose: 1080x1920
/// (wallpapers, story images) and 1536x2048 (AI-generated images) are too common otherwise.
/// Mac screenshots carry the system flag, so desktop sizes are not needed.
const SCREENS: &[(u32, u32)] = &[
    (1320, 2868), (1206, 2622), (1290, 2796), (1179, 2556), (1284, 2778), (1170, 2532), (1125, 2436),
    (1242, 2688), (828, 1792), (1080, 2340), (1242, 2208), (750, 1334), (640, 1136),
    (2048, 2732), (2064, 2752), (1668, 2388), (1668, 2420), (1640, 2360), (1620, 2160), (1668, 2224),
    (1488, 2266),
];
const PARTIAL: &[&str] = &[".crdownload", ".part", ".download", ".tmp", ".partial"];

fn local(purpose: Purpose, reason: String) -> Identity {
    Identity { purpose, confidence: 1.0, source: "local".into(), reason }
}

/// Some when a local signal is sure; None means "ask the model".
pub fn local_identify(item: &Item) -> Option<Identity> {
    if let Some(cam) = &item.camera {
        let p = if item.kind == Kind::Video { Purpose::Media } else { Purpose::Photo };
        return Some(local(p, format!("camera: {cam}")));
    }
    if item.screen_capture {
        return Some(local(Purpose::Screenshot, "system screenshot flag".into()));
    }
    if let (Kind::Image, Some(w), Some(h)) = (item.kind, item.width, item.height) {
        if SCREENS.contains(&(w.min(h), w.max(h))) {
            return Some(local(Purpose::Screenshot, format!("no camera, size {w}x{h} is a phone or tablet screen")));
        }
    }
    if name_features(&item.name)
        .iter()
        .any(|f| f.starts_with("looks like a screenshot") || f.starts_with("looks like a screen recording"))
    {
        return Some(local(Purpose::Screenshot, "name says screenshot or screen recording".into()));
    }
    let ext = &item.ext;
    if PARTIAL.contains(&ext.as_str()) {
        return Some(local(Purpose::Temporary, format!("partial download ({ext})")));
    }
    match item.kind {
        Kind::Installer => Some(local(Purpose::Installer, format!("installer ({ext})"))),
        Kind::Video | Kind::Audio => Some(local(Purpose::Media, format!("{} file ({ext})", item.kind.label()))),
        Kind::Model3d | Kind::Code => Some(local(Purpose::WorkProject, format!("{} ({ext})", item.kind.label()))),
        _ => None,
    }
}

/// The one question asked of the model when no local signal is sure.
pub fn purpose_question() -> Value {
    let criteria: serde_json::Map<String, Value> =
        Purpose::ALL.iter().map(|p| (p.key().to_string(), Value::String(p.criterion().into()))).collect();
    json!({"purpose": {"type": "choice",
        "instructions": "What is this item, as the person would describe it?",
        "criteria": criteria}})
}

/// Layer 1's answer as layer 3 sees it: certain when local, else with the model's confidence.
pub fn describe(id: &Identity) -> String {
    let how = if id.source == "local" {
        format!("certain, {}", id.reason)
    } else {
        format!("confidence {:.2}", id.confidence)
    };
    format!("{} ({how})", id.purpose.described())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::item::Item;
    use std::path::PathBuf;

    pub(crate) fn item(name: &str) -> Item {
        let ext = name.rfind('.').map(|i| name[i..].to_lowercase()).unwrap_or_default();
        Item {
            path: PathBuf::from(format!("/tmp/{name}")),
            name: name.into(),
            is_dir: false,
            kind: Kind::of(&ext, false),
            ext,
            size: 1000,
            files_inside: 1,
            mtime: 0,
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
            qhash: None,
            dupes: 0,
            redundant_copy: false,
            app_installed: None,
        }
    }

    #[test]
    fn camera_wins() {
        let mut i = item("anything.jpeg");
        i.camera = Some("Phone 1".into());
        assert_eq!(local_identify(&i).unwrap().purpose, Purpose::Photo);
    }

    #[test]
    fn screen_size() {
        let mut i = item("shared.png");
        (i.width, i.height) = (Some(2868), Some(1320));
        assert_eq!(local_identify(&i).unwrap().purpose, Purpose::Screenshot);
        (i.width, i.height) = (Some(1080), Some(1920));
        assert!(local_identify(&i).is_none(), "common wallpaper size is not a screenshot");
    }

    #[test]
    fn by_extension_and_name() {
        assert_eq!(local_identify(&item("Screenshot 2026.png")).unwrap().purpose, Purpose::Screenshot);
        assert_eq!(local_identify(&item("tool.dmg")).unwrap().purpose, Purpose::Installer);
        assert_eq!(local_identify(&item("part.stl")).unwrap().purpose, Purpose::WorkProject);
        assert!(local_identify(&item("letter.pdf")).is_none());
    }

    #[test]
    fn question_has_all_options() {
        let q = purpose_question();
        assert_eq!(q["purpose"]["criteria"].as_object().unwrap().len(), 10);
    }
}

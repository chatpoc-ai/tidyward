//! Turn numbers and names into words. Decision models read bucketed words much better than raw
//! numbers, and words also hide exact values.

use fancy_regex::Regex;
use std::sync::LazyLock;

pub const DAY: i64 = 86_400;

pub fn bucket<'a>(x: f64, edges: &[f64], words: &[&'a str]) -> &'a str {
    edges.iter().zip(words).find(|(e, _)| x < **e).map(|(_, w)| *w).unwrap_or(words[words.len() - 1])
}

pub fn size_words(bytes: u64) -> &'static str {
    bucket(
        bytes as f64,
        &[100e3, 10e6, 200e6, 2e9],
        &["tiny (under 100 KB)", "small (under 10 MB)", "medium (under 200 MB)", "large (under 2 GB)", "huge (over 2 GB)"],
    )
}

pub fn age_words(ts: Option<i64>, now: i64) -> &'static str {
    let Some(ts) = ts else { return "unknown" };
    bucket(
        (now - ts) as f64 / DAY as f64,
        &[1.0, 7.0, 30.0, 90.0, 180.0, 365.0, 730.0],
        &[
            "today",
            "this week",
            "this month",
            "1-3 months ago",
            "3-6 months ago",
            "6-12 months ago",
            "1-2 years ago",
            "over 2 years ago",
        ],
    )
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("words regex")
}

macro_rules! regexes {
    ($($name:ident = $pat:expr;)*) => { $(static $name: LazyLock<Regex> = LazyLock::new(|| re($pat));)* };
}

regexes! {
    SCREENSHOT = r"(?i)^(Screen ?Shot|Screenshot|截屏|屏幕快照|CleanShot)";
    RECORDING = r"(?i)^(Screen Recording|屏幕录制)";
    COPY_SUFFIX = r"(?i)(\(\d+\)|[ _-]copy\b| 副本| \d)$";
    VERSION = r"\bv?\d+\.\d+(\.\d+)?\b";
    INVOICE = r"(?i)(invoice|receipt|发票|收据|statement|账单)";
    PERSONAL = r"(?i)(contract|agreement|合同|协议|offer|resume|cv|简历|passport|护照|tax|税)";
    INSTALLER = r"(?i)(setup|install|installer|安装)";
    GEN_HEX = r"(?i)^[0-9a-f\-_]{16,}$";
    GEN_CAMERA = r"(?i)^(IMG|DSC|VID)_?\d+$";
    CR_IOS = r"^\d+(\.\d+)*$";
    CR_AUTO = r"(?i)headless|wkhtmltopdf|pdfium|reportlab|itext|prince|fop|tcpdf|dompdf|puppeteer|skia";
    CR_BROWSER = r"(?i)safari|chrome|chromium|firefox|edge|浏览器";
    CR_WORD = r"(?i)word|pages|writer|wps|docs";
    CR_SHEET = r"(?i)excel|numbers|sheets";
    CR_SCAN = r"(?i)preview|预览|scan|image capture|camscanner";
    CR_SLIDES = r"(?i)powerpoint|keynote";
}

fn hit(r: &Regex, s: &str) -> bool {
    r.is_match(s).unwrap_or(false)
}

/// The name without its last extension, like Python's `Path.stem`.
pub fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => &name[..i],
        _ => name,
    }
}

/// Clues computed locally from the real name. Only these clues are sent, never the name.
pub fn name_features(name: &str) -> Vec<&'static str> {
    let st = stem(name);
    let mut f = Vec::new();
    if hit(&SCREENSHOT, name) {
        f.push("looks like a screenshot");
    }
    if hit(&RECORDING, name) {
        f.push("looks like a screen recording");
    }
    if hit(&COPY_SUFFIX, st) {
        f.push("name has a copy suffix such as (1)");
    }
    if hit(&VERSION, st) {
        f.push("name contains a version number");
    }
    if hit(&INVOICE, name) {
        f.push("name suggests an invoice, receipt or statement");
    }
    if hit(&PERSONAL, name) {
        f.push("name suggests a personal or legal document");
    }
    if hit(&INSTALLER, name) {
        f.push("name suggests an installer");
    }
    if hit(&GEN_HEX, st) || hit(&GEN_CAMERA, st) {
        f.push("name is a generated id, not human-written");
    }
    f
}

/// Bucket the app that made the file; the exact app and version are not sent.
pub fn creator_kind(creator: Option<&str>) -> Option<&'static str> {
    let c = creator?;
    Some(if hit(&CR_IOS, c) {
        "iPhone or iPad" // iOS writes its version number here
    } else if hit(&CR_AUTO, c) {
        "an automated system (generated PDF)"
    } else if hit(&CR_BROWSER, c) {
        "a web browser (saved or printed page)"
    } else if hit(&CR_WORD, c) {
        "a word processor"
    } else if hit(&CR_SHEET, c) {
        "a spreadsheet app"
    } else if hit(&CR_SCAN, c) {
        "a scanner or Preview"
    } else if hit(&CR_SLIDES, c) {
        "a presentation app"
    } else {
        "another app"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features() {
        assert_eq!(name_features("截屏 2026-10-03 08.10.26.png"), ["looks like a screenshot", "name contains a version number"]);
        assert_eq!(name_features("IMG_0840 2.JPG"), ["name has a copy suffix such as (1)"]);
        assert_eq!(name_features("IMG_0840.JPG"), ["name is a generated id, not human-written"]);
        assert!(name_features("Acme invoice 2025.pdf").contains(&"name suggests an invoice, receipt or statement"));
    }

    #[test]
    fn creators() {
        assert_eq!(creator_kind(Some("26.5.2")), Some("iPhone or iPad"));
        assert_eq!(creator_kind(Some("Mozilla/5.0 HeadlessChrome/152.0")), Some("an automated system (generated PDF)"));
        assert_eq!(creator_kind(Some("Safari浏览器")), Some("a web browser (saved or printed page)"));
        assert_eq!(creator_kind(Some("Microsoft® Word for Microsoft 365")), Some("a word processor"));
        assert_eq!(creator_kind(None), None);
    }

    #[test]
    fn buckets() {
        assert_eq!(size_words(5_000_000), "small (under 10 MB)");
        assert_eq!(age_words(Some(0), 100 * DAY), "3-6 months ago");
        assert_eq!(age_words(None, 0), "unknown");
        assert_eq!(stem("a.b.c"), "a.b");
        assert_eq!(stem(".hidden"), ".hidden");
    }
}

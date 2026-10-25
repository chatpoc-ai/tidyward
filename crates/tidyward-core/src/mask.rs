//! Mask a file name so it keeps its shape and meaning but not who or what it is about.
//!
//! Generic words that tell what kind of file it is (IMG, Screenshot, facture, 合同, 請求書, 영수증)
//! are kept; everything else becomes a placeholder:
//!
//! | placeholder                       | stands for                                              |
//! |-----------------------------------|---------------------------------------------------------|
//! | `<date:…>`                        | a date in the name, as how long ago it is               |
//! | `<time>` `<version>` `<id>` `<n>` | clock times, versions, hex/uuid ids, other numbers      |
//! | `<word>`                          | a Latin-script word not in the word lists (names, companies) |
//! | `<中文>` `<かな>` `<한글>`          | Chinese characters (or kanji), kana, Hangul not in the lists |
//! | `<text>`                          | a word in any other script                              |
//!
//! The word lists are in `words/`, one file per language: English, French, Simplified and
//! Traditional Chinese, Japanese, Korean. Latin words match ignoring case and accents; CJK and
//! Hangul runs are split by longest match. Names are normalized to NFC first, since macOS file
//! names may be decomposed.
//!
//! ```text
//! IMG_4821.HEIC             -> IMG_<n>.HEIC
//! Invoice_Acme_2025-09.pdf  -> Invoice_<word>_<date:1-2 years ago>.pdf
//! 张三_劳动合同_签字版.pdf     -> <中文>_劳动合同_签字版.pdf
//! 請求書_山田商事.pdf          -> 請求書_<中文>.pdf
//! ```

use chrono::{Datelike, NaiveDate};
use fancy_regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

const EN: &str = include_str!("../words/en.txt");
const FR: &str = include_str!("../words/fr.txt");
const ZH_HANS: &str = include_str!("../words/zh-hans.txt");
const ZH_HANT: &str = include_str!("../words/zh-hant.txt");
const JA: &str = include_str!("../words/ja.txt");
const KO: &str = include_str!("../words/ko.txt");

fn entries(files: &[&'static str]) -> impl Iterator<Item = String> {
    files
        .iter()
        .flat_map(|f| f.lines())
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .map(|w| w.nfc().collect())
}

/// Lowercase without accents: "Relevé" and "releve" are the same word.
fn fold(w: &str) -> String {
    w.nfd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase()
}

static LATIN_WORDS: LazyLock<HashSet<String>> = LazyLock::new(|| entries(&[EN, FR]).map(|w| fold(&w)).collect());

/// Longest first, so 微信图片 wins over 微信.
fn longest_first(files: &[&'static str]) -> Vec<String> {
    let mut v: Vec<String> = entries(files).collect::<HashSet<_>>().into_iter().collect();
    v.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
    v
}

static CJK_WORDS: LazyLock<Vec<String>> = LazyLock::new(|| longest_first(&[ZH_HANS, ZH_HANT, JA]));
static HANGUL_WORDS: LazyLock<Vec<String>> = LazyLock::new(|| longest_first(&[KO]));

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
  (?P<kw>(?:arm64|aarch64|x86_64|x64|x86|amd64|w2|t4|1099|mp3|mp4|h264|4k|1080p|720p)(?![A-Za-z0-9]))
| (?P<datetime>(?:19|20)\d{6}[_\-T]?\d{6})
| (?P<date>(?:19|20)\d{2}년\s?\d{1,2}월(?:\s?\d{1,2}일)?|(?:19|20)\d{2}[年년](?!\d)|\d{1,2}월\s?\d{1,2}일|\d{4}[-_.年]?\d{1,2}[-_.月]?\d{1,2}日?|\d{1,2}月\d{1,2}日|\d{4}[-_.年]\d{1,2}月?)
| (?P<time>\d{1,2}[.:：]\d{2}(?:[.:：]\d{2})?(?:\s?[AP]M)?)
| (?P<version>v?\d+(?:\.\d+){1,3}[a-z]?\b)
| (?P<id>(?=[0-9a-f-]*[a-f])(?=[0-9a-f-]*\d)[0-9a-f]{8,}(?:-[0-9a-f]{4,})*)
| (?P<n>\d+)
| (?P<latin>\p{Latin}+)
| (?P<cjk>[\p{Han}\p{Hiragana}\p{Katakana}ー々〆]+)
| (?P<hangul>\p{Hangul}+)
| (?P<letters>\p{L}+)
| (?P<other>.)",
    )
    .expect("token regex")
});

static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{Lu}+(?!\p{Ll})|\p{Lu}?\p{Ll}+").unwrap());
static COLLAPSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(<word>|<中文>|<かな>|<한글>|<text>)(?:[\s_\-]*\1)+").unwrap());
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
static YEAR4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}").unwrap());
static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9]+$").unwrap());

const AGES: [(i64, &str); 8] = [
    (1, "today"),
    (7, "this week"),
    (31, "this month"),
    (92, "1-3 months ago"),
    (183, "3-6 months ago"),
    (366, "6-12 months ago"),
    (731, "1-2 years ago"),
    (1827, "2-5 years ago"),
];

/// '2025-09-14', '20250914083015', '2025年9月', '10月24日' -> "1-2 years ago"; None if not a real date.
fn date_words(s: &str, today: NaiveDate) -> Option<&'static str> {
    let mut nums: Vec<i64> = DIGITS
        .find_iter(s)
        .filter_map(|m| m.ok()?.as_str().parse().ok())
        .collect();
    if nums.len() == 1 && s.chars().count() >= 8 {
        // 20250914 or 20250914083015
        let d = nums[0].to_string();
        if d.len() < 8 {
            return None;
        }
        nums = vec![d[..4].parse().ok()?, d[4..6].parse().ok()?, d[6..8].parse().ok()?];
    }
    let no_year = (s.contains('月') || s.contains('월')) && !YEAR4.is_match(s).unwrap_or(false);
    if no_year {
        // 10月24日: no year, assume the last such day
        nums.insert(0, today.year() as i64);
    }
    nums.extend([1, 1]);
    let (y, m, dd) = (nums[0], nums[1], nums[2]);
    let mut d = NaiveDate::from_ymd_opt(y as i32, u32::try_from(m).ok()?, u32::try_from(dd).ok()?)?;
    if d > today && no_year {
        d = NaiveDate::from_ymd_opt(y as i32 - 1, m as u32, dd as u32)?;
    }
    let days = (today - d).num_days();
    if days < 0 {
        return Some("in the future");
    }
    Some(AGES.iter().find(|(lim, _)| days < *lim).map(|(_, w)| *w).unwrap_or("over 5 years ago"))
}

fn latin(w: &str) -> String {
    if LATIN_WORDS.contains(&fold(w)) {
        return w.to_string();
    }
    // camelCase / PascalCase: keep the generic parts, e.g. ScreenRecording, BankStatement
    let parts: Vec<&str> = CAMEL.find_iter(w).filter_map(|m| m.ok().map(|m| m.as_str())).collect();
    if parts.len() > 1 {
        return parts
            .iter()
            .map(|p| if LATIN_WORDS.contains(&fold(p)) { p.to_string() } else { "<word>".into() })
            .collect();
    }
    "<word>".into()
}

/// The placeholder for a character no listed word covers.
fn hidden(c: char) -> &'static str {
    match c {
        '\u{3040}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}' => "<かな>",
        '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{ac00}'..='\u{d7af}' => "<한글>",
        _ => "<中文>",
    }
}

/// Split a run with no spaces (Chinese, Japanese, Korean) into listed words, longest first;
/// everything else becomes placeholders.
fn split_run(run: &str, words: &[String]) -> String {
    let mut out = String::new();
    let mut rest = run;
    let mut last = "";
    while !rest.is_empty() {
        if let Some(hit) = words.iter().find(|t| rest.starts_with(t.as_str())) {
            out.push_str(hit);
            rest = &rest[hit.len()..];
            last = "";
        } else {
            let c = rest.chars().next().unwrap();
            let p = hidden(c);
            if p != last {
                out.push_str(p);
            }
            last = p;
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Split "name.ext" the way the masking expects: the extension is kept only when it looks like one.
fn split_ext(name: &str) -> (&str, String) {
    if name.trim_matches('.').contains('.') && !name.starts_with('.') {
        let (stem, ext) = name.rsplit_once('.').unwrap();
        if ext.chars().count() <= 12 && EXT.is_match(ext).unwrap_or(false) {
            return (stem, format!(".{ext}"));
        }
    }
    (name, String::new())
}

/// The masked name sent to the model instead of the real one.
pub fn mask(name: &str, today: NaiveDate) -> String {
    let name: String = name.nfc().collect();
    let (stem, ext) = split_ext(&name);
    let mut out = String::new();
    for caps in TOKEN.captures_iter(stem).filter_map(Result::ok) {
        let (kind, text) = ["kw", "datetime", "date", "time", "version", "id", "n", "latin", "cjk", "hangul", "letters", "other"]
            .iter()
            .find_map(|k| caps.name(k).map(|m| (*k, m.as_str())))
            .unwrap();
        match kind {
            "kw" => out.push_str(text),
            "date" | "datetime" => {
                match date_words(text, today) {
                    Some(w) => out.push_str(&format!("<date:{w}>")),
                    None => out.push_str("<n>"),
                }
                if kind == "datetime" {
                    out.push_str(" <time>");
                }
            }
            "latin" => out.push_str(&latin(text)),
            "cjk" => out.push_str(&split_run(text, &CJK_WORDS)),
            "hangul" => out.push_str(&split_run(text, &HANGUL_WORDS)),
            "letters" => out.push_str("<text>"),
            "other" => {
                if " _-.()[]+&,@#'’（）「」【】・".contains(text) {
                    out.push_str(text);
                }
            }
            k => out.push_str(&format!("<{k}>")),
        }
    }
    COLLAPSE.replace_all(&out, "$1").into_owned() + &ext
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str) -> String {
        mask(name, NaiveDate::from_ymd_opt(2026, 10, 25).unwrap())
    }

    #[test]
    fn keeps_meaning_hides_identity() {
        assert_eq!(m("IMG_4821.HEIC"), "IMG_<n>.HEIC");
        assert_eq!(m("截屏2026-10-24 10.23.45.png"), "截屏<date:this week> <time>.png");
        assert_eq!(m("张三_劳动合同_签字版.pdf"), "<中文>_劳动合同_签字版.pdf");
        assert_eq!(m("Invoice_Acme_2025-09.pdf"), "Invoice_<word>_<date:1-2 years ago>.pdf");
        assert_eq!(
            m("Screen Recording 2026-10-01 at 9.12.03 AM.mov"),
            "Screen Recording <date:this month> at <time>.mov"
        );
        assert_eq!(m("GoogleChrome-v131.0.6778.86-arm64.dmg"), "<word>-<version>-arm64.dmg");
        assert_eq!(m("3f2a9c1e-77b0-4d1e-9a3b-0c5d2e8f1a6b.jpg"), "<id>.jpg");
        assert_eq!(m("Jane Doe Resume (1).docx"), "<word> Resume (<n>).docx");
        assert_eq!(m("BankStatement_Sept.pdf"), "BankStatement_Sept.pdf");
        assert_eq!(m("微信图片_20260912083015.jpg"), "微信图片_<date:1-3 months ago> <time>.jpg");
        assert_eq!(m("2019年度个税.pdf"), "<date:over 5 years ago><中文>个税.pdf");
        assert_eq!(m("10月24日.mov"), "<date:this week>.mov");
    }

    #[test]
    fn secrets_stay_recognisable() {
        assert_eq!(m("acme_backup_code.txt"), "<word>_backup_code.txt");
        assert_eq!(m("service-recovery-codes.txt"), "<word>-recovery-codes.txt");
        assert_eq!(m("我的钱包助记词.txt"), "<中文>钱包助记词.txt");
    }

    #[test]
    fn five_languages() {
        assert_eq!(m("Capture d’écran 2026-10-24 à 15.22.33.png"), "Capture d’écran <date:this week> à <time>.png");
        assert_eq!(m("Relevé bancaire Desjardins octobre.pdf"), "Relevé bancaire <word> octobre.pdf");
        assert_eq!(m("releve BANCAIRE.pdf"), "releve BANCAIRE.pdf", "accents and case are ignored");
        assert_eq!(m("螢幕截圖 2026-10-24 下午3.22.33.png"), "螢幕截圖 <date:this week> 下午<time>.png");
        assert_eq!(m("陳大文_電費帳單.pdf"), "<中文>_電費帳單.pdf");
        assert_eq!(m("スクリーンショット 2026-10-24 15.22.33.png"), "スクリーンショット <date:this week> <time>.png");
        assert_eq!(m("請求書_山田商事.pdf"), "請求書_<中文>.pdf");
        assert_eq!(m("やまだ たろう 履歴書.docx"), "<かな> 履歴書.docx");
        assert_eq!(m("領収書（コンビニ）.pdf"), "領収書（<かな>）.pdf");
        assert_eq!(m("스크린샷 2026-10-24 오후 3.22.33.png"), "스크린샷 <date:this week> 오후 <time>.png");
        assert_eq!(m("김민수_영수증_2026년 9월 1일.pdf"), "<한글>_영수증_<date:1-3 months ago>.pdf");
        assert_eq!(m("Счёт Иванов.pdf"), "<text>.pdf", "other scripts are hidden, never dropped");
    }

    #[test]
    fn decomposed_names() {
        let nfd = |s: &str| s.nfd().collect::<String>();
        assert_eq!(m(&nfd("Relevé bancaire.pdf")), "Relevé bancaire.pdf");
        assert_eq!(m(&nfd("김민수_영수증.pdf")), "<한글>_영수증.pdf");
    }

    #[test]
    fn edge_cases() {
        assert_eq!(m(".hidden"), ".<word>");
        assert_eq!(m("no extension"), "<word> extension");
        assert_eq!(m("Electricity bill 水电费通知.pdf"), "<word> bill <中文>电费通知.pdf");
        assert_eq!(m("2026-13-40 report.pdf"), "<n> report.pdf");
        assert_eq!(m("archive.tar.gz"), "archive.<word>.gz");
    }
}

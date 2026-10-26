//! Local-only mode: answers to "what it is" conditions without a model.
//!
//! Some conditions can be settled from what the Mac already knows: a screenshot flag, a camera,
//! the file type, or telling words in the real name ("invoice", "护照", "영수증"). A condition is
//! matched to topics by its own words; each topic answers from hard signals or the name. Only
//! sure answers are given. Anything else stays unanswered, and an unanswered condition never
//! makes a rule apply, so nothing is moved on a guess. The real name is read here, on this Mac.

use crate::identify::{Identity, Purpose};
use crate::item::{Item, Kind};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// How sure a local "yes" is: below certain, above the default hit threshold.
pub const LOCAL_YES: f64 = 0.9;

struct Topic {
    /// Words in a condition that mean this topic.
    cond: &'static [&'static str],
    /// Words in a real file name that show it (whole words for Latin script, anywhere for CJK).
    name: &'static [&'static str],
    /// A sure yes or no from hard signals, if there is one.
    test: fn(&Item, &Identity) -> Option<bool>,
}

fn none(_: &Item, _: &Identity) -> Option<bool> {
    None
}

/// Topics about what a document is about: only the name can tell.
const SPECIFIC: &[Topic] = &[
    Topic {
        cond: &["password", "recovery code", "backup code", "seed phrase", "密码", "恢复码", "备份码", "密碼", "復原碼"],
        name: &["password", "passwords", "recovery code", "recovery codes", "backup code", "backup codes", "2fa", "seed phrase",
                "密码", "恢复码", "备份码", "助记词", "密碼", "復原碼", "備份碼", "パスワード", "リカバリーコード", "비밀번호", "복구 코드", "백업 코드"],
        test: none,
    },
    Topic {
        cond: &["passport", "id card", "identity", "driver", "licence", "license", "护照", "身份证", "证件", "驾照", "護照", "證件"],
        name: &["passport", "passeport", "id card", "licence", "license", "driver license", "permis", "visa",
                "护照", "身份证", "驾照", "证件", "護照", "身分證", "駕照", "證件", "パスポート", "免許証", "在留カード", "여권", "신분증", "운전면허"],
        test: none,
    },
    Topic {
        cond: &["tax", "assessment", "税", "稅"],
        name: &["tax", "taxes", "t4", "t4a", "t5", "noa", "notice of assessment", "impot", "impots", "cotisation",
                "税", "报税", "个税", "稅", "報稅", "確定申告", "源泉徴収", "세금", "연말정산"],
        test: none,
    },
    Topic {
        cond: &["invoice", "receipt", "bill", "statement", "发票", "收据", "账单", "發票", "收據", "帳單"],
        name: &["invoice", "invoices", "receipt", "receipts", "bill", "statement", "facture", "recu", "releve",
                "发票", "收据", "账单", "对账单", "發票", "收據", "帳單", "請求書", "領収書", "明細", "영수증", "청구서", "명세서"],
        test: none,
    },
    Topic {
        cond: &["contract", "lease", "agreement", "合同", "协议", "合約", "契約"],
        name: &["contract", "agreement", "lease", "nda", "contrat", "bail", "entente",
                "合同", "协议", "租约", "合約", "協議", "租約", "契約", "계약"],
        test: none,
    },
    Topic {
        cond: &["resume", "cv", "简历", "履歷", "履歴書"],
        name: &["resume", "cv", "curriculum", "简历", "履历", "簡歷", "履歷", "履歴書", "職務経歴書", "이력서"],
        test: none,
    },
    Topic {
        cond: &["school", "exam", "homework", "学校", "考试", "作业", "學校", "考試", "作業", "試験", "시험"],
        name: &["exam", "exams", "homework", "assignment", "quiz", "worksheet", "transcript", "report card", "school",
                "examen", "devoir", "ecole", "考试", "期末", "期中", "作业", "试卷", "测验", "学校", "成绩单",
                "考試", "作業", "試卷", "學校", "成績單", "試験", "宿題", "学校", "시험", "숙제", "학교"],
        test: none,
    },
    Topic {
        cond: &["unfinished", "partial download", "incomplete download", "没下载完", "未完成"],
        name: &[],
        test: |i, id| Some(id.purpose == Purpose::Temporary && id.source == "local" || i.ext == ".crdownload" || i.ext == ".part"),
    },
];

/// Topics about the kind of file: the system and the file type can tell.
const GENERAL: &[Topic] = &[
    Topic {
        cond: &["screenshot", "screen capture", "screen recording", "截图", "截屏", "截圖", "スクリーンショット", "스크린샷"],
        name: &[],
        test: |i, id| {
            if id.source == "local" {
                Some(id.purpose == Purpose::Screenshot)
            } else if i.camera.is_some() || !matches!(i.kind, Kind::Image | Kind::Video) {
                Some(false)
            } else {
                None
            }
        },
    },
    Topic {
        cond: &["photo", "照片", "相片", "写真", "사진"],
        name: &[],
        test: |i, _| {
            if i.camera.is_some() {
                Some(true)
            } else if i.kind != Kind::Image || i.screen_capture {
                Some(false)
            } else {
                None
            }
        },
    },
    Topic { cond: &["video", "movie", "视频", "影片", "ビデオ", "동영상"], name: &[], test: |i, _| Some(i.kind == Kind::Video) },
    Topic { cond: &["music", "audio", "song", "音乐", "音频", "音樂", "音楽", "음악"], name: &[], test: |i, _| Some(i.kind == Kind::Audio) },
    Topic { cond: &["zip", "archive", "压缩", "壓縮"], name: &[], test: |i, _| Some(i.kind == Kind::Archive) },
    Topic { cond: &["installer", "安装", "安裝"], name: &[], test: |i, _| Some(i.kind == Kind::Installer) },
    Topic { cond: &["document", "文档", "文件", "書類", "문서"], name: &[], test: |i, _| Some(i.kind == Kind::Document) },
];

/// Lowercase, no accents, separators as spaces.
fn fold(s: &str) -> String {
    let plain: String = s.nfd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase();
    plain.chars().map(|c| if c.is_alphanumeric() || !c.is_ascii() { c } else { ' ' }).collect()
}

fn is_cjk(w: &str) -> bool {
    w.chars().any(|c| c as u32 >= 0x2E80)
}

/// Whole words for Latin script, anywhere for Chinese, Japanese and Korean.
fn has_word(text: &str, word: &str) -> bool {
    if is_cjk(word) {
        return text.contains(word);
    }
    let padded = format!(" {text} ");
    padded.contains(&format!(" {word} "))
}

fn answer(t: &Topic, item: &Item, id: &Identity, name: &str, inner: &[String]) -> Option<bool> {
    if let Some(sure) = (t.test)(item, id) {
        return Some(sure);
    }
    // a name can say yes; it can't say no. For a folder or archive, so can most of the names inside.
    let says = |n: &str| t.name.iter().any(|w| has_word(n, w));
    let inner_hits = inner.iter().filter(|n| says(n)).count();
    (says(name) || inner_hits * 2 > inner.len()).then_some(true)
}

/// The probability that `cond` holds for the item, if it can be settled locally.
///
/// A condition about what a document is about (an invoice, a passport) is decided by those
/// topics only, even if it also names a general kind ("a photo or scan of a passport" is not
/// every photo). Any sure yes wins; all sure noes give 0; otherwise there is no answer.
pub fn answer_condition(cond: &str, item: &Item, id: &Identity) -> Option<f64> {
    let c = cond.to_lowercase();
    let pick = |ts: &'static [Topic]| ts.iter().filter(|t| t.cond.iter().any(|w| c.contains(w))).collect::<Vec<_>>();
    let mut topics = pick(SPECIFIC);
    if topics.is_empty() {
        topics = pick(GENERAL);
    }
    if topics.is_empty() {
        return None;
    }
    let name = fold(&item.name);
    let inner: Vec<String> = item.inside.iter().flat_map(|i| &i.names).map(|n| fold(n)).collect();
    let answers: Vec<Option<bool>> = topics.iter().map(|t| answer(t, item, id, &name, &inner)).collect();
    if answers.contains(&Some(true)) {
        Some(LOCAL_YES)
    } else if answers.iter().all(|a| *a == Some(false)) {
        Some(0.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::tests::item;

    fn unknown() -> Identity {
        Identity { purpose: Purpose::Other, confidence: 0.0, source: "unknown".into(), reason: String::new() }
    }

    fn local(p: Purpose) -> Identity {
        Identity { purpose: p, confidence: 1.0, source: "local".into(), reason: String::new() }
    }

    #[test]
    fn names_say_yes_never_no() {
        let id = unknown();
        let ask = |name: &str, cond: &str| answer_condition(cond, &item(name), &id);
        assert_eq!(ask("Invoice_Acme_2025-09.pdf", "an invoice, receipt or bill"), Some(LOCAL_YES));
        assert_eq!(ask("請求書_山田商事.pdf", "an invoice, receipt or bill"), Some(LOCAL_YES));
        assert_eq!(ask("Relevé bancaire.pdf", "an invoice, receipt, bill or bank statement"), Some(LOCAL_YES));
        assert_eq!(ask("github-recovery-codes.txt", "passwords, keys, recovery codes or backup codes"), Some(LOCAL_YES));
        assert_eq!(ask("JANE-PASSPORT.jpg", "a photo or scan of a passport, ID card or driver's licence"), Some(LOCAL_YES));
        assert_eq!(ask("notes.pdf", "an invoice, receipt or bill"), None, "no telling word: no answer, not a no");
        assert_eq!(ask("billing-report.pdf", "an invoice, receipt or bill"), None, "whole words only");
        assert_eq!(ask("x.pdf", "something only a model could judge"), None);
        assert_eq!(ask("Jane_中文期末考试_AB卷_1.pdf", "schoolwork, an exam or a school notice"), Some(LOCAL_YES));
        assert_eq!(ask("Jane_Doe_Resume_ZH.pdf", "a resume or CV"), Some(LOCAL_YES));
    }

    #[test]
    fn most_names_inside_can_say_yes() {
        let bills = "an invoice, receipt or bill";
        let with = |paths: &[&str]| {
            let mut z = item("files.zip");
            z.inside = Some(crate::item::Inside::from_paths(paths.iter().map(|s| s.to_string())));
            answer_condition(bills, &z, &unknown())
        };
        assert_eq!(with(&["Invoice 1.pdf", "Invoice 2.pdf", "notes.txt"]), Some(LOCAL_YES));
        assert_eq!(with(&["Invoice 1.pdf", "photo.jpg", "notes.txt"]), None, "one invoice does not make the zip one");
    }

    #[test]
    fn hard_signals_answer_both_ways() {
        let shot = item("Screen Shot.png");
        assert_eq!(answer_condition("a screenshot", &shot, &local(Purpose::Screenshot)), Some(LOCAL_YES));
        let mut photo = item("IMG_0001.HEIC");
        photo.kind = Kind::Image;
        photo.camera = Some("iPhone".into());
        assert_eq!(answer_condition("a screenshot", &photo, &local(Purpose::Photo)), Some(0.0));
        assert_eq!(answer_condition("a photo taken with a phone or camera", &photo, &local(Purpose::Photo)), Some(LOCAL_YES));
        // a camera photo is not therefore a passport scan
        assert_eq!(answer_condition("a photo or scan of a passport, ID card or driver's licence", &photo, &local(Purpose::Photo)), None);
        let mut mov = item("trip.mov");
        mov.kind = Kind::Video;
        assert_eq!(answer_condition("a video", &mov, &unknown()), Some(LOCAL_YES));
        assert_eq!(answer_condition("a document", &mov, &unknown()), Some(0.0));
    }
}

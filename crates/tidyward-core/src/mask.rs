//! Mask a file name so it keeps its shape and meaning but not who or what it is about.
//!
//! Generic words that tell what kind of file it is (IMG, Screenshot, invoice, 合同, setup) are
//! kept; everything else becomes a placeholder:
//!
//! | placeholder                       | stands for                                              |
//! |-----------------------------------|---------------------------------------------------------|
//! | `<date:…>`                        | a date in the name, as how long ago it is               |
//! | `<time>` `<version>` `<id>` `<n>` | clock times, versions, hex/uuid ids, other numbers      |
//! | `<word>`                          | any Latin word not in the allowlist (names, companies)  |
//! | `<中文>`                           | any Chinese run not in the allowlist                    |
//!
//! ```text
//! IMG_4821.HEIC             -> IMG_<n>.HEIC
//! Invoice_Acme_2025-09.pdf  -> Invoice_<word>_<date:1-2 years ago>.pdf
//! 张三_劳动合同_签字版.pdf     -> <中文>_劳动合同_签字版.pdf
//! ```

use chrono::{Datelike, NaiveDate};
use fancy_regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

const WORDS: &str = "
img image images dsc dscn dscf pxl mvimg vid video photo photos pic pics picture scan scanned scanner
screenshot screenshots screen shot recording cleanshot snip snapshot capture
wechat weixin whatsapp telegram signal mmexport messenger line kakaotalk
invoice invoices receipt receipts bill bills billing statement statements payment payments payslip paystub
salary bank tax taxes return w2 t4 1099 refund quote quotation order orders purchase estimate
contract contracts agreement agreements lease nda terms offer letter resume cv cover passport visa id
license licence certificate cert diploma transcript report reports form forms application signed
signature insurance coverage benefit benefits booklet policy claim medical lab prescription ticket tickets boarding pass itinerary booking
reservation confirmation hotel flight
manual guide handbook tutorial notes note slides deck presentation paper whitepaper article ebook book
chapter readme draft final copy version rev revised old new updated summary minutes agenda meeting
setup install installer mac macos osx darwin arm arm64 aarch64 x64 x86 amd64 intel universal win windows
linux dmg pkg release beta alpha update upgrade patch driver drivers plugin extension app desktop cli
backup export exported import data dataset db dump log logs temp tmp cache download downloads archive
sample samples test demo example template mockup design logo icon icons wallpaper avatar banner poster
untitled unnamed document doc file files part partial crdownload
audio music song songs movie episode voice memo podcast
model stl gcode print plate
a an the at of and or for to in on by with from my your our de
password passwords passwd pwd key keys secret secrets token tokens code codes recovery 2fa otp mfa
seed phrase wallet credential credentials login
jan feb mar apr may jun jul aug sep sept oct nov dec january february march april june july august
september october november december
";

const ZH: &str = "
截屏 截图 屏幕快照 屏幕截图 屏幕录制 录屏 照片 相片 图片 图像 扫描 扫描件 拍照
微信 微信图片 企业微信 钉钉 飞书 QQ
发票 电子发票 收据 账单 对账单 流水 银行 电费 水费 燃气费 话费 网费 物业费 房租 工资 工资条 付款 收款 报销 订单 报价 报价单 采购 退款
合同 协议 劳动合同 租房 租赁 简历 护照 签证 身份证 驾照 证书 证明 毕业证 成绩单 申请 申请表 表格
签字 签名 签字版 盖章 保险 保单 福利 理赔 体检 病历 处方 机票 车票 行程 行程单 预订 确认 确认函 酒店
报税 税 个税 社保 公积金 通知 学校 作业 课件 考试 论文 报告 说明书 手册 教程 笔记 会议 会议纪要
草稿 初稿 终稿 最终版 修改 修订 副本 备份 导出 数据 日志 临时 下载 模板 设计 壁纸 头像 海报 未命名
密码 密钥 秘钥 恢复码 备份码 验证码 助记词 私钥 钱包 账号 安装 安装包 更新 驱动 视频 音频 录音 音乐 歌曲 电影 文档 文件 资料 演示 幻灯片 新建 年度 季度 年 月 日
";

static WORD_SET: LazyLock<HashSet<&'static str>> = LazyLock::new(|| WORDS.split_whitespace().collect());

/// Longest first, so 微信图片 wins over 微信.
static ZH_LIST: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut v: Vec<&str> = ZH.split_whitespace().collect::<HashSet<_>>().into_iter().collect();
    v.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then(a.cmp(b)));
    v
});

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
  (?P<kw>(?:arm64|aarch64|x86_64|x64|x86|amd64|w2|t4|1099|mp3|mp4|h264|4k|1080p|720p)(?![A-Za-z0-9]))
| (?P<datetime>(?:19|20)\d{6}[_\-T]?\d{6})
| (?P<date>(?:19|20)\d{2}年(?!\d)|\d{4}[-_.年]?\d{1,2}[-_.月]?\d{1,2}日?|\d{1,2}月\d{1,2}日|\d{4}[-_.年]\d{1,2}月?)
| (?P<time>\d{1,2}[.:：]\d{2}(?:[.:：]\d{2})?(?:\s?[AP]M)?)
| (?P<version>v?\d+(?:\.\d+){1,3}[a-z]?\b)
| (?P<id>(?=[0-9a-f-]*[a-f])(?=[0-9a-f-]*\d)[0-9a-f]{8,}(?:-[0-9a-f]{4,})*)
| (?P<n>\d+)
| (?P<latin>[A-Za-z]+)
| (?P<zh>[一-鿿]+)
| (?P<other>.)",
    )
    .expect("token regex")
});

static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+").unwrap());
static COLLAPSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(<word>|<中文>)(?:[\s_\-]*\1)+").unwrap());
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
    let no_year = s.contains('月') && !YEAR4.is_match(s).unwrap_or(false);
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
    if WORD_SET.contains(w.to_lowercase().as_str()) {
        return w.to_string();
    }
    // camelCase / PascalCase: keep the generic parts, e.g. ScreenRecording, BankStatement
    let parts: Vec<&str> = CAMEL.find_iter(w).filter_map(|m| m.ok().map(|m| m.as_str())).collect();
    if parts.len() > 1 {
        return parts
            .iter()
            .map(|p| if WORD_SET.contains(p.to_lowercase().as_str()) { p.to_string() } else { "<word>".into() })
            .collect();
    }
    "<word>".into()
}

fn zh(run: &str) -> String {
    let mut out = String::new();
    let mut rest = run;
    let mut gap = false;
    while !rest.is_empty() {
        if let Some(hit) = ZH_LIST.iter().find(|t| rest.starts_with(**t)) {
            out.push_str(hit);
            rest = &rest[hit.len()..];
            gap = false;
        } else {
            if !gap {
                out.push_str("<中文>");
            }
            gap = true;
            let c = rest.chars().next().unwrap();
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
    let (stem, ext) = split_ext(name);
    let mut out = String::new();
    for caps in TOKEN.captures_iter(stem).filter_map(Result::ok) {
        let (kind, text) = ["kw", "datetime", "date", "time", "version", "id", "n", "latin", "zh", "other"]
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
            "zh" => out.push_str(&zh(text)),
            "other" => {
                if " _-.()[]+&,@#".contains(text) {
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
    fn edge_cases() {
        assert_eq!(m(".hidden"), ".<word>");
        assert_eq!(m("no extension"), "<word> extension");
        assert_eq!(m("Electricity bill 水电费通知.pdf"), "<word> bill <中文>电费通知.pdf");
        assert_eq!(m("2026-13-40 report.pdf"), "<n> report.pdf");
        assert_eq!(m("archive.tar.gz"), "archive.<word>.gz");
    }
}

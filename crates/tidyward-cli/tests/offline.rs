//! End to end, offline: rules with exact conditions only, so no model and no key is needed.
//! Everything happens in temporary folders; nothing goes to the Trash.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::io::Write;

fn tidyward(home: &Path, args: &[&str], stdin: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tidyward"))
        .arg("--home")
        .arg(home)
        .args(args)
        .env_remove("TYPESAFE_API_KEY")
        .env_remove("AI_GATEWAY_API_KEY")
        .env_remove("TIDYWARD_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "tidyward {args:?} failed:\n{text}");
    text
}

#[test]
fn scan_remember_apply_skip_undo() {
    let t = tempfile::tempdir().unwrap();
    let (home, inbox, root) = (t.path().join("home"), t.path().join("Inbox"), t.path().join("root"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&inbox).unwrap();
    fs::write(inbox.join("part.stl"), b"solid a").unwrap();
    fs::write(inbox.join("part (1).stl"), b"solid a").unwrap();
    fs::write(inbox.join("clip.mov"), b"mov").unwrap();
    fs::write(inbox.join("tool-1.0.dmg"), b"dmg").unwrap();
    fs::write(
        home.join("rules.txt"),
        "archive:Arch/Dupes | * | has duplicate\narchive:Arch/3D | * | extension=.stl\narchive:Arch/Video | * | 扩展名=.mov\nkeep | * |\n",
    )
    .unwrap();
    fs::write(
        home.join("config.toml"),
        format!("folders = [{:?}]\narchive_root = {:?}\nmagika = false\n", inbox, root),
    )
    .unwrap();

    let first = tidyward(&home, &["scan"], "");
    assert!(first.contains("4 items (0 unchanged, 0 moved, 4 new or changed)"), "{first}");
    assert!(first.contains("model: 0 calls"), "{first}");
    let second = tidyward(&home, &["scan"], "");
    assert!(second.contains("4 items (4 unchanged"), "{second}");

    let plan = tidyward(&home, &["plan"], "");
    assert!(plan.contains("Nothing has been moved"), "{plan}");

    // edit the video after the plan: it must be skipped even though the answer is yes
    fs::write(inbox.join("clip.mov"), b"edited").unwrap();
    let applied = tidyward(&home, &["apply"], "y\ny\ny\n");
    assert!(applied.contains("2 items moved, 1 skipped"), "{applied}");
    assert!(root.join("Arch/3D").read_dir().unwrap().count() == 1);
    assert!(root.join("Arch/Dupes").read_dir().unwrap().count() == 1);
    assert!(inbox.join("clip.mov").exists() && inbox.join("tool-1.0.dmg").exists());

    let undone = tidyward(&home, &["undo"], "");
    assert!(undone.contains("2 operations undone"), "{undone}");
    for f in ["part.stl", "part (1).stl", "clip.mov", "tool-1.0.dmg"] {
        assert!(inbox.join(f).exists(), "{f} is back");
    }

    let declined = tidyward(&home, &["apply"], "n\nn\nn\n");
    assert!(declined.contains("0 items moved"), "{declined}");
}

#[test]
fn local_only_mode_needs_no_key() {
    let t = tempfile::tempdir().unwrap();
    let (home, inbox, root) = (t.path().join("home"), t.path().join("Inbox"), t.path().join("root"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&inbox).unwrap();
    fs::write(inbox.join("Invoice_Acme_2025-09.pdf"), b"%PDF").unwrap();
    fs::write(inbox.join("notes.pdf"), b"%PDF").unwrap();
    fs::write(inbox.join("clip.mov"), b"mov").unwrap();
    fs::write(
        home.join("rules.txt"),
        "archive:Receipts | an invoice, receipt or bill |\narchive:Video | a video |\nkeep | * |\n",
    )
    .unwrap();
    fs::write(
        home.join("config.toml"),
        format!("provider = \"local\"\nfolders = [{:?}]\narchive_root = {:?}\nmagika = false\n", inbox, root),
    )
    .unwrap();
    let scan = tidyward(&home, &["scan"], "");
    assert!(scan.contains("model: 0 calls"), "{scan}");
    let plan = tidyward(&home, &["plan"], "");
    // the name says invoice and the type says video: settled on this Mac; notes.pdf is left alone
    assert!(plan.contains("archive → Receipts") && plan.contains("Invoice_Acme_2025-09.pdf"), "{plan}");
    assert!(plan.contains("archive → Video") && plan.contains("[this Mac]"), "{plan}");
    let keep = plan.split("## keep").nth(1).unwrap_or_default();
    assert!(keep.contains("notes.pdf"), "{plan}");
}

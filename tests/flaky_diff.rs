use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_heimdall");

const BRANCHY: &str = "if [ -e flag ]; then cat extra.txt; fi > /dev/null; cat seed.txt > out.txt";
const TEMPY: &str = "t=$$(mktemp ./tmp.XXXXXX); echo x > $$t; rm $$t; cat seed.txt > out.txt";

fn scratch(name: &str, cmd: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("heimdall-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seed.txt"), "seed\n").unwrap();
    fs::write(dir.join("extra.txt"), "extra\n").unwrap();
    fs::write(
        dir.join("build.ninja"),
        format!("rule gen\n  command = $cmd\n\nbuild out.txt: gen seed.txt\n  cmd = {cmd}\n"),
    )
    .unwrap();
    dir
}

fn trace(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(name);
    let ran = Command::new(BIN)
        .args(["run", "--out"])
        .arg(&file)
        .args(["ninja", "-C"])
        .arg(dir)
        .status()
        .unwrap();
    assert!(ran.success());
    file
}

fn clean(dir: &Path) {
    let out = Command::new("ninja")
        .arg("-C")
        .arg(dir)
        .args(["-t", "clean"])
        .output()
        .unwrap();
    assert!(out.status.success());
}

fn compare(first: &Path, second: &Path) -> String {
    let out = Command::new(BIN)
        .arg("diff")
        .arg(first)
        .arg(second)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn changed_read_shows_up_in_diff() {
    let dir = scratch("branchy", BRANCHY);
    let first = trace(&dir, "one.jsonl");
    clean(&dir);
    fs::write(dir.join("flag"), "").unwrap();
    let second = trace(&dir, "two.jsonl");
    let text = compare(&first, &second);
    assert!(text.contains("commands that differ: 1"), "{text}");
    assert!(text.contains("+ read ") && text.contains("extra.txt"), "{text}");
}

#[test]
fn identical_runs_do_not_differ() {
    let dir = scratch("same", BRANCHY);
    let first = trace(&dir, "one.jsonl");
    clean(&dir);
    let second = trace(&dir, "two.jsonl");
    let text = compare(&first, &second);
    assert!(text.contains("commands that differ: 0"), "{text}");
}

#[test]
fn random_scratch_names_are_ignored() {
    let dir = scratch("tempy", TEMPY);
    let first = trace(&dir, "one.jsonl");
    clean(&dir);
    let second = trace(&dir, "two.jsonl");
    let text = compare(&first, &second);
    assert!(text.contains("commands that differ: 0"), "{text}");
}

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_heimdall");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("heimdall-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn findings(dir: &PathBuf) -> Vec<String> {
    let trace = dir.join("trace.jsonl");
    let ran = Command::new(BIN)
        .args(["run", "--out"])
        .arg(&trace)
        .arg("ninja")
        .arg("-C")
        .arg(dir)
        .status()
        .unwrap();
    assert!(ran.success());
    let out = Command::new(BIN)
        .arg("actions")
        .arg(&trace)
        .arg("--dir")
        .arg(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.split("project level undeclared reads:")
        .nth(1)
        .unwrap()
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn undeclared_read_is_reported() {
    let dir = scratch("hidden");
    fs::write(dir.join("declared.txt"), "declared\n").unwrap();
    fs::write(dir.join("hidden.txt"), "secret\n").unwrap();
    fs::write(
        dir.join("build.ninja"),
        "rule gen\n  command = cat declared.txt hidden.txt > $out\n\nbuild out.txt: gen declared.txt\n",
    )
    .unwrap();
    let found = findings(&dir);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("hidden.txt"));
    assert!(!found[0].contains("declared.txt"));
}

#[test]
fn declared_only_build_is_clean() {
    let dir = scratch("clean");
    fs::write(dir.join("declared.txt"), "declared\n").unwrap();
    fs::write(
        dir.join("build.ninja"),
        "rule gen\n  command = cat declared.txt > $out\n\nbuild out.txt: gen declared.txt\n",
    )
    .unwrap();
    assert!(findings(&dir).is_empty());
}

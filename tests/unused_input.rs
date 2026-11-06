use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_heimdall");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("heimdall-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seed.txt"), "seed\n").unwrap();
    fs::write(dir.join("extra.txt"), "extra\n").unwrap();
    dir
}

fn report(dir: &PathBuf) -> String {
    let trace = dir.join("trace.jsonl");
    let ran = Command::new(BIN)
        .args(["run", "--out"])
        .arg(&trace)
        .args(["ninja", "-j1", "-C"])
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn graph(cmd: &str) -> String {
    format!(
        "rule gen\n  command = $cmd\n\nbuild out.txt: gen seed.txt extra.txt\n  cmd = {cmd}\n\ndefault out.txt\n"
    )
}

#[test]
fn declared_input_never_read_is_reported() {
    let dir = scratch("unused");
    fs::write(dir.join("build.ninja"), graph("cat seed.txt > out.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("declared inputs never touched: 1"), "{text}");
    assert!(text.contains("out.txt  ->  ") && text.contains("extra.txt"), "{text}");
}

#[test]
fn declared_input_that_is_read_is_not_reported() {
    let dir = scratch("used");
    fs::write(dir.join("build.ninja"), graph("cat seed.txt extra.txt > out.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("declared inputs never touched: 0"), "{text}");
    assert!(!text.contains("out.txt  ->  "), "{text}");
}

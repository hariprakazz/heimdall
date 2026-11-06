use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_heimdall");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("heimdall-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("seed.txt"), "seed\n").unwrap();
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

fn graph(last: &str) -> String {
    format!(
        "rule gen\n  command = $cmd\n\nbuild a.txt: gen seed.txt\n  cmd = cat seed.txt > a.txt\n\nbuild mid.txt: gen a.txt\n  cmd = cat a.txt > mid.txt\n\nbuild b.txt: gen {last}\n  cmd = cat mid.txt a.txt > b.txt\n\ndefault b.txt\n"
    )
}

#[test]
fn read_through_a_transitive_dependency_is_not_reported() {
    let dir = scratch("transitive");
    fs::write(dir.join("build.ninja"), graph("mid.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("missing dependency edges: 0"), "{text}");
    assert!(!text.contains("add \"|"), "{text}");
}


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

fn graph(second: &str) -> String {
    format!(
        "rule gen\n  command = $cmd\n\nbuild a.txt: gen seed.txt\n  cmd = echo made > a.txt\n\nbuild b.txt: gen {second}\n  cmd = cat a.txt > b.txt\n\ndefault a.txt b.txt\n"
    )
}

#[test]
fn read_of_another_edges_output_is_reported() {
    let dir = scratch("missing");
    fs::write(dir.join("build.ninja"), graph("seed.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("missing dependency edges: 1"), "{text}");
    assert!(text.contains("b.txt  <-  ") && text.contains("(written by a.txt)"), "{text}");
}

#[test]
fn declared_producer_is_not_reported() {
    let dir = scratch("ordered");
    fs::write(dir.join("build.ninja"), graph("seed.txt a.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("missing dependency edges: 0"), "{text}");
}

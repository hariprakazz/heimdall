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

fn graph(cmd: &str) -> String {
    format!(
        "rule gen\n  command = $cmd\n\nbuild gen.txt: gen\n  cmd = {cmd}\n\ndefault gen.txt\n"
    )
}

#[test]
fn undeclared_read_in_edge_without_inputs_is_reported() {
    let dir = scratch("noin-read");
    fs::write(dir.join("build.ninja"), graph("cat seed.txt > gen.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("matched to an edge: 1"), "{text}");
    let tail = text.rsplit("project level undeclared reads:").next().unwrap();
    assert!(tail.contains("gen.txt  <-  ") && tail.contains("seed.txt"), "{text}");
}

#[test]
fn edge_without_inputs_that_reads_nothing_is_quiet() {
    let dir = scratch("noin-quiet");
    fs::write(dir.join("build.ninja"), graph("echo hi > gen.txt")).unwrap();
    let text = report(&dir);
    assert!(text.contains("matched to an edge: 1"), "{text}");
    let tail = text.rsplit("project level undeclared reads:").next().unwrap();
    assert!(!tail.contains("gen.txt  <-  "), "{text}");
}

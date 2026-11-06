use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_heimdall");

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("heimdall-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("target.txt"), "target\n").unwrap();
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

#[test]
fn symlink_step_does_not_report_its_target_as_unused() {
    let dir = scratch("symlink");
    let graph = "rule link\n  command = ln -sf target.txt link.txt\n\nbuild link.txt: link target.txt\n\ndefault link.txt\n";
    fs::write(dir.join("build.ninja"), graph).unwrap();
    let text = report(&dir);
    assert!(text.contains("declared inputs never touched: 0"), "{text}");
}

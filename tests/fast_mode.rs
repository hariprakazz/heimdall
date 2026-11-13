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

#[test]
fn fast_mode_reports_undeclared_read() {
    let dir = scratch("fast");
    fs::write(dir.join("declared.txt"), "declared\n").unwrap();
    fs::write(dir.join("hidden.txt"), "secret\n").unwrap();
    fs::write(
        dir.join("build.ninja"),
        "rule gen\n  command = cat declared.txt hidden.txt > $out\n\nbuild out.txt: gen declared.txt\n",
    )
    .unwrap();
    let trace = dir.join("trace.jsonl");
    let ran = Command::new(BIN)
        .env("HEIMDALL_FAST", "1")
        .args(["run", "--out"])
        .arg(&trace)
        .arg("ninja")
        .arg("-C")
        .arg(&dir)
        .status()
        .unwrap();
    assert!(ran.success());
    let out = Command::new(BIN)
        .arg("actions")
        .arg(&trace)
        .arg("--dir")
        .arg(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let section = text.split("project level undeclared reads:").nth(1).unwrap_or("");
    assert!(section.contains("hidden.txt"), "{text}");
}

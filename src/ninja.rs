use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    command: String,
    output: String,
}

pub struct Edge {
    pub command: String,
    pub output: String,
}

pub fn edges(dir: &Path) -> Result<Vec<Edge>> {
    let out = Command::new("ninja")
        .arg("-C")
        .arg(dir)
        .args(["-t", "compdb"])
        .output()
        .context("cannot run ninja")?;
    if !out.status.success() {
        bail!("ninja -t compdb failed");
    }
    let entries: Vec<Entry> = serde_json::from_slice(&out.stdout)?;
    Ok(entries
        .into_iter()
        .filter(|ent| !ent.command.is_empty())
        .map(|ent| Edge { command: ent.command, output: ent.output })
        .collect())
}

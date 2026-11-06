use std::collections::HashSet;
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

fn tool(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("ninja")
        .arg("-C")
        .arg(dir)
        .args(["-t"])
        .args(args)
        .output()
        .context("cannot run ninja")?;
    if !out.status.success() {
        bail!("ninja -t {} failed", args.join(" "));
    }
    Ok(out.stdout)
}

pub fn edges(dir: &Path) -> Result<Vec<Edge>> {
    let entries: Vec<Entry> = serde_json::from_slice(&tool(dir, &["compdb"])?)?;
    let mut list: Vec<Edge> = entries
        .into_iter()
        .filter(|ent| !ent.command.is_empty())
        .map(|ent| Edge { command: ent.command, output: ent.output })
        .collect();

    let mut cmds: HashSet<String> = list.iter().map(|edge| edge.command.clone()).collect();
    let outs: HashSet<String> = list.iter().map(|edge| edge.output.clone()).collect();
    let targets = String::from_utf8_lossy(&tool(dir, &["targets", "all"])?).into_owned();
    for line in targets.lines() {
        let Some((out, rule)) = line.split_once(": ") else {
            continue;
        };
        if rule == "phony" || outs.contains(out) {
            continue;
        }
        let text = String::from_utf8_lossy(&tool(dir, &["commands", "-s", out])?).into_owned();
        let Some(cmd) = text.lines().last().map(str::trim).filter(|cmd| !cmd.is_empty()) else {
            continue;
        };
        if cmds.insert(cmd.to_string()) {
            list.push(Edge { command: cmd.to_string(), output: out.to_string() });
        }
    }
    Ok(list)
}

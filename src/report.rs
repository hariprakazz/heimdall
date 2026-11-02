use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::event::{decode, Access, Event};

const SYSTEM_TOPS: [&str; 9] = ["usr", "lib", "lib64", "etc", "proc", "dev", "sys", "bin", "sbin"];

fn bucket(path: &str) -> Option<String> {
    let mut parts = path.split('/').filter(|seg| !seg.is_empty());
    let top = parts.next()?;
    if !SYSTEM_TOPS.contains(&top) {
        return None;
    }
    if top == "usr" {
        if let Some(sub) = parts.next() {
            return Some(format!("/usr/{sub}"));
        }
    }
    Some(format!("/{top}"))
}

fn within(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .map_or(false, |rest| rest.is_empty() || rest.starts_with('/'))
}

fn show(title: &str, items: &BTreeSet<String>) {
    println!("{title}: {}", items.len());
    for item in items {
        println!("  {item}");
    }
}

pub fn run(file: &Path, root: &str) -> Result<()> {
    let root = root.trim_end_matches('/');
    let src = File::open(file).with_context(|| format!("cannot open {}", file.display()))?;
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    let mut outside = BTreeSet::new();
    let mut system: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut misses = 0;

    for line in BufReader::new(src).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Event::Access { kind, path, ok, .. } = decode(&line)? else {
            continue;
        };
        if !ok {
            misses += 1;
            continue;
        }
        if let Some(group) = bucket(&path) {
            system.entry(group).or_default().insert(path);
        } else if within(&path, root) {
            match kind {
                Access::Write | Access::Remove => writes.insert(path),
                _ => reads.insert(path),
            };
        } else {
            outside.insert(path);
        }
    }

    println!("root: {root}");
    show("written inside root", &writes);
    show("read inside root", &reads);
    show("outside root, not system", &outside);
    println!("system paths: {}", system.values().map(|set| set.len()).sum::<usize>());
    for (group, set) in &system {
        println!("  {group}  {}", set.len());
    }
    println!("failed lookups: {misses}");
    Ok(())
}

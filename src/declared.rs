use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::resolve::normalize;

fn ninja(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("ninja")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("cannot run ninja")?;
    if !out.status.success() {
        bail!("ninja {} failed", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_deps(text: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(key) = &cur {
                map.entry(key.clone()).or_default().push(line.trim().to_string());
            }
        } else if let Some(pos) = line.find(": #deps") {
            let key = line[..pos].to_string();
            map.entry(key.clone()).or_default();
            cur = Some(key);
        } else {
            cur = None;
        }
    }
    map
}

fn parse_query(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("input:") {
            inside = true;
            continue;
        }
        if t.starts_with("outputs:") {
            break;
        }
        if !inside || t.is_empty() || t.starts_with("||") {
            continue;
        }
        found.push(t.strip_prefix("| ").unwrap_or(t).to_string());
    }
    found
}

pub fn load(dir: &Path, outputs: &[String]) -> Result<HashMap<String, HashSet<String>>> {
    let base = std::fs::canonicalize(dir)?;
    let deps = parse_deps(&ninja(&base, &["-t", "deps"])?);
    let mut declared = HashMap::new();
    for out in outputs {
        let mut set = HashSet::new();
        for raw in parse_query(&ninja(&base, &["-t", "query", out])?) {
            set.insert(normalize(&base, &raw));
        }
        if let Some(list) = deps.get(out) {
            for raw in list {
                set.insert(normalize(&base, raw));
            }
        }
        declared.insert(out.clone(), set);
    }
    Ok(declared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deps_blocks_split_per_target() {
        let text = "a.o: #deps 2, deps mtime 1 (VALID)\n    /x/a.c\n    /usr/include/errno.h\n\nb.o: #deps 1, deps mtime 2 (VALID)\n    /x/b.c\n";
        let map = parse_deps(text);
        assert_eq!(map["a.o"].len(), 2);
        assert_eq!(map["b.o"], vec!["/x/b.c".to_string()]);
    }

    #[test]
    fn query_skips_order_only() {
        let text = "a.o:\n  input: C_COMPILER\n    /x/a.c\n    | /x/extra.h\n    || phony_target\n  outputs:\n    libuv.so\n";
        assert_eq!(parse_query(text), vec!["/x/a.c".to_string(), "/x/extra.h".to_string()]);
    }
}

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::declared;
use crate::event::{decode, Access, Event};
use crate::ninja;

pub fn owner_of(pid: i32, parent: &HashMap<i32, i32>, roots: &HashMap<i32, usize>) -> Option<usize> {
    let mut cur = pid;
    for _ in 0..64 {
        if let Some(&idx) = roots.get(&cur) {
            return Some(idx);
        }
        cur = *parent.get(&cur)?;
    }
    None
}

pub fn run(file: &Path, dir: &Path) -> Result<()> {
    let edges = ninja::edges(dir)?;
    let by_cmd: HashMap<&str, usize> = edges
        .iter()
        .enumerate()
        .map(|(idx, edge)| (edge.command.as_str(), idx))
        .collect();
    let outputs: Vec<String> = edges.iter().map(|edge| edge.output.clone()).collect();
    let declared = declared::load(dir, &outputs)?;

    let mut parent: HashMap<i32, i32> = HashMap::new();
    let mut roots: HashMap<i32, usize> = HashMap::new();
    let mut touched: Vec<i32> = Vec::new();
    let mut acts: Vec<(i32, Access, String)> = Vec::new();
    let mut stray: Vec<String> = Vec::new();
    let mut shells = 0;

    let src = File::open(file).with_context(|| format!("cannot open {}", file.display()))?;
    for line in BufReader::new(src).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match decode(&line)? {
            Event::Spawn { parent: par, child } => {
                parent.insert(child, par);
            }
            Event::Exec { pid, argv, .. } => {
                if argv.len() == 3 && argv[0] == "/bin/sh" && argv[1] == "-c" {
                    shells += 1;
                    match by_cmd.get(argv[2].as_str()) {
                        Some(&idx) => {
                            roots.insert(pid, idx);
                        }
                        None => stray.push(argv[2].clone()),
                    }
                }
            }
            Event::Access { pid, kind, path, ok, .. } => {
                touched.push(pid);
                if ok && kind != Access::Remove {
                    acts.push((pid, kind, path));
                }
            }
            Event::Exit { .. } => {}
        }
    }

    let hit = touched
        .iter()
        .filter(|&&pid| owner_of(pid, &parent, &roots).is_some())
        .count();
    println!("edges in ninja: {}", edges.len());
    println!("sh -c commands seen: {shells}");
    println!("matched to an edge: {}", roots.len());
    println!("accesses: {}, attributed: {hit}", touched.len());
    for cmd in stray.iter().take(3) {
        println!("unmatched: {}", cmd.chars().take(150).collect::<String>());
    }

    let mut undeclared: Vec<HashSet<String>> = vec![HashSet::new(); edges.len()];
    let mut wrote: Vec<HashSet<String>> = vec![HashSet::new(); edges.len()];
    let mut used: Vec<HashSet<String>> = vec![HashSet::new(); edges.len()];
    let mut probed: Vec<HashSet<String>> = vec![HashSet::new(); edges.len()];
    let mut writer: HashMap<String, usize> = HashMap::new();
    let mut seen = 0;
    for (pid, kind, path) in &acts {
        let Some(idx) = owner_of(*pid, &parent, &roots) else {
            continue;
        };
        if *kind == Access::Write {
            wrote[idx].insert(path.clone());
            writer.entry(path.clone()).or_insert(idx);
            continue;
        }
        if *kind == Access::Stat {
            probed[idx].insert(path.clone());
            continue;
        }
        seen += 1;
        used[idx].insert(path.clone());
        if wrote[idx].contains(path) {
            continue;
        }
        let known = declared
            .get(&edges[idx].output)
            .is_some_and(|set| set.contains(path));
        if !known {
            undeclared[idx].insert(path.clone());
        }
    }

    let mut generated: Vec<(usize, String, usize)> = Vec::new();
    for (idx, set) in undeclared.iter_mut().enumerate() {
        let mut moved: Vec<String> = set
            .iter()
            .filter(|path| writer.get(*path).is_some_and(|&other| other != idx))
            .cloned()
            .collect();
        moved.sort();
        for path in moved {
            set.remove(&path);
            let other = writer[&path];
            generated.push((idx, path, other));
        }
    }

    let base = format!("{}/", dir.to_string_lossy().trim_end_matches('/'));
    generated.retain(|(idx, path, _)| {
        let mut stack: Vec<&String> = declared
            .get(&edges[*idx].output)
            .map(|set| set.iter().collect())
            .unwrap_or_default();
        let mut visited: HashSet<&String> = HashSet::new();
        while let Some(cur) = stack.pop() {
            if cur == path {
                return false;
            }
            if !visited.insert(cur) {
                continue;
            }
            let key = cur.strip_prefix(&base).unwrap_or(cur.as_str());
            if let Some(next) = declared.get(key) {
                stack.extend(next.iter());
            }
        }
        true
    });

    let pairs: usize = undeclared.iter().map(|set| set.len()).sum();
    println!("attributed reads: {seen}, distinct undeclared (edge, path) pairs: {pairs}");

    let mut by_dir: HashMap<String, usize> = HashMap::new();
    for set in &undeclared {
        for path in set {
            let dir_name = Path::new(path)
                .parent()
                .map(|par| par.to_string_lossy().into_owned())
                .unwrap_or_default();
            *by_dir.entry(dir_name).or_default() += 1;
        }
    }
    let mut ranked: Vec<(String, usize)> = by_dir.into_iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    println!("top directories by undeclared reads:");
    for (dir_name, count) in ranked.iter().take(20) {
        println!("  {count:6}  {dir_name}");
    }

    println!("missing dependency edges: {}", generated.len());
    for (idx, path, other) in &generated {
        println!("  {}  <-  {path}  (written by {})", edges[*idx].output, edges[*other].output);
    }

    let matched: HashSet<usize> = roots.values().copied().collect();
    let mut idle: Vec<(usize, String)> = Vec::new();
    let mut only_probed = 0;
    for (idx, edge) in edges.iter().enumerate() {
        if !matched.contains(&idx) {
            continue;
        }
        let Some(set) = declared.get(&edge.output) else {
            continue;
        };
        let mut names: Vec<&String> = set.iter().collect();
        names.sort();
        for path in names {
            if used[idx].contains(path) {
                continue;
            }
            if probed[idx].contains(path) {
                only_probed += 1;
                continue;
            }
            if links_to(dir, &edge.output, path) {
                continue;
            }
            idle.push((idx, path.clone()));
        }
    }
    println!("declared inputs never touched: {}", idle.len());
    println!("declared inputs only probed: {only_probed}");
    for (idx, path) in idle.iter().take(40) {
        println!("  {}  ->  {path}", edges[*idx].output);
    }

    let skip = ["/bin/", "/usr/bin/", "/lib/", "/usr/lib/", "/usr/libexec/", "/usr/include/", "/dev/", "/etc/ld.so.cache"];
    let root = dir.to_string_lossy().trim_end_matches('/').to_string();
    let rel = |path: &str| -> String {
        path.strip_prefix(&format!("{root}/")).unwrap_or(path).to_string()
    };
    let mut fixes: Vec<(String, String)> = Vec::new();
    for (idx, _, other) in &generated {
        fixes.push((edges[*idx].output.clone(), edges[*other].output.clone()));
    }
    for (idx, set) in undeclared.iter().enumerate() {
        for path in set.iter().filter(|path| !skip.iter().any(|pre| path.starts_with(pre))) {
            fixes.push((edges[idx].output.clone(), rel(path)));
        }
    }
    fixes.sort();
    fixes.dedup();
    println!("suggested additions:");
    for (out, extra) in &fixes {
        println!("  {out}: add \"| {extra}\" to its build line");
    }

    println!("project level undeclared reads:");
    for (idx, set) in undeclared.iter().enumerate() {
        let mut list: Vec<&String> = set
            .iter()
            .filter(|path| !skip.iter().any(|pre| path.starts_with(pre)))
            .collect();
        list.sort();
        for path in list {
            println!("  {}  <-  {path}", edges[idx].output);
        }
    }
    Ok(())
}

fn links_to(dir: &Path, output: &str, input: &str) -> bool {
    let out = dir.join(output);
    let is_link = std::fs::symlink_metadata(&out).is_ok_and(|meta| meta.file_type().is_symlink());
    if !is_link {
        return false;
    }
    match (std::fs::canonicalize(&out), std::fs::canonicalize(dir.join(input))) {
        (Ok(real), Ok(want)) => real == want,
        _ => false,
    }
}

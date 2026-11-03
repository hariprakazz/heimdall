use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::event::{decode, Event};
use crate::ninja;

fn owner_of(pid: i32, parent: &HashMap<i32, i32>, roots: &HashMap<i32, usize>) -> Option<usize> {
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

    let mut parent: HashMap<i32, i32> = HashMap::new();
    let mut roots: HashMap<i32, usize> = HashMap::new();
    let mut touched: Vec<i32> = Vec::new();
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
            Event::Access { pid, .. } => touched.push(pid),
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
    Ok(())
}

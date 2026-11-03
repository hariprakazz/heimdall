use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::attribute::owner_of;
use crate::event::{decode, Access, Event};

type Touches = BTreeSet<(String, String)>;

fn kind_name(kind: Access) -> &'static str {
    match kind {
        Access::Read => "read",
        Access::Write => "write",
        Access::Remove => "remove",
        Access::Stat => "stat",
    }
}

fn short(cmd: &str) -> String {
    cmd.chars().take(100).collect()
}

fn load(file: &Path) -> Result<(HashMap<String, Touches>, usize)> {
    let mut parent: HashMap<i32, i32> = HashMap::new();
    let mut roots: HashMap<i32, usize> = HashMap::new();
    let mut cmds: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut acts: Vec<(i32, Access, String)> = Vec::new();

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
                    let idx = match index.get(&argv[2]) {
                        Some(&found) => found,
                        None => {
                            cmds.push(argv[2].clone());
                            index.insert(argv[2].clone(), cmds.len() - 1);
                            cmds.len() - 1
                        }
                    };
                    roots.insert(pid, idx);
                }
            }
            Event::Access { pid, kind, path, ok, .. } => {
                if ok && kind != Access::Stat {
                    acts.push((pid, kind, path));
                }
            }
            Event::Exit { .. } => {}
        }
    }

    let mut touched: Vec<Touches> = vec![BTreeSet::new(); cmds.len()];
    let mut wrote: Vec<HashSet<String>> = vec![HashSet::new(); cmds.len()];
    let mut removed: Vec<HashSet<String>> = vec![HashSet::new(); cmds.len()];
    for (pid, kind, path) in acts {
        let Some(idx) = owner_of(pid, &parent, &roots) else {
            continue;
        };
        match kind {
            Access::Write => {
                wrote[idx].insert(path.clone());
            }
            Access::Remove => {
                removed[idx].insert(path.clone());
            }
            _ => {}
        }
        touched[idx].insert((kind_name(kind).to_string(), path));
    }

    let mut scratch = 0;
    for idx in 0..cmds.len() {
        let gone: HashSet<&String> = wrote[idx].intersection(&removed[idx]).collect();
        scratch += gone.len();
        touched[idx].retain(|(_, path)| !gone.contains(path));
    }
    Ok((cmds.into_iter().zip(touched).collect(), scratch))
}

pub fn run(first: &Path, second: &Path) -> Result<()> {
    let (left, left_scratch) = load(first)?;
    let (right, right_scratch) = load(second)?;
    println!("commands: {} vs {}", left.len(), right.len());
    println!("scratch files ignored: {left_scratch} vs {right_scratch}");

    let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
    let mut differing = 0;
    for cmd in keys {
        match (left.get(cmd), right.get(cmd)) {
            (Some(one), Some(two)) => {
                let only_one: Vec<_> = one.difference(two).collect();
                let only_two: Vec<_> = two.difference(one).collect();
                if only_one.is_empty() && only_two.is_empty() {
                    continue;
                }
                differing += 1;
                println!("{}", short(cmd));
                for (kind, path) in only_one {
                    println!("  - {kind} {path}");
                }
                for (kind, path) in only_two {
                    println!("  + {kind} {path}");
                }
            }
            (Some(_), None) => {
                differing += 1;
                println!("only in first run: {}", short(cmd));
            }
            (None, Some(_)) => {
                differing += 1;
                println!("only in second run: {}", short(cmd));
            }
            (None, None) => {}
        }
    }
    println!("commands that differ: {differing}");
    Ok(())
}

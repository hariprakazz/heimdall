mod attribute;
mod declared;
mod diff;
mod event;
mod filter;
mod ninja;
mod report;
mod resolve;
mod syscalls;
mod trace;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "heimdall")]
struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    Run {
        #[arg(short, long)]
        out: Option<PathBuf>,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        cmd: Vec<String>,
    },
    Report {
        file: PathBuf,

        #[arg(short, long)]
        root: String,
    },
    Diff {
        first: PathBuf,

        second: PathBuf,
    },
    Actions {
        file: PathBuf,

        #[arg(short, long)]
        dir: PathBuf,
    },
}

fn run(out: Option<PathBuf>, cmd: Vec<String>) -> Result<()> {
    let mut sink: Box<dyn Write> = match &out {
        Some(path) => {
            let file = File::create(path)
                .with_context(|| format!("cannot create {}", path.display()))?;
            Box::new(BufWriter::new(file))
        }
        None => Box::new(std::io::stderr()),
    };
    let code = trace::run(&cmd, |ev| {
        if let Ok(line) = event::encode(&ev) {
            let _ = writeln!(sink, "{line}");
        }
    })?;
    sink.flush()?;
    std::process::exit(code);
}

fn main() -> Result<()> {
    match Cli::parse().action {
        Action::Run { out, cmd } => run(out, cmd),
        Action::Report { file, root } => report::run(&file, &root),
        Action::Actions { file, dir } => attribute::run(&file, &dir),
        Action::Diff { first, second } => diff::run(&first, &second),
    }
}

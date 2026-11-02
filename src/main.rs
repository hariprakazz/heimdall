mod event;
mod resolve;
mod syscalls;
mod trace;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser)]
#[command(name = "heimdall")]
struct Cli {
    #[arg(short, long)]
    out: Option<PathBuf>,

    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    cmd: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut sink: Box<dyn Write> = match &cli.out {
        Some(path) => {
            let file = File::create(path)
                .with_context(|| format!("cannot create {}", path.display()))?;
            Box::new(BufWriter::new(file))
        }
        None => Box::new(std::io::stderr()),
    };
    let code = trace::run(&cli.cmd, |ev| {
        if let Ok(line) = event::encode(&ev) {
            let _ = writeln!(sink, "{line}");
        }
    })?;
    sink.flush()?;
    std::process::exit(code);
}

mod event;
mod trace;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "heimdall")]
struct Cli {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    cmd: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let code = trace::run(&cli.cmd, |ev| {
        if let Ok(line) = event::encode(&ev) {
            eprintln!("{line}");
        }
    })?;
    std::process::exit(code);
}

use std::io::{self, Read, Write};

use anyhow::Result;
use clap::Parser;

use org2jsonl::org_to_json::org_to_entries;

#[derive(Parser)]
#[command(
    name = "org2jsonl",
    version,
    about = "Convert Emacs Org-mode files to JSONL"
)]
struct Cli {
    /// Input file (reads from stdin if not specified)
    #[arg(value_name = "FILE")]
    input: Option<String>,

    /// Pretty-print JSON output (one element per line, indented)
    #[arg(short, long)]
    pretty: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let input = match &cli.input {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let entries = org_to_entries(&input);
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for entry in &entries {
        if cli.pretty {
            serde_json::to_writer_pretty(&mut out, entry)?;
        } else {
            serde_json::to_writer(&mut out, entry)?;
        }
        writeln!(out)?;
    }

    Ok(())
}

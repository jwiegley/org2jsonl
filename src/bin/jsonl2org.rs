use std::io::{self, Read, Write};

use anyhow::{Context, Result};
use clap::Parser;

use org2jsonl::json_to_org::entries_to_org;
use org2jsonl::model::OrgEntry;

#[derive(Parser)]
#[command(
    name = "jsonl2org",
    version,
    about = "Convert JSONL back to Emacs Org-mode format"
)]
struct Cli {
    /// Input JSONL file (reads from stdin if not specified)
    #[arg(value_name = "FILE")]
    input: Option<String>,

    /// Output file (writes to stdout if not specified)
    #[arg(short, long, value_name = "FILE")]
    output: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let entries: Vec<OrgEntry> = match &cli.input {
        Some(path) => {
            let content = std::fs::read_to_string(path)?;
            parse_jsonl(&content)?
        }
        None => {
            // Try to detect if input is a single JSON array or JSONL
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            parse_jsonl(&buf)?
        }
    };

    let org_text = entries_to_org(&entries);

    match &cli.output {
        Some(path) => {
            std::fs::write(path, &org_text)?;
        }
        None => {
            io::stdout().write_all(org_text.as_bytes())?;
        }
    }

    Ok(())
}

fn parse_jsonl(input: &str) -> Result<Vec<OrgEntry>> {
    let mut entries = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: OrgEntry = serde_json::from_str(line)
            .with_context(|| format!("Failed to parse JSONL line {}", i + 1))?;
        entries.push(entry);
    }
    Ok(entries)
}

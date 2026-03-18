use std::io::{self, Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use walkdir::WalkDir;

use org2jsonl::json_to_org::{entries_to_org, inject_file_properties};
use org2jsonl::model::OrgEntry;

#[derive(Parser)]
#[command(
    name = "jsonl2org",
    version,
    about = "Convert JSONL back to Emacs Org-mode format"
)]
struct Cli {
    /// Input JSONL files or directories (reads from stdin if none specified)
    #[arg(value_name = "PATH")]
    inputs: Vec<String>,

    /// Output file (writes to stdout if not specified)
    #[arg(short, long, value_name = "FILE")]
    output: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let entries: Vec<OrgEntry> = if cli.inputs.is_empty() {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        parse_jsonl(&buf)?
    } else {
        let files = collect_jsonl_files(&cli.inputs)?;
        let mut all_entries = Vec::new();
        for file_path in &files {
            let content = std::fs::read_to_string(file_path)?;
            all_entries.extend(parse_jsonl(&content)?);
        }
        all_entries
    };

    let entries = inject_file_properties(entries);
    let org_text = entries_to_org(&entries);

    match &cli.output {
        Some(path) => std::fs::write(path, &org_text)?,
        None => io::stdout().write_all(org_text.as_bytes())?,
    }

    Ok(())
}

fn collect_jsonl_files(paths: &[String]) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for path_str in paths {
        let path = Path::new(path_str);
        if path.is_dir() {
            for entry in WalkDir::new(path) {
                let entry = entry?;
                if entry.path().extension().is_some_and(|ext| ext == "jsonl") {
                    files.push(entry.path().to_path_buf());
                }
            }
        } else {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
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

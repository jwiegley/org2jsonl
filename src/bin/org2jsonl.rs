use std::io::{self, Read, Write};
use std::path::Path;

use anyhow::Result;
use clap::Parser;
use walkdir::WalkDir;

use org2jsonl::model::OrgEntry;
use org2jsonl::org_to_json::{org_to_entries, org_to_entries_with_source};

#[derive(Parser)]
#[command(
    name = "org2jsonl",
    version,
    about = "Convert Emacs Org-mode files to JSONL"
)]
struct Cli {
    /// Input files or directories (reads from stdin if none specified)
    #[arg(value_name = "PATH")]
    inputs: Vec<String>,

    /// Pretty-print JSON output (one element per line, indented)
    #[arg(short, long)]
    pretty: bool,
}

fn collect_org_files(paths: &[String]) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for path_str in paths {
        let path = Path::new(path_str);
        if path.is_dir() {
            for entry in WalkDir::new(path) {
                let entry = entry?;
                if entry.path().extension().is_some_and(|ext| ext == "org") {
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

fn write_entries(out: &mut impl Write, entries: &[OrgEntry], pretty: bool) -> Result<()> {
    for entry in entries {
        if pretty {
            serde_json::to_writer_pretty(&mut *out, entry)?;
        } else {
            serde_json::to_writer(&mut *out, entry)?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if cli.inputs.is_empty() {
        // Read from stdin (no location metadata)
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        let entries = org_to_entries(&buf);
        write_entries(&mut out, &entries, cli.pretty)?;
    } else {
        let files = collect_org_files(&cli.inputs)?;
        for file_path in &files {
            let input = std::fs::read_to_string(file_path)?;
            let file_str = file_path.to_string_lossy();
            let entries = org_to_entries_with_source(&input, Some(&file_str));
            write_entries(&mut out, &entries, cli.pretty)?;
        }
    }
    Ok(())
}

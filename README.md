# org2jsonl

Convert Emacs Org-mode files to and from JSONL (JSON Lines) for manipulation
with standard JSON tools.

## Overview

org2jsonl provides two command-line tools and a Rust library for lossless
conversion between Org-mode and JSONL:

- **`org2jsonl`** -- parse an Org-mode file into one JSON object per
  top-level heading
- **`jsonl2org`** -- reconstruct canonical Org-mode text from JSONL

The conversion forms an *adjunction*: a first round-trip through JSONL may
normalize non-standard formatting (trailing whitespace, inconsistent
indentation), but every subsequent round-trip is byte-identical. Well-formatted
Org-mode files round-trip with zero changes.

## Installation

### With Nix (recommended)

```sh
nix build    # produces result/bin/org2jsonl and result/bin/jsonl2org
nix develop  # enters a dev shell with cargo, clippy, rust-analyzer, etc.
```

### With Cargo

```sh
cargo install --path .
```

This installs both `org2jsonl` and `jsonl2org` into your Cargo bin directory.

## Command-Line Usage

### org2jsonl

Convert an Org-mode file to JSONL (one JSON object per line):

```sh
org2jsonl notes.org > notes.jsonl
cat notes.org | org2jsonl > notes.jsonl   # or read from stdin
org2jsonl --pretty notes.org              # indented JSON for readability
```

### jsonl2org

Convert JSONL back to Org-mode text:

```sh
jsonl2org notes.jsonl > notes.org
cat notes.jsonl | jsonl2org > notes.org       # or read from stdin
jsonl2org notes.jsonl --output restored.org   # write to a file
```

### Piping together

```sh
# Round-trip: should produce no diff on well-formatted files
diff notes.org <(org2jsonl notes.org | jsonl2org)

# Extract all TODO headings using jq
org2jsonl notes.org | jq 'select(.content.heading.keyword == "TODO")'

# Change all TODO keywords to DONE
org2jsonl notes.org \
  | jq 'if .content.heading.keyword == "TODO" then .content.heading.keyword = "DONE" else . end' \
  | jsonl2org > done.org

# Count headings by level
org2jsonl notes.org | jq -r '.content.heading.level // empty' | sort | uniq -c

# Extract all source blocks
org2jsonl notes.org \
  | jq -r '.. | select(.type? == "src_block") | .value'
```

## JSONL Format

Each line of the JSONL output is a self-contained JSON object representing one
top-level entry. There are two entry types:

### Section (content before the first heading)

```json
{"schema_version":1,"type":"section","elements":[...]}
```

### Heading (a top-level heading with all nested content)

```json
{"schema_version":1,"type":"heading","level":1,"title":[{"type":"text","value":"My Heading"}],"keyword":"TODO","tags":["work"],"body":[...],"children":[...]}
```

Key fields on headings:

| Field | Type | Description |
|-------|------|-------------|
| `level` | number | Heading depth (1 = `*`, 2 = `**`, etc.) |
| `keyword` | string? | TODO keyword (`"TODO"`, `"DONE"`, etc.) |
| `priority` | string? | Priority character (`"A"`, `"B"`, `"C"`) |
| `title` | inline[] | Title as inline content (may contain markup) |
| `tags` | string[] | Tags on the heading line |
| `planning` | object? | `scheduled`, `deadline`, `closed` timestamps |
| `properties` | property[] | Property drawer key-value pairs |
| `body` | element[] | Block-level body elements |
| `children` | heading[] | Nested child headings |

The full schema supports all Org-mode element and object types including
paragraphs, lists, tables, source blocks, drawers, timestamps, inline markup
(bold, italic, code, links, etc.), footnotes, LaTeX fragments, and more. See
`src/model.rs` for the complete type definitions.

## Library Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
org2jsonl = { path = "." }  # or publish to crates.io
serde_json = "1"
```

### Parse Org-mode to structured entries

```rust
use org2jsonl::org_to_json::org_to_entries;

let input = "* TODO Buy groceries\nSCHEDULED: <2025-01-15>\n- [ ] Milk\n- [X] Eggs\n";
let entries = org_to_entries(input);

// Each entry is an OrgEntry with schema_version and content
for entry in &entries {
    println!("{}", serde_json::to_string_pretty(entry).unwrap());
}
```

### Convert entries back to Org-mode text

```rust
use org2jsonl::json_to_org::entries_to_org;

let org_text = entries_to_org(&entries);
// org_text is canonical Org-mode text with consistent formatting
```

### Full round-trip through JSON

```rust
use org2jsonl::org_to_json::org_to_entries;
use org2jsonl::json_to_org::entries_to_org;
use org2jsonl::model::OrgEntry;

let input = std::fs::read_to_string("notes.org").unwrap();
let entries = org_to_entries(&input);

// Serialize to JSONL
let jsonl: String = entries.iter()
    .map(|e| serde_json::to_string(e).unwrap())
    .collect::<Vec<_>>()
    .join("\n");

// Deserialize back
let recovered: Vec<OrgEntry> = jsonl.lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect();

// Reconstruct Org-mode text
let output = entries_to_org(&recovered);
```

### Key modules

| Module | Description |
|--------|-------------|
| `org2jsonl::org_to_json` | Parse Org-mode text into `Vec<OrgEntry>` |
| `org2jsonl::json_to_org` | Render `&[OrgEntry]` back into Org-mode text |
| `org2jsonl::model` | All data types (`OrgEntry`, `Heading`, `Element`, `InlineContent`, etc.) |

## Canonical Form

The `jsonl2org` output follows a canonical form:

- No trailing whitespace on any line
- Property drawers immediately after the heading line (with planning in between
  when present)
- UTF-8 encoding, LF line endings
- File ends with exactly one newline
- Blank lines between entries controlled by the `post_blank` field

## Development

```sh
nix develop              # enter the dev shell
cargo test               # run all tests (unit + integration + property-based)
cargo clippy -- -D warnings
cargo doc --no-deps      # generate API documentation
```

### Project structure

```
src/
  lib.rs          -- crate root, re-exports modules
  model.rs        -- data types (OrgEntry, Heading, Element, InlineContent, ...)
  org_to_json.rs  -- Org-mode parser (orgize CST -> model)
  json_to_org.rs  -- canonical writer (model -> Org-mode text)
  bin/
    org2jsonl.rs  -- CLI: Org -> JSONL
    jsonl2org.rs  -- CLI: JSONL -> Org
tests/
  integration_tests.rs  -- round-trip, idempotency, and fixture tests
```

## License

MIT

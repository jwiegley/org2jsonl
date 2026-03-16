# org2jsonl

I've often wanted to manipulate my Org-mode files with standard JSON tools --
jq, custom scripts, that sort of thing. org2jsonl is what I came up with: a
pair of Rust command-line tools and a library for lossless conversion between
Org-mode and JSONL (JSON Lines).

The conversion is designed to be lossless. It forms what I'd call an
*adjunction*: the first round-trip through JSONL may normalize things like
trailing whitespace or inconsistent indentation, but every round-trip after
that is byte-identical. If your Org files are already well-formatted, the
first round-trip won't change them either.

## Installation

### With Nix

```sh
nix build    # produces result/bin/org2jsonl and result/bin/jsonl2org
nix develop  # enters a dev shell with cargo, clippy, rust-analyzer, etc.
```

### With Cargo

```sh
cargo install --path .
```

This installs both `org2jsonl` and `jsonl2org` into your Cargo bin directory.

## Usage

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

### Piping the two together

Here's where it gets interesting. Since the intermediate format is just JSON,
you can use jq or any other JSON tool to transform your Org files:

```sh
# Round-trip: should produce no diff on well-formatted files
diff notes.org <(org2jsonl notes.org | jsonl2org)

# Extract all TODO headings
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

Each line of the JSONL output is a self-contained JSON object. There are two
types of entry:

**Section** -- content that appears before the first heading:

```json
{"schema_version":1,"type":"section","elements":[...]}
```

**Heading** -- a top-level heading with all its nested content:

```json
{"schema_version":1,"type":"heading","level":1,"title":[{"type":"text","value":"My Heading"}],...}
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

The full schema covers all Org-mode element and object types -- paragraphs,
lists, tables, source blocks, drawers, timestamps, inline markup, footnotes,
LaTeX fragments, and more. See `src/model.rs` for the complete type
definitions.

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

for entry in &entries {
    println!("{}", serde_json::to_string_pretty(entry).unwrap());
}
```

### Convert entries back to Org-mode text

```rust
use org2jsonl::json_to_org::entries_to_org;

let org_text = entries_to_org(&entries);
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
| `org2jsonl::json_to_org` | Render `&[OrgEntry]` back to Org-mode text |
| `org2jsonl::model` | All data types (`OrgEntry`, `Heading`, `Element`, etc.) |

## Canonical Form

The `jsonl2org` output follows a canonical form:

- No trailing whitespace on any line
- Property drawers immediately after the heading line (with planning in
  between when present)
- UTF-8, LF line endings
- File ends with exactly one newline
- Blank lines between entries controlled by the `post_blank` field

## Development

```sh
nix develop              # enter the dev shell
cargo test               # run all tests (unit + integration + property-based)
cargo clippy -- -D warnings
cargo bench              # run benchmarks
cargo doc --no-deps      # generate API docs
nix flake check          # run all checks (build, test, clippy, fmt, doc)
```

### Pre-commit hooks

This project uses [lefthook](https://github.com/evilmartians/lefthook) for
pre-commit checks. Install the hooks with:

```sh
lefthook install
```

The hooks run formatting, linting, tests, coverage, documentation, benchmark
regression, and Nix build checks -- all in parallel.

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
  integration_tests.rs  -- round-trip, idempotency, fixture, and property-based tests
benches/
  bench_roundtrip.rs    -- criterion benchmarks for parse/write/round-trip
fuzz/
  fuzz_targets/         -- cargo-fuzz targets for parser fuzzing
```

## License

BSD-3-Clause -- see [LICENSE.md](LICENSE.md) for details.

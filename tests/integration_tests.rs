//! Comprehensive integration tests for org2jsonl.
//!
//! These tests exercise the full pipeline:
//!   Org text -> org_to_entries -> JSON serialization -> JSON deserialization -> entries_to_org
//!
//! The key property (adjunction) is:
//!   - First round-trip may normalize formatting
//!   - Second round-trip MUST be identical to the first (idempotency)

use org2jsonl::json_to_org::entries_to_org;
use org2jsonl::model::{EntryContent, Heading, InlineContent, OrgEntry};
use org2jsonl::org_to_json::org_to_entries;
use org2jsonl::SCHEMA_VERSION;

// Import pretty_assertions::assert_eq only where used explicitly.
// Inside macro-generated modules we use the standard assert_eq to avoid
// ambiguity between the glob-imported pretty_assertions version and the
// prelude version.

// =========================================================================
// Helpers
// =========================================================================

/// Perform a single round-trip: Org -> entries -> JSON lines -> entries -> Org.
fn round_trip(input: &str) -> String {
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    let recovered = jsonl_to_entries(&jsonl);
    entries_to_org(&recovered)
}

/// Serialize entries to JSONL (one JSON object per line).
fn entries_to_jsonl(entries: &[OrgEntry]) -> String {
    let mut lines = Vec::new();
    for entry in entries {
        lines.push(serde_json::to_string(entry).expect("serialization should not fail"));
    }
    lines.join("\n")
}

/// Deserialize JSONL back to entries.
fn jsonl_to_entries(jsonl: &str) -> Vec<OrgEntry> {
    jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<OrgEntry>(line).expect("deserialization should not fail")
        })
        .collect()
}

/// Assert the adjunction/idempotency property:
/// The second round-trip must produce output identical to the first.
fn assert_idempotent(input: &str) {
    let first = round_trip(input);
    let second = round_trip(&first);
    pretty_assertions::assert_eq!(
        first,
        second,
        "Idempotency violation: second round-trip differs from first"
    );
}

/// Assert that the full pipeline (including JSON ser/de) produces valid
/// output and that the entries survive the JSON round-trip intact.
fn assert_json_round_trip(input: &str) {
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    let recovered = jsonl_to_entries(&jsonl);
    pretty_assertions::assert_eq!(
        entries,
        recovered,
        "Entries differ after JSON serialization round-trip"
    );
}

// =========================================================================
// 1. Fixture file round-trip tests
// =========================================================================

macro_rules! fixture_test {
    ($name:ident, $file:expr) => {
        mod $name {
            use super::{assert_idempotent, assert_json_round_trip, entries_to_jsonl, round_trip};
            use org2jsonl::model::OrgEntry;
            use org2jsonl::org_to_json::org_to_entries;
            use org2jsonl::SCHEMA_VERSION;

            const FIXTURE: &str = include_str!(concat!("fixtures/", $file));

            #[test]
            fn round_trip_produces_valid_output() {
                let output = round_trip(FIXTURE);
                // Output must end with at least one newline
                assert!(output.ends_with('\n'), "output should end with newline");
            }

            #[test]
            fn json_round_trip_preserves_entries() {
                assert_json_round_trip(FIXTURE);
            }

            #[test]
            fn idempotency() {
                assert_idempotent(FIXTURE);
            }

            #[test]
            fn schema_version_present_in_all_entries() {
                let entries = org_to_entries(FIXTURE);
                for (i, entry) in entries.iter().enumerate() {
                    assert_eq!(
                        entry.schema_version, SCHEMA_VERSION,
                        "Entry {i} has wrong schema_version"
                    );
                }
            }

            #[test]
            fn jsonl_format_valid() {
                let entries = org_to_entries(FIXTURE);
                let jsonl = entries_to_jsonl(&entries);
                for (i, line) in jsonl.lines().enumerate() {
                    // Each line must be valid JSON
                    let parsed: serde_json::Value =
                        serde_json::from_str(line).unwrap_or_else(|e| {
                            panic!("Line {i} is not valid JSON: {e}\nLine: {line}")
                        });
                    // Must have schema_version field
                    assert!(
                        parsed.get("schema_version").is_some(),
                        "Line {i} missing schema_version field"
                    );
                    // Must have type field
                    assert!(parsed.get("type").is_some(), "Line {i} missing type field");
                    // Must deserialize back to OrgEntry
                    let _entry: OrgEntry = serde_json::from_str(line)
                        .unwrap_or_else(|e| panic!("Line {i} cannot deserialize to OrgEntry: {e}"));
                }
            }
        }
    };
}

fixture_test!(minimal, "minimal.org");
fixture_test!(simple, "simple.org");
fixture_test!(deep_nesting, "deep_nesting.org");
fixture_test!(complex_lists, "complex_lists.org");
fixture_test!(tables, "tables.org");
fixture_test!(links, "links.org");
fixture_test!(edge_cases, "edge_cases.org");
fixture_test!(no_headings, "no_headings.org");
fixture_test!(timestamps, "timestamps.org");
fixture_test!(full_document, "full_document.org");
fixture_test!(inline_objects, "inline_objects.org");

// =========================================================================
// 2. Hand-crafted round-trip and idempotency tests
// =========================================================================

// -------------------------------------------------------------------------
// Headings with all features
// -------------------------------------------------------------------------

#[test]
fn heading_todo_priority_tags_planning_properties() {
    let input = "\
* TODO [#A] Important task :work:urgent:
SCHEDULED: <2024-01-15 Mon> DEADLINE: <2024-01-20 Sat>
:PROPERTIES:
:ID: task-001
:EFFORT: 2:00
:CATEGORY: project
:END:

This is the body text.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn heading_done_with_closed() {
    let input = "\
* DONE [#B] Completed task :done:
CLOSED: [2024-01-18 Thu 14:30] SCHEDULED: <2024-01-15 Mon>
:PROPERTIES:
:ID: done-001
:END:

Completed description.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn multiple_top_level_headings() {
    let input = "\
* First heading

Body of first.

* Second heading

Body of second.

* Third heading

Body of third.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Deeply nested headings (5+ levels)
// -------------------------------------------------------------------------

#[test]
fn deeply_nested_headings_six_levels() {
    let input = "\
* Level 1
** Level 2
*** Level 3
**** Level 4
***** Level 5
****** Level 6

Content at level 6.

***** Back to 5

Content at 5.

**** Back to 4

Content at 4.

** Another level 2

Content at 2.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn deeply_nested_with_bodies_at_every_level() {
    let input = "\
* Level 1 body

Text at level 1.

** Level 2 body

Text at level 2.

*** Level 3 body

Text at level 3.

**** Level 4 body

Text at level 4.

***** Level 5 body

Text at level 5.
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Block types
// -------------------------------------------------------------------------

#[test]
fn src_block_with_language_and_params() {
    let input = "\
* Code blocks

#+begin_src rust :tangle yes
fn main() {
    println!(\"Hello, world!\");
}
#+end_src
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn src_block_no_language() {
    let input = "\
* No language

#+begin_src
some code
#+end_src
";
    assert_idempotent(input);
}

#[test]
fn example_block() {
    let input = "\
* Example

#+begin_example
This is example text.
  It preserves indentation.
#+end_example
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn quote_block() {
    let input = "\
* Quote

#+begin_quote
A wise person once said something.
#+end_quote
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn center_block() {
    let input = "\
* Center

#+begin_center
Centered text here.
#+end_center
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn verse_block() {
    let input = "\
* Verse

#+begin_verse
Roses are red,
Violets are blue,
Org-mode is great,
And so are you.
#+end_verse
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn comment_block() {
    let input = "\
* Comment block

#+begin_comment
This is hidden content.
Multiple lines.
#+end_comment
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn export_block_html() {
    let input = "\
* Export

#+begin_export html
<div class=\"custom\">HTML content</div>
#+end_export
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn special_block() {
    let input = "\
* Special

#+begin_warning
Be careful with this operation!
#+end_warning
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn dynamic_block() {
    let input = "\
* Dynamic

#+BEGIN: clocktable :maxlevel 2
content inside
#+END:
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn multiple_block_types_together() {
    let input = "\
* Mixed blocks

#+begin_src python
print(\"hello\")
#+end_src

#+begin_quote
A quote here.
#+end_quote

#+begin_example
An example here.
#+end_example

#+begin_center
Centered.
#+end_center
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Inline markup
// -------------------------------------------------------------------------

#[test]
fn all_inline_markup() {
    let input = "\
* Inline markup

A paragraph with *bold*, /italic/, _underline_, +strikethrough+, ~code~, and =verbatim= text.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn nested_inline_markup() {
    let input = "\
* Nested markup

This has *bold with /italic inside/* text.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Links
// -------------------------------------------------------------------------

#[test]
fn link_with_description() {
    let input = "\
* Links

A link to [[https://example.com][Example Site]].
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn link_without_description() {
    let input = "\
* Links

A bare link: [[https://example.com]].
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn file_link() {
    let input = "\
* File links

Link to a file: [[file:./other.org][Other file]].
";
    assert_idempotent(input);
}

#[test]
fn internal_link() {
    let input = "\
* Target heading

** Internal links

Link to [[*Target heading][target]].
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Tables
// -------------------------------------------------------------------------

#[test]
fn table_with_header_rule() {
    let input = "\
* Table

| Name  | Age |
|-------+-----|
| Alice |  30 |
| Bob   |  25 |
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn table_without_rule() {
    let input = "\
* Table

| a | b | c |
| d | e | f |
| g | h | i |
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn table_with_multiple_rules() {
    let input = "\
* Table

| Item  | Price | Total |
|-------+-------+-------|
| Apple |  1.50 |  4.50 |
| Bread |  2.00 |  2.00 |
|-------+-------+-------|
| Total |       |  6.50 |
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Lists
// -------------------------------------------------------------------------

#[test]
fn unordered_list() {
    let input = "\
* Unordered

- Item one
- Item two
- Item three
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn ordered_list() {
    let input = "\
* Ordered

1. First
2. Second
3. Third
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn descriptive_list() {
    let input = "\
* Descriptive

- Emacs :: A text editor
- Vim :: Another text editor
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn list_with_checkboxes() {
    let input = "\
* Checkboxes

- [X] Done item
- [ ] Undone item
- [-] Partial item
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn nested_lists() {
    let input = "\
* Nested

- Top level item 1
  - Nested item 1a
  - Nested item 1b
    - Deep nested
- Top level item 2
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn list_with_continuation_paragraphs() {
    let input = "\
* Continuation

- First item

  Continuation paragraph.

- Second item

  Another continuation.
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Timestamps
// -------------------------------------------------------------------------

#[test]
fn active_and_inactive_timestamps() {
    let input = "\
* Timestamps

Active: <2024-03-15 Fri 10:00>

Inactive: [2024-03-15 Fri 10:00]
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn timestamp_with_repeater() {
    let input = "\
* TODO Repeating task
SCHEDULED: <2024-03-15 Fri +1w>
";
    assert_idempotent(input);
}

#[test]
fn timestamp_range() {
    let input = "\
* Range

Date range: <2024-03-15 Fri>--<2024-03-20 Wed>
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Footnotes
// -------------------------------------------------------------------------

#[test]
fn footnote_reference_and_definition() {
    let input = "\
* Footnotes

This has a footnote[fn:1].

[fn:1] This is the footnote definition.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn inline_footnote() {
    let input = "\
* Inline footnote

This has an inline footnote[fn:: inline definition here].
";
    assert_idempotent(input);
}

// -------------------------------------------------------------------------
// Drawers
// -------------------------------------------------------------------------

#[test]
fn drawer() {
    let input = "\
* Heading with drawer

:LOGBOOK:
- Note taken on [2024-01-15 Mon 10:00]
:END:

Body text.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Property drawers
// -------------------------------------------------------------------------

#[test]
fn property_drawer_multiple_properties() {
    let input = "\
* Heading
:PROPERTIES:
:ID: abc-123
:CUSTOM_ID: my-heading
:CATEGORY: test
:EFFORT: 1:30
:END:

Body.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Keywords
// -------------------------------------------------------------------------

#[test]
fn keywords_title_author() {
    let input = "\
#+TITLE: My Document
#+AUTHOR: John Doe
#+DATE: 2024-01-15
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Comments and fixed-width areas
// -------------------------------------------------------------------------

#[test]
fn comment_lines() {
    let input = "\
* Comments

# This is a comment line.
# Another comment line.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn fixed_width() {
    let input = "\
* Fixed width

: This is fixed width text.
: It preserves formatting.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// LaTeX fragments and environments
// -------------------------------------------------------------------------

#[test]
fn latex_fragment_inline() {
    let input = "\
* LaTeX

The equation $E = mc^2$ is famous.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn latex_environment() {
    let input = "\
* LaTeX environment

\\begin{equation}
F = ma
\\end{equation}
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Horizontal rules
// -------------------------------------------------------------------------

#[test]
fn horizontal_rule() {
    let input = "\
* Rule

-----

Text after the rule.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Clock entries
// -------------------------------------------------------------------------

#[test]
fn clock_entry() {
    let input = "\
* Task with clock
CLOCK: [2024-01-15 Mon 10:00]--[2024-01-15 Mon 11:30] =>  1:30
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Zeroth section (preamble)
// -------------------------------------------------------------------------

#[test]
fn preamble_and_headings() {
    let input = "\
#+TITLE: Test Document

Some preamble text.

* First heading

Body.

* Second heading

Body.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Empty and edge cases
// -------------------------------------------------------------------------

#[test]
fn empty_input_produces_empty_entries() {
    let entries = org_to_entries("");
    assert!(entries.is_empty(), "empty input should produce no entries");
}

#[test]
fn whitespace_only_input() {
    let entries = org_to_entries("   \n\n   \n");
    // May or may not produce entries, but should not panic.
    let jsonl = entries_to_jsonl(&entries);
    let recovered = jsonl_to_entries(&jsonl);
    assert_eq!(entries, recovered);
}

#[test]
fn heading_with_no_body() {
    let input = "* Empty heading\n";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn heading_with_only_children() {
    let input = "\
* Parent
** Child one
** Child two
";
    assert_idempotent(input);
}

#[test]
fn consecutive_headings_at_same_level() {
    let input = "\
* First
* Second
* Third
* Fourth
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// -------------------------------------------------------------------------
// Complex integration scenarios
// -------------------------------------------------------------------------

#[test]
fn full_document_with_everything() {
    let input = "\
#+TITLE: Full Test
#+AUTHOR: Test Author

Preamble paragraph.

* TODO [#A] First heading :project:urgent:
SCHEDULED: <2024-01-15 Mon> DEADLINE: <2024-01-20 Sat>
:PROPERTIES:
:ID:       entry-001
:EFFORT:   3:00
:END:

A paragraph with *bold*, /italic/, ~code~, =verbatim= text.

- [X] Done item
- [ ] Pending item
  - Nested under pending

#+begin_src python
def hello():
    print(\"world\")
#+end_src

| Col1 | Col2 |
|------+------|
| a    | b    |

** DONE [#B] Child heading :done:
CLOSED: [2024-01-18 Thu]

Child body.

*** Deep child

Deep content.

* Second heading

#+begin_quote
A quote.
#+end_quote

-----

Final paragraph.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn document_with_all_block_types() {
    let input = "\
* Blocks galore

#+begin_src emacs-lisp
(message \"hello\")
#+end_src

#+begin_example
Example text.
#+end_example

#+begin_quote
Quoted text.
#+end_quote

#+begin_center
Centered text.
#+end_center

#+begin_verse
Verse text.
More verse.
#+end_verse

#+begin_comment
Hidden comment.
#+end_comment

#+begin_export html
<p>HTML</p>
#+end_export

#+begin_warning
Warning content.
#+end_warning
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn document_with_all_list_types() {
    let input = "\
* List types

- Unordered one
- Unordered two

1. Ordered one
2. Ordered two

- Term :: Definition
- Another :: Another def

- [X] Checked
- [ ] Unchecked
- [-] Partial
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

// =========================================================================
// 3. JSONL serialization format tests
// =========================================================================

#[test]
fn schema_version_matches_constant() {
    let entries = org_to_entries("* Test heading\nBody.\n");
    for entry in &entries {
        assert_eq!(entry.schema_version, SCHEMA_VERSION);
    }
}

#[test]
fn jsonl_each_line_is_valid_json() {
    let input = "\
#+TITLE: Test

* Heading One

Body.

* Heading Two

Body.
";
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    for (i, line) in jsonl.lines().enumerate() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
        assert!(
            parsed.is_ok(),
            "Line {i} is not valid JSON: {}",
            parsed.unwrap_err()
        );
    }
}

#[test]
fn jsonl_schema_version_on_every_line() {
    let input = "\
#+TITLE: Test

* Heading One

* Heading Two
";
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    for (i, line) in jsonl.lines().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        let sv = parsed.get("schema_version");
        assert!(sv.is_some(), "Line {i} is missing schema_version");
        assert_eq!(
            sv.unwrap().as_u64().unwrap(),
            SCHEMA_VERSION as u64,
            "Line {i} has wrong schema_version"
        );
    }
}

#[test]
fn jsonl_type_field_is_heading_or_section() {
    let input = "\
Some preamble.

* A heading
";
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    let lines: Vec<&str> = jsonl.lines().collect();
    assert!(lines.len() >= 2, "Expected at least 2 JSONL lines");

    let section: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(section["type"], "section");

    let heading: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(heading["type"], "heading");
}

#[test]
fn jsonl_heading_has_expected_fields() {
    let input = "\
* TODO [#A] My heading :tag1:tag2:
SCHEDULED: <2024-01-15 Mon>
:PROPERTIES:
:ID: test
:END:

Body paragraph.
";
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    let parsed: serde_json::Value = serde_json::from_str(jsonl.lines().next().unwrap()).unwrap();

    assert_eq!(parsed["type"], "heading");
    assert_eq!(parsed["level"], 1);
    assert_eq!(parsed["keyword"], "TODO");
    assert_eq!(parsed["priority"], "A");
    assert!(parsed["title"].is_array());
    assert!(parsed["tags"].is_array());
    assert_eq!(parsed["tags"][0], "tag1");
    assert_eq!(parsed["tags"][1], "tag2");
    assert!(parsed["planning"].is_object());
    assert!(parsed["properties"].is_array());
    assert!(parsed["body"].is_array());
}

#[test]
fn jsonl_section_has_elements() {
    let input = "Some text before headings.\n";
    let entries = org_to_entries(input);
    let jsonl = entries_to_jsonl(&entries);
    let parsed: serde_json::Value = serde_json::from_str(jsonl.lines().next().unwrap()).unwrap();

    assert_eq!(parsed["type"], "section");
    assert!(parsed["elements"].is_array());
}

#[test]
fn deserialized_entries_equal_original() {
    let input = "\
#+TITLE: Round-trip

* TODO [#A] Task :tag:
SCHEDULED: <2024-01-15 Mon>
:PROPERTIES:
:ID: rt-001
:END:

Body with *bold* and ~code~.

- List item

| a | b |
|---+---|
| c | d |
";
    let original = org_to_entries(input);
    let jsonl = entries_to_jsonl(&original);
    let recovered = jsonl_to_entries(&jsonl);
    assert_eq!(original, recovered);
}

// =========================================================================
// 4. Idempotency stress tests
// =========================================================================

#[test]
fn triple_round_trip_same_as_double() {
    let input = "\
#+TITLE: Test

* TODO [#A] Heading :tag:
SCHEDULED: <2024-01-15 Mon>
:PROPERTIES:
:ID: triple-001
:END:

Body with *bold*, /italic/, ~code~.

- [X] Done
- [ ] Pending
  - Sub item

** Child heading

Child body.

| a | b |
|---+---|
| c | d |
";
    let first = round_trip(input);
    let second = round_trip(&first);
    let third = round_trip(&second);
    assert_eq!(first, second, "first != second");
    assert_eq!(second, third, "second != third");
}

#[test]
fn idempotency_with_complex_lists() {
    let input = "\
* Complex lists

- Top level item 1
  - Nested item 1a
  - Nested item 1b
    - Deep nested 1b-i
- Top level item 2

1. Ordered first
2. Ordered second
   - Unordered nested

- Emacs :: An extensible text editor
- Vim :: A modal text editor

- [ ] Project Alpha
  - [X] Design
  - [ ] Testing
";
    assert_idempotent(input);
}

#[test]
fn idempotency_with_tables() {
    let input = "\
* Tables

| Name  | Age |
|-------+-----|
| Alice |  30 |
| Bob   |  25 |

| a | b | c |
| d | e | f |
";
    assert_idempotent(input);
}

// =========================================================================
// 5. Property-based tests (proptest)
// =========================================================================

mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy for generating a simple heading title (alphanumeric + spaces).
    fn heading_title() -> impl Strategy<Value = String> {
        "[A-Za-z][A-Za-z0-9 ]{0,30}".prop_map(|s| s.trim_end().to_string())
    }

    /// Strategy for generating an optional TODO keyword.
    fn maybe_keyword() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("TODO ".to_string()),
            Just("DONE ".to_string()),
        ]
    }

    /// Strategy for generating an optional priority.
    fn maybe_priority() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just("[#A] ".to_string()),
            Just("[#B] ".to_string()),
            Just("[#C] ".to_string()),
        ]
    }

    /// Strategy for generating an optional tag suffix.
    fn maybe_tags() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            Just(" :tag1:".to_string()),
            Just(" :tag1:tag2:".to_string()),
            Just(" :work:urgent:".to_string()),
        ]
    }

    /// Strategy for generating a valid heading level (1-5).
    fn heading_level() -> impl Strategy<Value = usize> {
        1..=5usize
    }

    /// Strategy for generating a simple body paragraph.
    fn maybe_body() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::new()),
            "[A-Za-z][A-Za-z0-9 .]{0,50}".prop_map(|s| format!("\n{}\n", s.trim_end())),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn random_heading_idempotent(
            level in heading_level(),
            kw in maybe_keyword(),
            priority in maybe_priority(),
            title in heading_title(),
            tags in maybe_tags(),
            body in maybe_body(),
        ) {
            let stars = "*".repeat(level);
            let input = format!("{stars} {kw}{priority}{title}{tags}{body}\n");
            let first = round_trip(&input);
            let second = round_trip(&first);
            prop_assert_eq!(
                &first, &second,
                "Idempotency violation for input:\n{}", input
            );
        }

        #[test]
        fn random_heading_json_survives_round_trip(
            level in heading_level(),
            title in heading_title(),
        ) {
            let stars = "*".repeat(level);
            let input = format!("{stars} {title}\n");
            let entries = org_to_entries(&input);
            let jsonl = entries_to_jsonl(&entries);
            let recovered = jsonl_to_entries(&jsonl);
            prop_assert_eq!(
                entries, recovered,
                "JSON round-trip failure for input:\n{}", input
            );
        }
    }

    /// Strategy for generating simple inline content strings (no markup conflicts).
    fn inline_text() -> impl Strategy<Value = String> {
        "[A-Za-z][A-Za-z0-9 ,;.!?]{0,40}".prop_map(|s| s.trim_end().to_string())
    }

    /// Strategy for generating a paragraph wrapped in a heading.
    fn simple_paragraph_in_heading() -> impl Strategy<Value = String> {
        (heading_title(), inline_text()).prop_map(|(title, body)| format!("* {title}\n\n{body}\n"))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn random_paragraph_idempotent(input in simple_paragraph_in_heading()) {
            let first = round_trip(&input);
            let second = round_trip(&first);
            prop_assert_eq!(
                &first, &second,
                "Paragraph idempotency violation for input:\n{}", input
            );
        }

        #[test]
        fn random_paragraph_json_round_trip(input in simple_paragraph_in_heading()) {
            let entries = org_to_entries(&input);
            let jsonl = entries_to_jsonl(&entries);
            let recovered = jsonl_to_entries(&jsonl);
            prop_assert_eq!(
                entries, recovered,
                "Paragraph JSON round-trip failure for input:\n{}", input
            );
        }
    }

    /// Strategy for generating a simple unordered list.
    fn simple_list() -> impl Strategy<Value = String> {
        proptest::collection::vec(inline_text(), 1..=5).prop_map(|items| {
            let list_str: String = items.iter().map(|item| format!("- {item}\n")).collect();
            format!("* List heading\n\n{list_str}")
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        #[test]
        fn random_list_idempotent(input in simple_list()) {
            let first = round_trip(&input);
            let second = round_trip(&first);
            prop_assert_eq!(
                &first, &second,
                "List idempotency violation for input:\n{}", input
            );
        }
    }

    /// Strategy for generating a simple table.
    fn simple_table() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            proptest::collection::vec("[A-Za-z][A-Za-z0-9]{0,6}", 2..=4),
            2..=5,
        )
        .prop_map(|rows| {
            let mut table = String::from("* Table heading\n\n");
            for row in &rows {
                table.push('|');
                for cell in row {
                    table.push(' ');
                    table.push_str(cell);
                    table.push_str(" |");
                }
                table.push('\n');
            }
            table
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        #[test]
        fn random_table_idempotent(input in simple_table()) {
            let first = round_trip(&input);
            let second = round_trip(&first);
            prop_assert_eq!(
                &first, &second,
                "Table idempotency violation for input:\n{}", input
            );
        }
    }

    /// Strategy for generating nested headings.
    fn nested_headings() -> impl Strategy<Value = String> {
        (
            1..=4usize,
            heading_title(),
            heading_title(),
            heading_title(),
        )
            .prop_map(|(depth, t1, t2, t3)| {
                let mut out = String::new();
                let s1 = "*".to_string();
                out.push_str(&format!("{s1} {t1}\n"));
                if depth >= 2 {
                    let s2 = "*".repeat(2);
                    out.push_str(&format!("\n{s2} {t2}\n"));
                }
                if depth >= 3 {
                    let s3 = "*".repeat(3);
                    out.push_str(&format!("\n{s3} {t3}\n"));
                }
                out
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn random_nested_headings_idempotent(input in nested_headings()) {
            let first = round_trip(&input);
            let second = round_trip(&first);
            prop_assert_eq!(
                &first, &second,
                "Nested headings idempotency violation for input:\n{}", input
            );
        }
    }
}

// =========================================================================
// 6. Additional structural tests
// =========================================================================

#[test]
fn entries_to_org_no_trailing_whitespace_on_any_line() {
    let input = "\
#+TITLE: Test

* TODO [#A] Heading :tag:
SCHEDULED: <2024-01-15 Mon>
:PROPERTIES:
:ID: test
:END:

Body with *bold* and ~code~.

- List item one
- List item two

| a | b |
|---+---|
| c | d |

#+begin_src rust
fn main() {}
#+end_src

** Child

Child body.
";
    let output = round_trip(input);
    for (i, line) in output.lines().enumerate() {
        assert_eq!(
            line,
            line.trim_end(),
            "Line {i} has trailing whitespace: {:?}",
            line
        );
    }
}

#[test]
fn entries_to_org_ends_with_newline() {
    let cases = vec![
        "* Heading\n",
        "* H1\n\n* H2\n",
        "#+TITLE: T\n\n* H\n",
        "Some text.\n",
    ];
    for input in cases {
        let output = round_trip(input);
        assert!(
            output.ends_with('\n'),
            "Output does not end with newline for input: {input:?}"
        );
    }
}

#[test]
fn entities_round_trip() {
    let input = "\
* Entities

Entities: \\alpha \\beta \\gamma
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn export_snippet_round_trip() {
    let input = "\
* Export snippet

This has @@html:<br/>@@ in it.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn inline_src_round_trip() {
    let input = "\
* Inline source

Result: src_python{1+1}.
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn statistics_cookie_round_trip() {
    let input = "\
* Progress [2/3]

- [X] Done
- [X] Done
- [ ] Not done
";
    assert_idempotent(input);
    assert_json_round_trip(input);
}

#[test]
fn no_panic_on_many_newlines() {
    let input = "\n\n\n\n\n* Heading\n\n\n\n\n";
    // Should not panic, and idempotency should hold after normalization.
    let first = round_trip(input);
    let second = round_trip(&first);
    assert_eq!(first, second);
}

#[test]
fn large_heading_count() {
    let mut input = String::new();
    for i in 1..=50 {
        input.push_str(&format!("* Heading {i}\n\nBody {i}.\n\n"));
    }
    let entries = org_to_entries(&input);
    assert_eq!(entries.len(), 50);
    assert_idempotent(&input);
}

#[test]
fn entry_count_matches_top_level_structure() {
    // Section + 3 headings = 4 entries
    let input = "\
#+TITLE: Test

* H1
* H2
* H3
";
    let entries = org_to_entries(input);
    assert_eq!(entries.len(), 4);

    // Children are NOT separate entries; they nest inside their parent.
    let input2 = "\
* Parent
** Child
*** Grandchild
";
    let entries2 = org_to_entries(input2);
    assert_eq!(entries2.len(), 1);
}

// =========================================================================
// 7. Location field serialization tests
// =========================================================================

#[test]
fn location_fields_serialize_when_present() {
    let entry = OrgEntry {
        schema_version: SCHEMA_VERSION,
        file: Some("test.org".to_string()),
        char_begin: Some(0),
        char_end: Some(42),
        line_begin: Some(1),
        line_end: Some(3),
        content: EntryContent::Heading(Box::new(Heading {
            level: 1,
            keyword: None,
            priority: None,
            title: vec![InlineContent::Text {
                value: "Test".to_string(),
            }],
            tags: vec![],
            planning: None,
            properties: vec![],
            pre_body_blank: None,
            body: vec![],
            body_spacing: vec![],
            post_body_blank: None,
            children: vec![],
            post_blank: None,
        })),
        post_blank: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["file"], "test.org");
    assert_eq!(parsed["char_begin"], 0);
    assert_eq!(parsed["char_end"], 42);
    assert_eq!(parsed["line_begin"], 1);
    assert_eq!(parsed["line_end"], 3);
}

#[test]
fn location_fields_omitted_when_none() {
    let entry = OrgEntry {
        schema_version: SCHEMA_VERSION,
        file: None,
        char_begin: None,
        char_end: None,
        line_begin: None,
        line_end: None,
        content: EntryContent::Heading(Box::new(Heading {
            level: 1,
            keyword: None,
            priority: None,
            title: vec![InlineContent::Text {
                value: "Test".to_string(),
            }],
            tags: vec![],
            planning: None,
            properties: vec![],
            pre_body_blank: None,
            body: vec![],
            body_spacing: vec![],
            post_body_blank: None,
            children: vec![],
            post_blank: None,
        })),
        post_blank: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("file").is_none());
    assert!(parsed.get("char_begin").is_none());
    assert!(parsed.get("char_end").is_none());
    assert!(parsed.get("line_begin").is_none());
    assert!(parsed.get("line_end").is_none());
}

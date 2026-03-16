//! Canonical pretty-printer: converts the JSON data model back into Org-mode text.
//!
//! This module is deterministic -- the same [`OrgEntry`] slice always produces
//! byte-identical output.  The output follows these canonical-form rules:
//!
//! * Blank lines between top-level entries controlled by `post_blank` field
//!   (defaults to 1 blank line when absent)
//! * No trailing whitespace on any line
//! * Property drawers immediately after the heading (planning line in between
//!   when present)
//! * UTF-8, LF line endings, file ends with at least one newline

use crate::model::{
    CheckboxState, Element, EntryContent, Heading, InlineContent, ListItem, OrgEntry, Planning,
    Property, TableRow, TableRowKind,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render a slice of [`OrgEntry`] values into canonical Org-mode text.
///
/// Inter-entry spacing is controlled by each entry's `post_blank` field.
/// When absent, entries are separated by one blank line (default), and the
/// last entry has zero trailing blank lines.  The returned string always
/// ends with at least one newline.
pub fn entries_to_org(entries: &[OrgEntry]) -> String {
    let mut buf = String::new();
    for (i, entry) in entries.iter().enumerate() {
        write_entry(&mut buf, entry);
        // Emit post_blank blank lines after this entry
        let blank_count = entry.post_blank.unwrap_or_else(|| {
            // Default: 1 blank line between entries, 0 for the last entry
            if i + 1 < entries.len() {
                1
            } else {
                0
            }
        });
        for _ in 0..blank_count {
            buf.push('\n');
        }
    }
    ensure_final_newline(&mut buf);
    buf
}

/// Render a single [`OrgEntry`] into canonical Org-mode text.
pub fn entry_to_org(entry: &OrgEntry) -> String {
    let mut buf = String::new();
    write_entry(&mut buf, entry);
    ensure_final_newline(&mut buf);
    buf
}

// ---------------------------------------------------------------------------
// Entry / section / heading
// ---------------------------------------------------------------------------

fn write_entry(buf: &mut String, entry: &OrgEntry) {
    match &entry.content {
        EntryContent::Section {
            elements,
            body_spacing,
        } => {
            write_elements_with_spacing(buf, elements, body_spacing, 0, false);
        }
        EntryContent::Heading(heading) => {
            write_heading(buf, heading);
        }
    }
}

/// Render a heading and all of its descendants (planning, properties, body,
/// children).
fn write_heading(buf: &mut String, heading: &Heading) {
    // --- Heading line ---
    write_heading_line(buf, heading);

    // --- Planning (immediately after heading line) ---
    if let Some(planning) = &heading.planning {
        write_planning(buf, planning);
    }

    // --- Property drawer (immediately after planning / heading) ---
    if !heading.properties.is_empty() {
        write_properties(buf, &heading.properties);
    }

    // --- Blank lines before body content ---
    if let Some(count) = heading.pre_body_blank {
        for _ in 0..count {
            buf.push('\n');
        }
    }

    // --- Body elements ---
    if !heading.body.is_empty() {
        write_elements_with_spacing(buf, &heading.body, &heading.body_spacing, 0, false);
    }

    // --- Blank lines after body (before children or next sibling) ---
    if let Some(count) = heading.post_body_blank {
        for _ in 0..count {
            buf.push('\n');
        }
    }

    // --- Child headings ---
    for child in &heading.children {
        write_heading(buf, child);
        // Emit post_blank blank lines after this child heading
        let blank_count = child.post_blank.unwrap_or(0);
        for _ in 0..blank_count {
            buf.push('\n');
        }
    }
}

/// Produce the `* KEYWORD [#PRIORITY] Title  :tag1:tag2:` line.
fn write_heading_line(buf: &mut String, heading: &Heading) {
    // Stars
    for _ in 0..heading.level {
        buf.push('*');
    }
    buf.push(' ');

    // Keyword (TODO / DONE / ...)
    if let Some(kw) = &heading.keyword {
        buf.push_str(kw);
        buf.push(' ');
    }

    // Priority
    if let Some(pri) = &heading.priority {
        buf.push_str("[#");
        buf.push_str(pri);
        buf.push_str("] ");
    }

    // Title (inline content)
    write_inline(buf, &heading.title);

    // Tags -- right-aligned is nice but canonical form just needs correctness.
    // Org-mode format: a single space then `:tag1:tag2:`
    if !heading.tags.is_empty() {
        // Ensure there is a space before the tags (title may already end with
        // space, but canonical form always has exactly one).
        if !buf.ends_with(' ') {
            buf.push(' ');
        }
        buf.push(':');
        for tag in &heading.tags {
            buf.push_str(tag);
            buf.push(':');
        }
    }

    // Trim trailing whitespace from the heading line (rule 2).
    trim_trailing_whitespace(buf);
    buf.push('\n');
}

/// Render planning keywords.  Order: SCHEDULED, DEADLINE, CLOSED (standard
/// Org convention).
fn write_planning(buf: &mut String, planning: &Planning) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(ts) = &planning.scheduled {
        parts.push(format!("SCHEDULED: {ts}"));
    }
    if let Some(ts) = &planning.deadline {
        parts.push(format!("DEADLINE: {ts}"));
    }
    if let Some(ts) = &planning.closed {
        parts.push(format!("CLOSED: {ts}"));
    }
    if !parts.is_empty() {
        buf.push_str(&parts.join(" "));
        buf.push('\n');
    }
}

/// Render a `:PROPERTIES:` drawer.
/// Uses Emacs org-property-format convention: `:KEY:` portion is padded to 10 chars minimum.
fn write_properties(buf: &mut String, properties: &[Property]) {
    buf.push_str(":PROPERTIES:\n");
    for prop in properties {
        buf.push(':');
        buf.push_str(&prop.key);
        buf.push(':');
        // Emacs org-property-format: "%-10s %s" where the first %s is KEY:
        // So KEY: is padded to 10 chars minimum
        let key_colon_len = prop.key.len() + 2; // ":" + key + ":"
        let pad_to = 10usize; // Emacs default
        let padding = pad_to.saturating_sub(key_colon_len);
        for _ in 0..padding {
            buf.push(' ');
        }
        buf.push(' '); // always at least one space
        buf.push_str(&prop.value);
        buf.push('\n');
    }
    buf.push_str(":END:\n");
}

// ---------------------------------------------------------------------------
// Block-level elements
// ---------------------------------------------------------------------------

/// Check if an element is "light" (keyword-like) that doesn't need blank lines around it.
fn is_light_element(elem: &Element) -> bool {
    matches!(
        elem,
        Element::Keyword { .. } | Element::AffiliatedKeyword { .. } | Element::Raw { .. }
    )
}

/// Determine if a blank line is needed between two consecutive elements.
fn needs_blank_line_between(prev: &Element, next: &Element) -> bool {
    // No blank line after affiliated keyword
    if matches!(prev, Element::AffiliatedKeyword { .. }) {
        return false;
    }
    // No blank line between consecutive light elements
    if is_light_element(prev) && is_light_element(next) {
        return false;
    }
    true
}

/// Write a sequence of elements, separated by blank lines.
///
/// When `indent_contents` is true every line of every element is indented by
/// `indent` spaces (used for elements nested inside special blocks).
///
/// `body_spacing` optionally provides explicit inter-element spacing
/// information: `body_spacing[i]` indicates whether there should be a blank
/// line between `elements[i]` and `elements[i+1]`.  When empty, spacing is
/// determined heuristically by `needs_blank_line_between`.
fn write_elements(buf: &mut String, elements: &[Element], indent: usize, indent_contents: bool) {
    write_elements_with_spacing(buf, elements, &[], indent, indent_contents);
}

/// Write elements with explicit inter-element spacing control.
fn write_elements_with_spacing(
    buf: &mut String,
    elements: &[Element],
    body_spacing: &[bool],
    indent: usize,
    indent_contents: bool,
) {
    for (i, elem) in elements.iter().enumerate() {
        if i > 0 {
            let has_spacing_info = i - 1 < body_spacing.len();
            let needs_blank = if has_spacing_info {
                body_spacing[i - 1]
            } else {
                let prev = &elements[i - 1];
                needs_blank_line_between(prev, elem)
            };
            if needs_blank {
                buf.push('\n');
            }
        }
        if indent_contents {
            write_element_indented(buf, elem, indent);
        } else {
            write_element(buf, elem, indent);
        }
    }
}

/// Write a single element.  `indent` is the *base* indentation in spaces
/// (used inside list items, nested blocks, etc.).
fn write_element(buf: &mut String, element: &Element, indent: usize) {
    let prefix = " ".repeat(indent);
    match element {
        Element::Paragraph { contents } => {
            write_paragraph(buf, contents, &prefix);
        }
        Element::PlainList { kind: _, items } => {
            write_list(buf, items, indent);
        }
        Element::SrcBlock {
            language,
            parameters,
            value,
        } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_src");
            if !language.is_empty() {
                buf.push(' ');
                buf.push_str(language);
            }
            if let Some(params) = parameters {
                if !params.is_empty() {
                    buf.push(' ');
                    buf.push_str(params);
                }
            }
            buf.push('\n');
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str("#+end_src\n");
        }
        Element::ExampleBlock { value } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_example\n");
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str("#+end_example\n");
        }
        Element::QuoteBlock { elements } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_quote\n");
            write_elements(buf, elements, indent, false);
            buf.push_str(&prefix);
            buf.push_str("#+end_quote\n");
        }
        Element::CenterBlock { elements } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_center\n");
            write_elements(buf, elements, indent, false);
            buf.push_str(&prefix);
            buf.push_str("#+end_center\n");
        }
        Element::VerseBlock { value } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_verse\n");
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str("#+end_verse\n");
        }
        Element::CommentBlock { value } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_comment\n");
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str("#+end_comment\n");
        }
        Element::ExportBlock { backend, value } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_export");
            if !backend.is_empty() {
                buf.push(' ');
                buf.push_str(backend);
            }
            buf.push('\n');
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str("#+end_export\n");
        }
        Element::SpecialBlock {
            name,
            parameters,
            value,
        } => {
            buf.push_str(&prefix);
            buf.push_str("#+begin_");
            buf.push_str(name);
            if let Some(params) = parameters {
                if !params.is_empty() {
                    buf.push(' ');
                    buf.push_str(params);
                }
            }
            buf.push('\n');
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str("#+end_");
            buf.push_str(name);
            buf.push('\n');
        }
        Element::Drawer { name, value } => {
            buf.push_str(&prefix);
            buf.push(':');
            buf.push_str(name);
            buf.push_str(":\n");
            write_block_value_raw(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str(":END:\n");
        }
        Element::Table { rows } => {
            write_table(buf, rows, &prefix);
        }
        Element::HorizontalRule { dash_count } => {
            buf.push_str(&prefix);
            let count = dash_count.unwrap_or(5);
            for _ in 0..count {
                buf.push('-');
            }
            buf.push('\n');
        }
        Element::Keyword { key, value } => {
            buf.push_str(&prefix);
            buf.push_str("#+");
            buf.push_str(key);
            buf.push(':');
            buf.push_str(value);
            buf.push('\n');
        }
        Element::Comment { value } => {
            for line in value.lines() {
                buf.push_str(&prefix);
                if line.is_empty() {
                    buf.push('#');
                } else {
                    buf.push_str("# ");
                    buf.push_str(line);
                }
                buf.push('\n');
            }
            // Handle the case where value is empty
            if value.is_empty() {
                buf.push_str(&prefix);
                buf.push_str("#\n");
            }
        }
        Element::FixedWidth { value } => {
            for line in value.lines() {
                buf.push_str(&prefix);
                buf.push_str(": ");
                buf.push_str(line);
                buf.push('\n');
            }
            if value.is_empty() {
                buf.push_str(&prefix);
                buf.push_str(":\n");
            }
        }
        Element::Clock { value } => {
            buf.push_str(&prefix);
            buf.push_str("CLOCK: ");
            buf.push_str(value);
            buf.push('\n');
        }
        Element::DiarySexp { value } => {
            buf.push_str(&prefix);
            buf.push_str(value);
            buf.push('\n');
        }
        Element::FootnoteDefinition { label, elements } => {
            buf.push_str(&prefix);
            buf.push_str("[fn:");
            buf.push_str(label);
            buf.push_str("] ");
            // The first element (usually a paragraph) is inlined after the
            // label; remaining elements go on subsequent lines.
            if let Some((first, rest)) = elements.split_first() {
                write_element_inline_start(buf, first, indent);
                if !rest.is_empty() {
                    write_elements(buf, rest, indent, false);
                }
            } else {
                buf.push('\n');
            }
        }
        Element::AffiliatedKeyword { key, value } => {
            buf.push_str(&prefix);
            buf.push_str("#+");
            buf.push_str(key);
            buf.push(':');
            buf.push_str(value);
            buf.push('\n');
        }
        Element::LatexEnvironment { value } => {
            // LaTeX environments are stored verbatim (they already include
            // \begin{...} and \end{...}).
            for line in value.lines() {
                buf.push_str(&prefix);
                buf.push_str(line);
                buf.push('\n');
            }
            if value.is_empty() {
                buf.push('\n');
            }
        }
        Element::DynamicBlock {
            name,
            parameters,
            elements,
        } => {
            buf.push_str(&prefix);
            buf.push_str("#+BEGIN: ");
            buf.push_str(name);
            if let Some(params) = parameters {
                if !params.is_empty() {
                    buf.push(' ');
                    buf.push_str(params);
                }
            }
            buf.push('\n');
            write_elements(buf, elements, indent, false);
            buf.push_str(&prefix);
            buf.push_str("#+END:\n");
        }
        Element::Raw { value } => {
            for line in value.lines() {
                buf.push_str(&prefix);
                buf.push_str(line);
                buf.push('\n');
            }
            if value.is_empty() {
                buf.push('\n');
            }
        }
    }
}

/// Render a paragraph, wrapping the inline contents with the given prefix on
/// each line.  A paragraph is always followed by a newline.
fn write_paragraph(buf: &mut String, contents: &[InlineContent], prefix: &str) {
    let text = inline_to_string(contents);
    for line in text.lines() {
        buf.push_str(prefix);
        let trimmed = line.trim_end();
        buf.push_str(trimmed);
        buf.push('\n');
    }
    // If inline_to_string returned an empty string, still emit one newline so
    // the paragraph occupies a line.
    if text.is_empty() {
        buf.push_str(prefix);
        buf.push('\n');
    }
}

/// Write the body of a block element (src, example, etc.).
///
/// The value is already the raw text inside the block.  Each line is prefixed
/// with the given indentation.  If the value does not end with a newline we
/// add one so that the closing `#+end_*` lands on its own line.
///
/// When `comma_escape` is true, lines starting with `*` or `#+` are
/// comma-escaped (orgize strips leading commas when parsing these block
/// types, so we add them back on writing).  Comma escaping applies to
/// src, example, verse, export, and comment blocks — NOT drawers.
fn write_block_value(buf: &mut String, value: &str, prefix: &str) {
    write_block_value_inner(buf, value, prefix, true);
}

fn write_block_value_raw(buf: &mut String, value: &str, prefix: &str) {
    write_block_value_inner(buf, value, prefix, false);
}

fn write_block_value_inner(buf: &mut String, value: &str, prefix: &str, comma_escape: bool) {
    if value.is_empty() {
        return;
    }
    for line in value.lines() {
        buf.push_str(prefix);
        if comma_escape && (line.starts_with('*') || line.starts_with("#+")) {
            buf.push(',');
        }
        buf.push_str(line);
        buf.push('\n');
    }
}

/// Write an element but indent every output line by `indent` spaces.
fn write_element_indented(buf: &mut String, element: &Element, indent: usize) {
    // Render into a temporary buffer, then indent each line.
    let mut tmp = String::new();
    write_element(&mut tmp, element, indent);
    for line in tmp.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            buf.push('\n');
        } else {
            buf.push_str(trimmed);
            buf.push('\n');
        }
    }
}

/// Write the first element of a footnote definition inline (no leading
/// prefix, since the `[fn:label] ` prefix is already in the buffer).
fn write_element_inline_start(buf: &mut String, element: &Element, indent: usize) {
    match element {
        Element::Paragraph { contents } => {
            let text = inline_to_string(contents);
            let trimmed = text.trim_end();
            buf.push_str(trimmed);
            buf.push('\n');
        }
        _ => {
            // Fallback: render the element normally with a preceding newline
            // (rare for footnotes, but handle it).
            buf.push('\n');
            write_element(buf, element, indent);
        }
    }
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

/// Render list items.
fn write_list(buf: &mut String, items: &[ListItem], base_indent: usize) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            // No blank line between sibling list items by default in canonical
            // form; sub-items are nested via indentation.
        }
        write_list_item(buf, item, base_indent);
    }
}

/// Render a single list item.
fn write_list_item(buf: &mut String, item: &ListItem, base_indent: usize) {
    let prefix = " ".repeat(base_indent);
    buf.push_str(&prefix);
    buf.push_str(&item.bullet);
    // Ensure space after bullet
    if !item.bullet.ends_with(' ') {
        buf.push(' ');
    }

    // Counter set ([@N])
    if let Some(counter) = &item.counter_set {
        buf.push_str("[@");
        buf.push_str(counter);
        buf.push_str("] ");
    }

    // Checkbox
    if let Some(cb) = &item.checkbox {
        match cb {
            CheckboxState::Checked => buf.push_str("[X] "),
            CheckboxState::Unchecked => buf.push_str("[ ] "),
            CheckboxState::Partial => buf.push_str("[-] "),
        }
    }

    // Tag (descriptive lists: `tag ::`)
    if let Some(tag_contents) = &item.tag {
        write_inline(buf, tag_contents);
        buf.push_str(" :: ");
    }

    // Content elements -- the first paragraph is inlined on the bullet line;
    // subsequent elements are indented under the item.
    let body_indent = base_indent + bullet_body_indent(&item.bullet);

    if let Some((first, rest)) = item.contents.split_first() {
        write_list_item_first_element(buf, first, body_indent);
        for (idx, elem) in rest.iter().enumerate() {
            // idx is 0-based within `rest`, so the spacing index for the
            // PREVIOUS element is `idx` (since rest starts at contents[1]).
            let prev_spacing = item.content_spacing.get(idx).copied().unwrap_or(0);
            // If the previous element had trailing blank lines, or if we're
            // between consecutive paragraphs (which always have a separating
            // blank line in Org-mode), emit blank lines.
            let blank_count = if prev_spacing > 0 {
                prev_spacing
            } else if matches!(item.contents.get(idx), Some(Element::Paragraph { .. }))
                && matches!(elem, Element::Paragraph { .. })
            {
                1
            } else {
                0
            };
            for _ in 0..blank_count {
                buf.push('\n');
            }
            write_list_continuation_element(buf, elem, body_indent);
        }
    } else {
        // Empty item -- just end the line.
        trim_trailing_whitespace(buf);
        buf.push('\n');
    }

    // Emit post_blank blank lines
    if let Some(count) = item.post_blank {
        for _ in 0..count {
            buf.push('\n');
        }
    }
}

/// Determine how many extra spaces to indent continuation lines of a list
/// item based on the bullet string.  The continuation should align after the
/// bullet + one space.
fn bullet_body_indent(bullet: &str) -> usize {
    // Bullet strings from the parser look like "- ", "+ ", "1. ", "1) ", etc.
    // We want the body to start at the column after the bullet + space.
    let trimmed = bullet.trim_end();
    // +1 for the space after the bullet
    trimmed.len() + 1
}

/// Write the first element of a list item *inline* (no preceding indentation
/// or newline -- the bullet prefix is already in the buffer).
/// For multi-line paragraphs, continuation lines are indented to align with the first line.
fn write_list_item_first_element(buf: &mut String, element: &Element, body_indent: usize) {
    match element {
        Element::Paragraph { contents } => {
            let text = inline_to_string(contents);
            let lines: Vec<&str> = text.lines().collect();
            if let Some((first, rest)) = lines.split_first() {
                // First line goes inline after the bullet
                buf.push_str(first.trim_end());
                buf.push('\n');

                // Remaining lines: the parser already stripped exactly
                // body_indent characters of leading whitespace, so any
                // remaining leading whitespace is intentional (extra
                // indentation beyond the standard body indent).  Just
                // prepend body_indent spaces and preserve the rest.
                let indent_str = " ".repeat(body_indent);
                for line in rest {
                    if line.trim().is_empty() {
                        buf.push('\n');
                    } else {
                        buf.push_str(&indent_str);
                        buf.push_str(line.trim_end());
                        buf.push('\n');
                    }
                }
            } else {
                // Empty paragraph — trim any trailing whitespace (e.g.,
                // the space after " :: " on a descriptive list tag line).
                trim_trailing_whitespace(buf);
                buf.push('\n');
            }
        }
        _ => {
            // Non-paragraph first element: put it on the next line.
            trim_trailing_whitespace(buf);
            buf.push('\n');
            write_element(buf, element, 0);
        }
    }
}

/// Write a continuation element (not the first) inside a list item.
///
/// orgize preserves source indentation inside all element values (paragraphs,
/// src blocks, etc.) within list items.  The writer needs to strip the
/// original indentation and apply its own `body_indent` so indentation
/// isn't doubled.
///
/// For paragraphs we strip all leading whitespace per line (orgize's inter-
/// element whitespace is inconsistent across inline boundaries).  For all
/// other elements we render with indent=0 into a temp buffer, measure
/// the minimum leading whitespace, strip it, then re-indent.
fn write_list_continuation_element(buf: &mut String, element: &Element, body_indent: usize) {
    match element {
        Element::Paragraph { contents } => {
            let text = inline_to_string(contents);
            let indent_str = " ".repeat(body_indent);
            for line in text.lines() {
                if line.trim().is_empty() {
                    buf.push('\n');
                } else {
                    buf.push_str(&indent_str);
                    buf.push_str(line.trim_end());
                    buf.push('\n');
                }
            }
        }
        _ => {
            // Render with indent=0 to get the raw output.
            // The parser has already stripped body_indent, so we just need to add it back.
            let mut tmp = String::new();
            write_element(&mut tmp, element, 0);
            let indent_str = " ".repeat(body_indent);
            for line in tmp.lines() {
                if line.is_empty() {
                    buf.push('\n');
                } else {
                    buf.push_str(&indent_str);
                    buf.push_str(line);
                    buf.push('\n');
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum ColumnAlignment {
    Left,
    Right,
    Center,
}

/// Detect column alignments from alignment-cookie rows.
fn detect_column_alignments(rows: &[TableRow]) -> Vec<ColumnAlignment> {
    let mut alignments: Vec<ColumnAlignment> = Vec::new();
    for row in rows {
        if let TableRowKind::Standard { cells } = &row.kind {
            // Check if this is an alignment-cookie row: all non-empty cells match <r>, <l>, or <c>
            let is_alignment_row = !cells.is_empty()
                && cells.iter().all(|cell| {
                    let text = inline_to_string(cell).trim().to_string();
                    text.is_empty() || text == "<r>" || text == "<l>" || text == "<c>"
                });
            if is_alignment_row {
                // Expand alignments vector if needed
                while alignments.len() < cells.len() {
                    alignments.push(ColumnAlignment::Left);
                }
                for (i, cell) in cells.iter().enumerate() {
                    let text = inline_to_string(cell).trim().to_string();
                    if text == "<r>" {
                        alignments[i] = ColumnAlignment::Right;
                    } else if text == "<c>" {
                        alignments[i] = ColumnAlignment::Center;
                    } else if text == "<l>" {
                        alignments[i] = ColumnAlignment::Left;
                    }
                }
            }
        }
    }
    alignments
}

/// Render an Org table.
fn write_table(buf: &mut String, rows: &[TableRow], prefix: &str) {
    // Detect column alignments from alignment-cookie rows
    let alignments = detect_column_alignments(rows);

    // Track the most recent standard row's cell widths for rule rows
    // (rule rows use the widths from the preceding/following standard row).
    let mut last_standard_widths: Vec<usize> = Vec::new();

    // Pre-compute: find first standard row's widths for rule rows that
    // appear before any standard row.
    for row in rows {
        if let TableRowKind::Standard { .. } = &row.kind {
            last_standard_widths = row_effective_widths(row);
            break;
        }
    }

    let mut current_widths = last_standard_widths.clone();

    for row in rows {
        buf.push_str(prefix);
        match &row.kind {
            TableRowKind::Standard { cells } => {
                current_widths = row_effective_widths(row);
                buf.push('|');
                let num_cols = current_widths.len().max(cells.len());
                for i in 0..num_cols {
                    let text = cells
                        .get(i)
                        .map(|c| inline_to_string(c))
                        .unwrap_or_default();
                    let width = current_widths.get(i).copied().unwrap_or(text.len());
                    let alignment = alignments.get(i).copied().unwrap_or(ColumnAlignment::Left);

                    buf.push(' ');
                    match alignment {
                        ColumnAlignment::Left => {
                            buf.push_str(&text);
                            let padding = width.saturating_sub(text.len());
                            for _ in 0..padding {
                                buf.push(' ');
                            }
                        }
                        ColumnAlignment::Right => {
                            let padding = width.saturating_sub(text.len());
                            for _ in 0..padding {
                                buf.push(' ');
                            }
                            buf.push_str(&text);
                        }
                        ColumnAlignment::Center => {
                            let total_padding = width.saturating_sub(text.len());
                            let left_pad = total_padding / 2;
                            let right_pad = total_padding - left_pad;
                            for _ in 0..left_pad {
                                buf.push(' ');
                            }
                            buf.push_str(&text);
                            for _ in 0..right_pad {
                                buf.push(' ');
                            }
                        }
                    }
                    buf.push_str(" |");
                }
                buf.push('\n');
            }
            TableRowKind::Rule => {
                buf.push('|');
                // Use cell_widths from the row if available (measured from
                // raw rule text), otherwise use widths from the preceding
                // standard row.
                let widths = if row.cell_widths.is_empty() {
                    &current_widths
                } else {
                    &row.cell_widths
                };
                for (i, &w) in widths.iter().enumerate() {
                    for _ in 0..(w + 2) {
                        buf.push('-');
                    }
                    if i + 1 < widths.len() {
                        buf.push('+');
                    } else {
                        buf.push('|');
                    }
                }
                buf.push('\n');
            }
        }
    }
}

/// Get the effective cell widths for a table row.
/// Uses stored cell_widths if available, otherwise computes from content.
fn row_effective_widths(row: &TableRow) -> Vec<usize> {
    if !row.cell_widths.is_empty() {
        return row.cell_widths.clone();
    }
    // Fall back: compute from rendered content
    if let TableRowKind::Standard { cells } = &row.kind {
        cells
            .iter()
            .map(|c| {
                let w = inline_to_string(c).len();
                if w == 0 {
                    1
                } else {
                    w
                }
            })
            .collect()
    } else {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Inline content
// ---------------------------------------------------------------------------

/// Render inline content into a String.
pub(crate) fn inline_to_string(contents: &[InlineContent]) -> String {
    let mut s = String::new();
    write_inline(&mut s, contents);
    s
}

/// Append inline content to a buffer.
fn write_inline(buf: &mut String, contents: &[InlineContent]) {
    for item in contents {
        write_inline_content(buf, item);
    }
}

/// Append a single inline content item to a buffer.
fn write_inline_content(buf: &mut String, content: &InlineContent) {
    match content {
        InlineContent::Text { value } => {
            buf.push_str(value);
        }
        InlineContent::Bold { contents } => {
            buf.push('*');
            write_inline(buf, contents);
            buf.push('*');
        }
        InlineContent::Italic { contents } => {
            buf.push('/');
            write_inline(buf, contents);
            buf.push('/');
        }
        InlineContent::Underline { contents } => {
            buf.push('_');
            write_inline(buf, contents);
            buf.push('_');
        }
        InlineContent::StrikeThrough { contents } => {
            buf.push('+');
            write_inline(buf, contents);
            buf.push('+');
        }
        InlineContent::Code { value } => {
            buf.push('~');
            buf.push_str(value);
            buf.push('~');
        }
        InlineContent::Verbatim { value } => {
            buf.push('=');
            buf.push_str(value);
            buf.push('=');
        }
        InlineContent::Link { path, description } => {
            buf.push_str("[[");
            buf.push_str(path);
            if let Some(desc) = description {
                buf.push_str("][");
                write_inline(buf, desc);
            }
            buf.push_str("]]");
        }
        InlineContent::Timestamp { value } => {
            buf.push_str(value);
        }
        InlineContent::FootnoteReference { label, definition } => {
            buf.push_str("[fn:");
            if let Some(lbl) = label {
                buf.push_str(lbl);
            }
            if let Some(def) = definition {
                buf.push(':');
                write_inline(buf, def);
            }
            buf.push(']');
        }
        InlineContent::LineBreak => {
            buf.push_str("\\\\\n");
        }
        InlineContent::Entity { name } => {
            buf.push('\\');
            buf.push_str(name);
        }
        InlineContent::LatexFragment { value } => {
            buf.push_str(value);
        }
        InlineContent::ExportSnippet { backend, value } => {
            buf.push_str("@@");
            buf.push_str(backend);
            buf.push(':');
            buf.push_str(value);
            buf.push_str("@@");
        }
        InlineContent::InlineBabel { value } => {
            buf.push_str(value);
        }
        InlineContent::InlineSrc { language, value } => {
            buf.push_str("src_");
            buf.push_str(language);
            buf.push('{');
            buf.push_str(value);
            buf.push('}');
        }
        InlineContent::Macro { value } => {
            buf.push_str("{{{");
            buf.push_str(value);
            buf.push_str("}}}");
        }
        InlineContent::Target { value } => {
            buf.push_str("<<");
            buf.push_str(value);
            buf.push_str(">>");
        }
        InlineContent::RadioTarget { value } => {
            buf.push_str("<<<");
            buf.push_str(value);
            buf.push_str(">>>");
        }
        InlineContent::StatisticsCookie { value } => {
            buf.push_str(value);
        }
        InlineContent::Subscript {
            contents,
            use_braces,
        } => {
            if *use_braces {
                buf.push_str("_{");
                write_inline(buf, contents);
                buf.push('}');
            } else {
                buf.push('_');
                write_inline(buf, contents);
            }
        }
        InlineContent::Superscript {
            contents,
            use_braces,
        } => {
            if *use_braces {
                buf.push_str("^{");
                write_inline(buf, contents);
                buf.push('}');
            } else {
                buf.push('^');
                write_inline(buf, contents);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Trim trailing whitespace from the last line currently in the buffer
/// (everything after the last `\n`, or the whole buffer if there is no `\n`).
fn trim_trailing_whitespace(buf: &mut String) {
    let trimmed_len = buf.trim_end_matches([' ', '\t']).len();
    buf.truncate(trimmed_len);
}

/// Make sure `buf` ends with at least one `\n`.
///
/// Trailing blank lines are preserved -- they are controlled by the
/// `post_blank` field on the last entry.
fn ensure_final_newline(buf: &mut String) {
    if !buf.ends_with('\n') {
        buf.push('\n');
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::SCHEMA_VERSION;

    fn text(s: &str) -> InlineContent {
        InlineContent::Text {
            value: s.to_string(),
        }
    }

    fn simple_heading(level: u32, title: &str) -> Heading {
        Heading {
            level,
            keyword: None,
            priority: None,
            title: vec![text(title)],
            tags: vec![],
            planning: None,
            properties: vec![],
            pre_body_blank: None,
            body_spacing: vec![],
            body: vec![],
            post_body_blank: None,
            children: vec![],
            post_blank: None,
        }
    }

    fn entry(content: EntryContent) -> OrgEntry {
        OrgEntry {
            schema_version: SCHEMA_VERSION,
            content,
            post_blank: None,
        }
    }

    #[test]
    fn simple_heading_line() {
        let h = simple_heading(1, "Hello");
        let e = entry(EntryContent::Heading(Box::new(h)));
        assert_eq!(entry_to_org(&e), "* Hello\n");
    }

    #[test]
    fn heading_with_keyword_and_priority() {
        let h = Heading {
            keyword: Some("TODO".into()),
            priority: Some("A".into()),
            ..simple_heading(2, "Important task")
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        assert_eq!(entry_to_org(&e), "** TODO [#A] Important task\n");
    }

    #[test]
    fn heading_with_tags() {
        let h = Heading {
            tags: vec!["work".into(), "urgent".into()],
            ..simple_heading(1, "Tagged")
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        assert_eq!(entry_to_org(&e), "* Tagged :work:urgent:\n");
    }

    #[test]
    fn heading_with_planning() {
        let h = Heading {
            planning: Some(Planning {
                closed: None,
                deadline: Some("<2024-01-15>".into()),
                scheduled: Some("<2024-01-10>".into()),
            }),
            ..simple_heading(1, "Planned")
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        let expected = "* Planned\nSCHEDULED: <2024-01-10> DEADLINE: <2024-01-15>\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn heading_with_properties() {
        let h = Heading {
            properties: vec![
                Property {
                    key: "ID".into(),
                    value: "abc-123".into(),
                },
                Property {
                    key: "CUSTOM".into(),
                    value: "val".into(),
                },
            ],
            ..simple_heading(1, "Props")
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        let expected = "\
* Props
:PROPERTIES:
:ID:       abc-123
:CUSTOM:   val
:END:
";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn heading_with_body_paragraph() {
        let h = Heading {
            body: vec![Element::Paragraph {
                contents: vec![text("Some body text.")],
            }],
            ..simple_heading(1, "Title")
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        let expected = "* Title\nSome body text.\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn heading_with_children() {
        let child = simple_heading(2, "Child");
        let h = Heading {
            children: vec![child],
            ..simple_heading(1, "Parent")
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        let expected = "* Parent\n** Child\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn multiple_entries_separated() {
        let entries = vec![
            entry(EntryContent::Heading(Box::new(simple_heading(1, "First")))),
            entry(EntryContent::Heading(Box::new(simple_heading(1, "Second")))),
        ];
        let expected = "* First\n\n* Second\n";
        assert_eq!(entries_to_org(&entries), expected);
    }

    #[test]
    fn src_block() {
        let elem = Element::SrcBlock {
            language: "rust".into(),
            parameters: Some(":tangle yes".into()),
            value: "fn main() {}\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "\
#+begin_src rust :tangle yes
fn main() {}
#+end_src
";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn inline_markup() {
        let contents = vec![
            text("Hello "),
            InlineContent::Bold {
                contents: vec![text("bold")],
            },
            text(" and "),
            InlineContent::Code {
                value: "code".into(),
            },
            text("."),
        ];
        let result = inline_to_string(&contents);
        assert_eq!(result, "Hello *bold* and ~code~.");
    }

    #[test]
    fn link_with_description() {
        let link = InlineContent::Link {
            path: "https://example.com".into(),
            description: Some(vec![text("Example")]),
        };
        let result = inline_to_string(&[link]);
        assert_eq!(result, "[[https://example.com][Example]]");
    }

    #[test]
    fn link_without_description() {
        let link = InlineContent::Link {
            path: "https://example.com".into(),
            description: None,
        };
        let result = inline_to_string(&[link]);
        assert_eq!(result, "[[https://example.com]]");
    }

    #[test]
    fn table_rendering() {
        let rows = vec![
            TableRow {
                kind: TableRowKind::Standard {
                    cells: vec![vec![text("Name")], vec![text("Age")]],
                },
                cell_widths: vec![5, 3],
            },
            TableRow {
                kind: TableRowKind::Rule,
                cell_widths: vec![5, 3],
            },
            TableRow {
                kind: TableRowKind::Standard {
                    cells: vec![vec![text("Alice")], vec![text("30")]],
                },
                cell_widths: vec![5, 3],
            },
        ];
        let elem = Element::Table { rows };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "\
| Name  | Age |
|-------+-----|
| Alice | 30  |
";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn plain_list_unordered() {
        let items = vec![
            ListItem {
                bullet: "-".into(),
                checkbox: None,
                counter_set: None,
                tag: None,
                contents: vec![Element::Paragraph {
                    contents: vec![text("First")],
                }],
                content_spacing: vec![],
                post_blank: None,
            },
            ListItem {
                bullet: "-".into(),
                checkbox: Some(CheckboxState::Checked),
                counter_set: None,
                tag: None,
                contents: vec![Element::Paragraph {
                    contents: vec![text("Second")],
                }],
                content_spacing: vec![],
                post_blank: None,
            },
        ];
        let elem = Element::PlainList {
            kind: ListKind::Unordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "- First\n- [X] Second\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn descriptive_list() {
        let items = vec![ListItem {
            bullet: "-".into(),
            checkbox: None,
            counter_set: None,
            tag: Some(vec![text("Term")]),
            contents: vec![Element::Paragraph {
                contents: vec![text("Definition here")],
            }],
            content_spacing: vec![],
            post_blank: None,
        }];
        let elem = Element::PlainList {
            kind: ListKind::Descriptive,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "- Term :: Definition here\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn horizontal_rule() {
        let elem = Element::HorizontalRule { dash_count: None };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "-----\n");
    }

    #[test]
    fn keyword_element() {
        let elem = Element::Keyword {
            key: "TITLE".into(),
            value: " My Document".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "#+TITLE: My Document\n");
    }

    #[test]
    fn drawer_element() {
        let elem = Element::Drawer {
            name: "LOGBOOK".into(),
            value: "- Note taken\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = ":LOGBOOK:\n- Note taken\n:END:\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn quote_block() {
        let elem = Element::QuoteBlock {
            elements: vec![Element::Paragraph {
                contents: vec![text("A wise saying.")],
            }],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_quote\nA wise saying.\n#+end_quote\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn footnote_reference_inline() {
        let fr = InlineContent::FootnoteReference {
            label: Some("1".into()),
            definition: None,
        };
        assert_eq!(inline_to_string(&[fr]), "[fn:1]");
    }

    #[test]
    fn footnote_reference_with_definition() {
        let fr = InlineContent::FootnoteReference {
            label: Some("note".into()),
            definition: Some(vec![text("inline def")]),
        };
        assert_eq!(inline_to_string(&[fr]), "[fn:note:inline def]");
    }

    #[test]
    fn export_snippet() {
        let es = InlineContent::ExportSnippet {
            backend: "html".into(),
            value: "<br/>".into(),
        };
        assert_eq!(inline_to_string(&[es]), "@@html:<br/>@@");
    }

    #[test]
    fn inline_src() {
        let is = InlineContent::InlineSrc {
            language: "python".into(),
            value: "1+1".into(),
        };
        assert_eq!(inline_to_string(&[is]), "src_python{1+1}");
    }

    #[test]
    fn macro_content() {
        let m = InlineContent::Macro {
            value: "date".into(),
        };
        assert_eq!(inline_to_string(&[m]), "{{{date}}}");
    }

    #[test]
    fn target_and_radio_target() {
        let t = InlineContent::Target {
            value: "my-target".into(),
        };
        let rt = InlineContent::RadioTarget {
            value: "radio".into(),
        };
        assert_eq!(inline_to_string(&[t]), "<<my-target>>");
        assert_eq!(inline_to_string(&[rt]), "<<<radio>>>");
    }

    #[test]
    fn superscript_and_subscript() {
        let sup = InlineContent::Superscript {
            contents: vec![text("2")],
            use_braces: true,
        };
        let sub = InlineContent::Subscript {
            contents: vec![text("i")],
            use_braces: true,
        };
        assert_eq!(inline_to_string(&[sup]), "^{2}");
        assert_eq!(inline_to_string(&[sub]), "_{i}");
    }

    #[test]
    fn superscript_complex() {
        let sup = InlineContent::Superscript {
            contents: vec![text("a + b")],
            use_braces: true,
        };
        assert_eq!(inline_to_string(&[sup]), "^{a + b}");
    }

    #[test]
    fn entity_rendering() {
        let e = InlineContent::Entity {
            name: "alpha".into(),
        };
        assert_eq!(inline_to_string(&[e]), "\\alpha");
    }

    #[test]
    fn dynamic_block() {
        let elem = Element::DynamicBlock {
            name: "clocktable".into(),
            parameters: Some(":maxlevel 2".into()),
            elements: vec![Element::Paragraph {
                contents: vec![text("content")],
            }],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+BEGIN: clocktable :maxlevel 2\ncontent\n#+END:\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn comment_element() {
        let elem = Element::Comment {
            value: "A comment line\nSecond line".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "# A comment line\n# Second line\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn fixed_width_element() {
        let elem = Element::FixedWidth {
            value: "fixed text".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), ": fixed text\n");
    }

    #[test]
    fn clock_element() {
        let elem = Element::Clock {
            value: "[2024-01-15 Mon 10:00]--[2024-01-15 Mon 11:30] =>  1:30".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(
            entry_to_org(&e),
            "CLOCK: [2024-01-15 Mon 10:00]--[2024-01-15 Mon 11:30] =>  1:30\n"
        );
    }

    #[test]
    fn footnote_definition_element() {
        let elem = Element::FootnoteDefinition {
            label: "1".into(),
            elements: vec![Element::Paragraph {
                contents: vec![text("Footnote text.")],
            }],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "[fn:1] Footnote text.\n");
    }

    #[test]
    fn full_heading_all_fields() {
        let h = Heading {
            level: 2,
            keyword: Some("TODO".into()),
            priority: Some("B".into()),
            title: vec![text("Complete task")],
            tags: vec!["project".into(), "review".into()],
            planning: Some(Planning {
                closed: Some("[2024-01-20 Sat 14:00]".into()),
                deadline: Some("<2024-01-18>".into()),
                scheduled: None,
            }),
            properties: vec![Property {
                key: "ID".into(),
                value: "task-42".into(),
            }],
            pre_body_blank: None,
            body_spacing: vec![],
            body: vec![Element::Paragraph {
                contents: vec![text("Description of the task.")],
            }],
            post_body_blank: None,
            children: vec![simple_heading(3, "Subtask")],
            post_blank: None,
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        let expected = "\
** TODO [#B] Complete task :project:review:
DEADLINE: <2024-01-18> CLOSED: [2024-01-20 Sat 14:00]
:PROPERTIES:
:ID:       task-42
:END:
Description of the task.
*** Subtask
";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn determinism() {
        let entries = vec![
            entry(EntryContent::Section {
                elements: vec![Element::Keyword {
                    key: "TITLE".into(),
                    value: " Test".into(),
                }],
                body_spacing: vec![],
            }),
            entry(EntryContent::Heading(Box::new(Heading {
                level: 1,
                keyword: Some("TODO".into()),
                priority: Some("A".into()),
                title: vec![
                    text("Buy "),
                    InlineContent::Bold {
                        contents: vec![text("groceries")],
                    },
                ],
                tags: vec!["errand".into()],
                planning: Some(Planning {
                    closed: None,
                    deadline: Some("<2024-03-01>".into()),
                    scheduled: None,
                }),
                properties: vec![Property {
                    key: "EFFORT".into(),
                    value: "1:00".into(),
                }],
                pre_body_blank: None,
                body_spacing: vec![],
                body: vec![Element::PlainList {
                    kind: ListKind::Unordered,
                    items: vec![
                        ListItem {
                            bullet: "-".into(),
                            checkbox: Some(CheckboxState::Unchecked),
                            counter_set: None,
                            tag: None,
                            contents: vec![Element::Paragraph {
                                contents: vec![text("Milk")],
                            }],
                            content_spacing: vec![],
                            post_blank: None,
                        },
                        ListItem {
                            bullet: "-".into(),
                            checkbox: Some(CheckboxState::Checked),
                            counter_set: None,
                            tag: None,
                            contents: vec![Element::Paragraph {
                                contents: vec![text("Eggs")],
                            }],
                            content_spacing: vec![],
                            post_blank: None,
                        },
                    ],
                }],
                post_body_blank: None,
                children: vec![],
                post_blank: None,
            }))),
        ];
        // Run twice and verify identical output.
        let out1 = entries_to_org(&entries);
        let out2 = entries_to_org(&entries);
        assert_eq!(out1, out2);
        // Verify structure
        assert!(out1.contains("#+TITLE: Test"));
        assert!(out1.contains("* TODO [#A] Buy *groceries* :errand:"));
        assert!(out1.contains("DEADLINE: <2024-03-01>"));
        assert!(out1.contains(":EFFORT:   1:00"));
        assert!(out1.contains("- [ ] Milk"));
        assert!(out1.contains("- [X] Eggs"));
        assert!(out1.ends_with('\n'));
        // No trailing whitespace on any line
        for line in out1.lines() {
            assert_eq!(line, line.trim_end(), "trailing whitespace: {:?}", line);
        }
    }

    #[test]
    fn example_block() {
        let elem = Element::ExampleBlock {
            value: "example line 1\nexample line 2\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_example\nexample line 1\nexample line 2\n#+end_example\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn center_block() {
        let elem = Element::CenterBlock {
            elements: vec![Element::Paragraph {
                contents: vec![text("Centered text")],
            }],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_center\nCentered text\n#+end_center\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn verse_block() {
        let elem = Element::VerseBlock {
            value: "Roses are red\nViolets are blue\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_verse\nRoses are red\nViolets are blue\n#+end_verse\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn special_block() {
        let elem = Element::SpecialBlock {
            name: "warning".into(),
            parameters: None,
            value: "Be careful!\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_warning\nBe careful!\n#+end_warning\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn export_block() {
        let elem = Element::ExportBlock {
            backend: "html".into(),
            value: "<div>content</div>\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_export html\n<div>content</div>\n#+end_export\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn comment_block() {
        let elem = Element::CommentBlock {
            value: "hidden comment\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "#+begin_comment\nhidden comment\n#+end_comment\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn latex_environment() {
        let elem = Element::LatexEnvironment {
            value: "\\begin{equation}\nx = 42\n\\end{equation}".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "\\begin{equation}\nx = 42\n\\end{equation}\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn diary_sexp() {
        let elem = Element::DiarySexp {
            value: "%%(diary-anniversary 6 14 1988)".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "%%(diary-anniversary 6 14 1988)\n");
    }

    #[test]
    fn raw_element() {
        let elem = Element::Raw {
            value: "some raw text\n".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "some raw text\n");
    }

    #[test]
    fn affiliated_keyword() {
        let elem = Element::AffiliatedKeyword {
            key: "NAME".into(),
            value: " my-table".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "#+NAME: my-table\n");
    }

    #[test]
    fn line_break_inline() {
        let contents = vec![text("first"), InlineContent::LineBreak, text("second")];
        assert_eq!(inline_to_string(&contents), "first\\\\\nsecond");
    }

    #[test]
    fn latex_fragment_inline() {
        let lf = InlineContent::LatexFragment {
            value: "$x^2$".into(),
        };
        assert_eq!(inline_to_string(&[lf]), "$x^2$");
    }

    #[test]
    fn statistics_cookie() {
        let sc = InlineContent::StatisticsCookie {
            value: "[2/5]".into(),
        };
        assert_eq!(inline_to_string(&[sc]), "[2/5]");
    }

    #[test]
    fn nested_inline_markup() {
        let content = InlineContent::Bold {
            contents: vec![
                text("bold with "),
                InlineContent::Italic {
                    contents: vec![text("italic")],
                },
            ],
        };
        assert_eq!(inline_to_string(&[content]), "*bold with /italic/*");
    }

    #[test]
    fn empty_section() {
        let e = entry(EntryContent::Section {
            elements: vec![],
            body_spacing: vec![],
        });
        // An empty section should produce just a trailing newline.
        assert_eq!(entry_to_org(&e), "\n");
    }

    #[test]
    fn ordered_list() {
        let items = vec![
            ListItem {
                bullet: "1.".into(),
                checkbox: None,
                counter_set: None,
                tag: None,
                contents: vec![Element::Paragraph {
                    contents: vec![text("First item")],
                }],
                content_spacing: vec![],
                post_blank: None,
            },
            ListItem {
                bullet: "2.".into(),
                checkbox: None,
                counter_set: Some("5".into()),
                tag: None,
                contents: vec![Element::Paragraph {
                    contents: vec![text("Second item")],
                }],
                content_spacing: vec![],
                post_blank: None,
            },
        ];
        let elem = Element::PlainList {
            kind: ListKind::Ordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let expected = "1. First item\n2. [@5] Second item\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn inline_babel() {
        let ib = InlineContent::InlineBabel {
            value: "call_my_func()".into(),
        };
        assert_eq!(inline_to_string(&[ib]), "call_my_func()");
    }

    #[test]
    fn timestamp_inline() {
        let ts = InlineContent::Timestamp {
            value: "<2024-01-15 Mon>".into(),
        };
        assert_eq!(inline_to_string(&[ts]), "<2024-01-15 Mon>");
    }

    #[test]
    fn empty_comment_value() {
        let elem = Element::Comment { value: "".into() };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "#\n");
    }

    #[test]
    fn empty_fixed_width_value() {
        let elem = Element::FixedWidth { value: "".into() };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), ":\n");
    }

    #[test]
    fn empty_footnote_definition() {
        let elem = Element::FootnoteDefinition {
            label: "1".into(),
            elements: vec![],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        // The code emits "[fn:label] " then "\n" for empty footnotes.
        assert_eq!(entry_to_org(&e), "[fn:1] \n");
    }

    #[test]
    fn empty_latex_environment() {
        let elem = Element::LatexEnvironment { value: "".into() };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "\n");
    }

    #[test]
    fn empty_raw_element() {
        let elem = Element::Raw { value: "".into() };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "\n");
    }

    #[test]
    fn list_item_with_no_contents() {
        let items = vec![ListItem {
            bullet: "-".into(),
            checkbox: None,
            counter_set: None,
            tag: None,
            contents: vec![],
            content_spacing: vec![],
            post_blank: None,
        }];
        let elem = Element::PlainList {
            kind: ListKind::Unordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "-\n");
    }

    #[test]
    fn list_item_first_element_not_paragraph() {
        let items = vec![ListItem {
            bullet: "-".into(),
            checkbox: None,
            counter_set: None,
            tag: None,
            contents: vec![Element::SrcBlock {
                language: "python".into(),
                parameters: None,
                value: "print(1)\n".into(),
            }],
            content_spacing: vec![],
            post_blank: None,
        }];
        let elem = Element::PlainList {
            kind: ListKind::Unordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let result = entry_to_org(&e);
        assert!(
            result.starts_with("-\n"),
            "expected bullet on its own line, got: {result:?}"
        );
        assert!(result.contains("#+begin_src python"));
        assert!(result.contains("print(1)"));
        assert!(result.contains("#+end_src"));
    }

    #[test]
    fn consecutive_light_elements_no_blank_line() {
        let elems = vec![
            Element::Keyword {
                key: "TITLE".into(),
                value: " My Document".into(),
            },
            Element::Keyword {
                key: "AUTHOR".into(),
                value: " John".into(),
            },
        ];
        let e = entry(EntryContent::Section {
            elements: elems,
            body_spacing: vec![],
        });
        let result = entry_to_org(&e);
        assert_eq!(result, "#+TITLE: My Document\n#+AUTHOR: John\n");
    }

    #[test]
    fn footnote_def_with_rest_elements() {
        let elem = Element::FootnoteDefinition {
            label: "2".into(),
            elements: vec![
                Element::Paragraph {
                    contents: vec![text("First paragraph.")],
                },
                Element::Paragraph {
                    contents: vec![text("Second paragraph.")],
                },
            ],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let result = entry_to_org(&e);
        assert!(
            result.starts_with("[fn:2] First paragraph."),
            "got: {result:?}"
        );
        assert!(result.contains("Second paragraph."));
    }

    #[test]
    fn subscript_without_braces() {
        let sub = InlineContent::Subscript {
            contents: vec![text("i")],
            use_braces: false,
        };
        assert_eq!(inline_to_string(&[sub]), "_i");
    }

    #[test]
    fn superscript_without_braces() {
        let sup = InlineContent::Superscript {
            contents: vec![text("2")],
            use_braces: false,
        };
        assert_eq!(inline_to_string(&[sup]), "^2");
    }

    #[test]
    fn partial_checkbox_in_list() {
        let items = vec![ListItem {
            bullet: "-".into(),
            checkbox: Some(CheckboxState::Partial),
            counter_set: None,
            tag: None,
            contents: vec![Element::Paragraph {
                contents: vec![text("Partial item")],
            }],
            content_spacing: vec![],
            post_blank: None,
        }];
        let elem = Element::PlainList {
            kind: ListKind::Unordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        assert_eq!(entry_to_org(&e), "- [-] Partial item\n");
    }

    #[test]
    fn footnote_def_non_paragraph_first_element() {
        let elem = Element::FootnoteDefinition {
            label: "3".into(),
            elements: vec![Element::SrcBlock {
                language: "elisp".into(),
                parameters: None,
                value: "(message \"hello\")\n".into(),
            }],
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
            body_spacing: vec![],
        });
        let result = entry_to_org(&e);
        assert!(result.starts_with("[fn:3]"), "got: {result:?}");
        assert!(result.contains("#+begin_src elisp"));
    }
}

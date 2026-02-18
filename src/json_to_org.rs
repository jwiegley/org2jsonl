//! Canonical pretty-printer: converts the JSON data model back into Org-mode text.
//!
//! This module is deterministic -- the same [`OrgEntry`] slice always produces
//! byte-identical output.  The output follows these canonical-form rules:
//!
//! * Single blank line between top-level headings
//! * No trailing whitespace on any line
//! * Property drawers immediately after the heading (planning line in between
//!   when present)
//! * UTF-8, LF line endings, file ends with exactly one newline

use crate::model::{
    CheckboxState, Element, EntryContent, Heading, InlineContent, ListItem, OrgEntry, Planning,
    Property, TableRow, TableRowKind,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Render a slice of [`OrgEntry`] values into canonical Org-mode text.
///
/// Entries are separated by a single blank line when at least one of the
/// adjacent entries is a heading.  The returned string always ends with
/// exactly one newline.
pub fn entries_to_org(entries: &[OrgEntry]) -> String {
    let mut buf = String::new();
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            // Blank line between entries
            buf.push('\n');
        }
        write_entry(&mut buf, entry);
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
        EntryContent::Section { elements } => {
            write_elements(buf, elements, 0, false);
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

    // --- Body elements ---
    if !heading.body.is_empty() {
        // Blank line between property drawer / planning and body content.
        if !heading.properties.is_empty() || heading.planning.is_some() {
            buf.push('\n');
        }
        write_elements(buf, &heading.body, 0, false);
    }

    // --- Child headings ---
    for child in &heading.children {
        // Blank line before each child heading
        buf.push('\n');
        write_heading(buf, child);
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

/// Render planning keywords.  Order: CLOSED, DEADLINE, SCHEDULED (standard
/// Org convention).
fn write_planning(buf: &mut String, planning: &Planning) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(ts) = &planning.closed {
        parts.push(format!("CLOSED: {ts}"));
    }
    if let Some(ts) = &planning.deadline {
        parts.push(format!("DEADLINE: {ts}"));
    }
    if let Some(ts) = &planning.scheduled {
        parts.push(format!("SCHEDULED: {ts}"));
    }
    if !parts.is_empty() {
        buf.push_str(&parts.join(" "));
        buf.push('\n');
    }
}

/// Render a `:PROPERTIES:` drawer.
fn write_properties(buf: &mut String, properties: &[Property]) {
    buf.push_str(":PROPERTIES:\n");
    for prop in properties {
        buf.push(':');
        buf.push_str(&prop.key);
        buf.push_str(": ");
        buf.push_str(&prop.value);
        buf.push('\n');
    }
    buf.push_str(":END:\n");
}

// ---------------------------------------------------------------------------
// Block-level elements
// ---------------------------------------------------------------------------

/// Write a sequence of elements, separated by blank lines.
///
/// When `indent_contents` is true every line of every element is indented by
/// `indent` spaces (used for elements nested inside special blocks).
fn write_elements(buf: &mut String, elements: &[Element], indent: usize, indent_contents: bool) {
    for (i, elem) in elements.iter().enumerate() {
        if i > 0 {
            buf.push('\n');
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
            write_block_value(buf, value, &prefix);
            buf.push_str(&prefix);
            buf.push_str(":END:\n");
        }
        Element::Table { rows } => {
            write_table(buf, rows, &prefix);
        }
        Element::HorizontalRule => {
            buf.push_str(&prefix);
            buf.push_str("-----\n");
        }
        Element::Keyword { key, value } => {
            buf.push_str(&prefix);
            buf.push_str("#+");
            buf.push_str(key);
            buf.push_str(": ");
            buf.push_str(value);
            buf.push('\n');
        }
        Element::Comment { value } => {
            for line in value.lines() {
                buf.push_str(&prefix);
                buf.push_str("# ");
                buf.push_str(line);
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
            buf.push_str(": ");
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
fn write_block_value(buf: &mut String, value: &str, prefix: &str) {
    if value.is_empty() {
        return;
    }
    for line in value.lines() {
        buf.push_str(prefix);
        buf.push_str(line);
        buf.push('\n');
    }
    // If the original value ended with a trailing newline, `lines()` will
    // have consumed it already (no extra empty element), so no double newline.
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
        write_list_item_first_element(buf, first);
        for elem in rest {
            // Blank line between sub-elements of a list item.
            buf.push('\n');
            write_element(buf, elem, body_indent);
        }
    } else {
        // Empty item -- just end the line.
        trim_trailing_whitespace(buf);
        buf.push('\n');
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
fn write_list_item_first_element(buf: &mut String, element: &Element) {
    match element {
        Element::Paragraph { contents } => {
            let text = inline_to_string(contents);
            let trimmed = text.trim_end();
            buf.push_str(trimmed);
            buf.push('\n');
        }
        _ => {
            // Non-paragraph first element: put it on the next line.
            trim_trailing_whitespace(buf);
            buf.push('\n');
            write_element(buf, element, 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// Render an Org table.
fn write_table(buf: &mut String, rows: &[TableRow], prefix: &str) {
    // First pass: determine column widths for alignment.
    let col_count = rows
        .iter()
        .filter_map(|r| match &r.kind {
            TableRowKind::Standard { cells } => Some(cells.len()),
            TableRowKind::Rule => None,
        })
        .max()
        .unwrap_or(0);

    let mut col_widths = vec![0usize; col_count];
    for row in rows {
        if let TableRowKind::Standard { cells } = &row.kind {
            for (i, cell) in cells.iter().enumerate() {
                let w = inline_to_string(cell).len();
                if i < col_widths.len() && w > col_widths[i] {
                    col_widths[i] = w;
                }
            }
        }
    }

    // Ensure minimum width of 1 for rule separators
    for w in &mut col_widths {
        if *w == 0 {
            *w = 1;
        }
    }

    // Second pass: render rows.
    for row in rows {
        buf.push_str(prefix);
        match &row.kind {
            TableRowKind::Standard { cells } => {
                buf.push('|');
                for (i, cell) in cells.iter().enumerate() {
                    let text = inline_to_string(cell);
                    let width = col_widths.get(i).copied().unwrap_or(text.len());
                    buf.push(' ');
                    buf.push_str(&text);
                    // Pad to column width
                    let padding = width.saturating_sub(text.len());
                    for _ in 0..padding {
                        buf.push(' ');
                    }
                    buf.push_str(" |");
                }
                // If there are fewer cells than col_count, that is fine --
                // Org allows ragged tables.
                buf.push('\n');
            }
            TableRowKind::Rule => {
                buf.push('|');
                for (i, &w) in col_widths.iter().enumerate() {
                    // +2 for the padding spaces on each side
                    for _ in 0..(w + 2) {
                        buf.push('-');
                    }
                    if i + 1 < col_widths.len() {
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
        InlineContent::Subscript { contents } => {
            // Always use brace form _{...} for round-trip safety.
            buf.push_str("_{");
            write_inline(buf, contents);
            buf.push('}');
        }
        InlineContent::Superscript { contents } => {
            // Always use brace form ^{...} for round-trip safety.
            buf.push_str("^{");
            write_inline(buf, contents);
            buf.push('}');
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

/// Make sure `buf` ends with exactly one `\n`.
fn ensure_final_newline(buf: &mut String) {
    // Remove trailing blank lines
    while buf.ends_with("\n\n") {
        buf.pop();
    }
    // Ensure at least one newline
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
            body: vec![],
            children: vec![],
        }
    }

    fn entry(content: EntryContent) -> OrgEntry {
        OrgEntry {
            schema_version: SCHEMA_VERSION,
            content,
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
        let expected = "* Planned\nDEADLINE: <2024-01-15> SCHEDULED: <2024-01-10>\n";
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
:ID: abc-123
:CUSTOM: val
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
        let expected = "* Parent\n\n** Child\n";
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
            },
            TableRow {
                kind: TableRowKind::Rule,
            },
            TableRow {
                kind: TableRowKind::Standard {
                    cells: vec![vec![text("Alice")], vec![text("30")]],
                },
            },
        ];
        let elem = Element::Table { rows };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
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
            },
            ListItem {
                bullet: "-".into(),
                checkbox: Some(CheckboxState::Checked),
                counter_set: None,
                tag: None,
                contents: vec![Element::Paragraph {
                    contents: vec![text("Second")],
                }],
            },
        ];
        let elem = Element::PlainList {
            kind: ListKind::Unordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
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
        }];
        let elem = Element::PlainList {
            kind: ListKind::Descriptive,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
        });
        let expected = "- Term :: Definition here\n";
        assert_eq!(entry_to_org(&e), expected);
    }

    #[test]
    fn horizontal_rule() {
        let elem = Element::HorizontalRule;
        let e = entry(EntryContent::Section {
            elements: vec![elem],
        });
        assert_eq!(entry_to_org(&e), "-----\n");
    }

    #[test]
    fn keyword_element() {
        let elem = Element::Keyword {
            key: "TITLE".into(),
            value: "My Document".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
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
        };
        let sub = InlineContent::Subscript {
            contents: vec![text("i")],
        };
        assert_eq!(inline_to_string(&[sup]), "^{2}");
        assert_eq!(inline_to_string(&[sub]), "_{i}");
    }

    #[test]
    fn superscript_complex() {
        let sup = InlineContent::Superscript {
            contents: vec![text("a + b")],
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
            body: vec![Element::Paragraph {
                contents: vec![text("Description of the task.")],
            }],
            children: vec![simple_heading(3, "Subtask")],
        };
        let e = entry(EntryContent::Heading(Box::new(h)));
        let expected = "\
** TODO [#B] Complete task :project:review:
CLOSED: [2024-01-20 Sat 14:00] DEADLINE: <2024-01-18>
:PROPERTIES:
:ID: task-42
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
                    value: "Test".into(),
                }],
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
                        },
                        ListItem {
                            bullet: "-".into(),
                            checkbox: Some(CheckboxState::Checked),
                            counter_set: None,
                            tag: None,
                            contents: vec![Element::Paragraph {
                                contents: vec![text("Eggs")],
                            }],
                        },
                    ],
                }],
                children: vec![],
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
        assert!(out1.contains(":EFFORT: 1:00"));
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
        });
        assert_eq!(entry_to_org(&e), "some raw text\n");
    }

    #[test]
    fn affiliated_keyword() {
        let elem = Element::AffiliatedKeyword {
            key: "NAME".into(),
            value: "my-table".into(),
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
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
        let e = entry(EntryContent::Section { elements: vec![] });
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
            },
            ListItem {
                bullet: "2.".into(),
                checkbox: None,
                counter_set: Some("5".into()),
                tag: None,
                contents: vec![Element::Paragraph {
                    contents: vec![text("Second item")],
                }],
            },
        ];
        let elem = Element::PlainList {
            kind: ListKind::Ordered,
            items,
        };
        let e = entry(EntryContent::Section {
            elements: vec![elem],
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
}

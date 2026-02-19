//! Converts an Org-mode file (parsed by the `orgize` crate) into the JSON
//! model defined in [`crate::model`].
//!
//! The single public entry point is [`org_to_entries`], which parses raw
//! Org-mode text and returns a `Vec<OrgEntry>` suitable for serialising
//! as JSONL.

use orgize::ast::{
    Bold, CenterBlock, Clock, Code, Comment, CommentBlock, Cookie, Drawer, DynBlock, Entity,
    ExampleBlock, ExportBlock, FixedWidth, FnDef, FnRef, Headline, InlineCall, InlineSrc, Italic,
    Keyword, LatexEnvironment, LatexFragment, LineBreak, Link, List, ListItem as OrgListItem,
    Macros, OrgTable, OrgTableCell, OrgTableRow, Paragraph, QuoteBlock, RadioTarget, Rule, Section,
    Snippet, SourceBlock, SpecialBlock, Strike, Subscript, Superscript, Target, Timestamp,
    Underline, Verbatim, VerseBlock,
};
use orgize::rowan::ast::AstNode;
use orgize::{Org, SyntaxKind};

use crate::model::{
    CheckboxState, Element, EntryContent, Heading, InlineContent, ListItem, ListKind, OrgEntry,
    Planning, Property, TableRow, TableRowKind,
};
use crate::SCHEMA_VERSION;

/// Parse raw Org-mode text and return a list of [`OrgEntry`] values.
///
/// Each top-level heading becomes a separate entry of type
/// [`EntryContent::Heading`]. Any content before the first heading (the
/// "zeroth section") becomes an [`EntryContent::Section`].
pub fn org_to_entries(input: &str) -> Vec<OrgEntry> {
    let org = Org::parse(input);
    let doc = org.document();
    let mut entries = Vec::new();
    let mut raw_texts: Vec<String> = Vec::new();

    // Zeroth section: content before the first heading.
    if let Some(section) = doc.section() {
        let raw = section.syntax().to_string();
        let mut elements = Vec::new();

        // Check for file-level property drawer in document syntax (before section)
        // The property drawer may be a direct child of the Document node
        for child in doc.syntax().children() {
            if child.kind() == SyntaxKind::PROPERTY_DRAWER {
                // Found a file-level property drawer
                elements.push(Element::Raw {
                    value: child.to_string(),
                });
                break; // Only one property drawer at file level
            } else if child.kind() == SyntaxKind::SECTION {
                // Once we hit the section, stop looking
                break;
            }
        }

        // Add section elements
        let (sec_elements, mut sec_spacing) = convert_section_elements(&section);
        // If we prepended a file-level property drawer, adjust spacing.
        // There's no blank line between :END: and the first keyword.
        if !elements.is_empty() && !sec_elements.is_empty() {
            sec_spacing.insert(0, false);
        }
        elements.extend(sec_elements);
        let body_spacing = sec_spacing;

        if !elements.is_empty() {
            entries.push(OrgEntry {
                schema_version: SCHEMA_VERSION,
                content: EntryContent::Section {
                    elements,
                    body_spacing,
                },
                post_blank: None,
            });
            raw_texts.push(raw);
        }
    }

    // Each top-level (level-1) heading becomes its own entry.
    for headline in doc.headlines() {
        let raw = headline.syntax().to_string();
        entries.push(OrgEntry {
            schema_version: SCHEMA_VERSION,
            content: EntryContent::Heading(Box::new(convert_headline(&headline, true))),
            post_blank: None,
        });
        raw_texts.push(raw);
    }

    // Calculate post_blank from trailing newlines in each top-level structure's
    // raw text.  In orgize's AST, a top-level headline's syntax text includes
    // all sub-headlines, so trailing newlines at the very end correspond to
    // blank lines between this entry and the next (or trailing EOF blanks for
    // the last entry).
    for (i, entry) in entries.iter_mut().enumerate() {
        if i < raw_texts.len() {
            let raw = &raw_texts[i];
            let trailing_newlines = raw.bytes().rev().take_while(|&b| b == b'\n').count();
            // Subtract 1 for the mandatory newline at end of the last line
            let mut blank_count = if trailing_newlines > 1 {
                (trailing_newlines - 1) as u32
            } else {
                0
            };
            // When the entry has children, the trailing newlines in the raw
            // text include the last child's post_body_blank (which is already
            // tracked separately on the child heading). Subtract it to avoid
            // double-counting.
            if let EntryContent::Heading(ref heading) = entry.content {
                if let Some(last_child) = heading.children.last() {
                    if let Some(child_pdb) = last_child.post_body_blank {
                        blank_count = blank_count.saturating_sub(child_pdb);
                    }
                }
            }
            // Set post_blank: always set it explicitly so the writer doesn't
            // fall back to the default of 1 blank line when the actual
            // trailing blanks were already accounted for by child headings.
            entry.post_blank = Some(blank_count);
        }
    }

    entries
}

// ---------------------------------------------------------------------------
// Headline conversion
// ---------------------------------------------------------------------------

fn convert_headline(hl: &Headline, is_entry_level: bool) -> Heading {
    let level = hl.level() as u32;
    let keyword = hl.todo_keyword().map(|t| t.to_string());
    let priority = hl.priority().map(|t| t.to_string());
    let title = convert_title_objects(hl);
    let tags: Vec<String> = hl.tags().map(|t| t.to_string()).collect();

    let planning = hl.planning().map(|p| convert_planning(&p));
    let properties = hl.properties().map(convert_properties).unwrap_or_default();

    // Detect pre_body_blank from two possible sources:
    //
    // 1. Trailing BLANK_LINE tokens inside the PROPERTY_DRAWER (orgize absorbs
    //    blank lines after :END: into the property drawer node).
    // 2. Leading empty paragraphs in the section (orgize wraps blank lines
    //    between heading and body in PARAGRAPH nodes containing only a
    //    BLANK_LINE token).
    let mut pre_blank_count = 0u32;

    // Source 1: trailing BLANK_LINE tokens in the property drawer
    for child in hl.syntax().children() {
        if child.kind() == SyntaxKind::PROPERTY_DRAWER {
            for tok in child.children_with_tokens().collect::<Vec<_>>().into_iter().rev() {
                if let orgize::rowan::NodeOrToken::Token(t) = tok {
                    if t.kind() == SyntaxKind::BLANK_LINE {
                        pre_blank_count += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            break;
        }
    }

    let (section_leading_blanks, section_trailing_blanks, body, body_spacing) =
        if let Some(section) = hl.section()
    {
        let section_children: Vec<_> = section.syntax().children().collect();

        // Source 2: leading empty paragraphs in the section
        let mut leading = 0u32;
        for child in &section_children {
            if is_blank_paragraph(child) {
                leading += 1;
            } else {
                break;
            }
        }

        // Trailing blank lines in the section come from two sources:
        //
        // A) Separate empty PARAGRAPH nodes (containing only BLANK_LINE
        //    tokens) at the end of the section.
        // B) Trailing BLANK_LINE tokens inside the last real PARAGRAPH
        //    (orgize sometimes folds blank lines into the preceding
        //    paragraph rather than creating a separate node).
        let mut trailing = 0u32;

        // Source A: trailing empty paragraphs
        for child in section_children.iter().rev() {
            if is_blank_paragraph(child) {
                trailing += 1;
            } else {
                break;
            }
        }

        // If the section only has blank paragraphs, they are ALL leading
        // (pre_body_blank); don't double-count them as trailing.
        // This check only applies to Source A (separate blank paragraph
        // nodes), not Source B below.
        let total = section_children.len() as u32;
        if leading + trailing >= total {
            trailing = 0;
        }

        // Source B: trailing BLANK_LINE tokens in the last non-blank child.
        // These are tokens INSIDE a real element (paragraph or list),
        // representing a different blank line than the leading blank paragraph
        // nodes counted above.
        if trailing == 0 {
            if let Some(last) = section_children.last() {
                if !is_blank_paragraph(last) {
                    trailing = count_trailing_blank_lines(last);
                }
            }
        }

        let (elements, body_spacing) = convert_section_elements(&section);
        (leading, trailing, elements, body_spacing)
    } else {
        (0, 0, vec![], vec![])
    };

    let children: Vec<Heading> = hl
        .headlines()
        .map(|child| convert_headline(&child, false))
        .collect();

    // Only treat the blank lines as pre_body_blank when there is actual
    // body content or child headings after them.  When both body and
    // children are empty, the blanks are trailing inter-entry spacing
    // already captured by the entry-level post_blank calculation.
    let pre_body_blank = if !body.is_empty() || !children.is_empty() {
        pre_blank_count += section_leading_blanks;
        if pre_blank_count > 0 {
            Some(pre_blank_count)
        } else {
            None
        }
    } else {
        None
    };

    // Trailing blank lines in the section represent spacing after the
    // body content (before the first child heading or next sibling).
    //
    // For entry-level (top-level) headings WITHOUT children, trailing
    // blanks are inter-entry spacing already captured by the entry-level
    // post_blank — don't double-count them.  For all other cases (child
    // headings, or entry-level headings with children), track them.
    let post_body_blank = if section_trailing_blanks > 0
        && (!is_entry_level || !children.is_empty())
    {
        Some(section_trailing_blanks)
    } else {
        None
    };

    Heading {
        level,
        keyword,
        priority,
        title,
        tags,
        planning,
        properties,
        pre_body_blank,
        body,
        body_spacing,
        post_body_blank,
        children,
    }
}

/// Extract inline objects from a headline's title.
fn convert_title_objects(hl: &Headline) -> Vec<InlineContent> {
    let mut result = Vec::new();
    for elem in hl.title() {
        collect_inline_from_element(&elem, &mut result);
    }
    result
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

fn convert_planning(p: &orgize::ast::Planning) -> Planning {
    Planning {
        closed: p.closed().map(|t| t.raw()),
        deadline: p.deadline().map(|t| t.raw()),
        scheduled: p.scheduled().map(|t| t.raw()),
    }
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

fn convert_properties(pd: orgize::ast::PropertyDrawer) -> Vec<Property> {
    pd.iter()
        .map(|(k, v)| Property {
            key: k.to_string(),
            value: v.to_string(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Section -> Vec<Element>
// ---------------------------------------------------------------------------

/// Check if a syntax node is a PARAGRAPH that contains only BLANK_LINE tokens
/// (i.e., an "empty paragraph" that represents a blank line in the source).
fn is_blank_paragraph(node: &orgize::SyntaxNode) -> bool {
    node.kind() == SyntaxKind::PARAGRAPH
        && node.children_with_tokens().all(|c| {
            matches!(
                c,
                orgize::rowan::NodeOrToken::Token(ref t)
                    if t.kind() == SyntaxKind::BLANK_LINE
            )
        })
}

/// Convert the children of a `Section` node into our `Element` model,
/// along with inter-element spacing information.
///
/// Returns (elements, body_spacing) where body_spacing[i] indicates whether
/// there was a blank line between elements[i] and elements[i+1].
fn convert_section_elements(section: &Section) -> (Vec<Element>, Vec<bool>) {
    let syntax = section.syntax();
    let mut elements = Vec::new();
    let mut spacing = Vec::new();
    let mut saw_blank = false;

    for child in syntax.children() {
        // Check if this is a blank paragraph (blank line marker)
        if is_blank_paragraph(&child) {
            saw_blank = true;
            continue;
        }
        // Also check for trailing BLANK_LINE tokens in previous real element
        // (orgize sometimes folds blank lines into the preceding paragraph)

        if let Some(el) = convert_block_element(&child) {
            // Filter out empty paragraphs (these arise from blank lines
            // between a heading and its children, and are not meaningful).
            if let Element::Paragraph { ref contents } = el {
                if contents.is_empty()
                    || contents.iter().all(|c| match c {
                        InlineContent::Text { value } => value.trim().is_empty(),
                        _ => false,
                    })
                {
                    saw_blank = true;
                    continue;
                }
            }
            if !elements.is_empty() {
                spacing.push(saw_blank);
            }
            elements.push(el);
            // Check if this element has trailing blank lines that should
            // be attributed as spacing before the next element.
            //
            // For PARAGRAPH nodes: trailing BLANK_LINE tokens indicate
            // blank lines after the paragraph.
            //
            // For LIST nodes: orgize folds trailing blank lines into the
            // last LIST_ITEM's raw text, so we check the raw text instead.
            saw_blank = if child.kind() == SyntaxKind::LIST {
                let raw = child.to_string();
                raw.ends_with("\n\n")
            } else {
                count_trailing_blank_lines(&child) > 0
            };
        } else {
            // Skipped node (planning, property drawer, etc.) - preserve blank state
        }
    }

    (elements, spacing)
}

/// Convert a single block-level syntax node into an [`Element`].
///
/// Returns `None` for nodes we intentionally skip (blank lines,
/// property drawers inside sections that orgize may produce, planning
/// nodes that are handled separately, etc.).
fn convert_block_element(node: &orgize::SyntaxNode) -> Option<Element> {
    let kind = node.kind();

    // Paragraph
    if let Some(para) = Paragraph::cast(node.clone()) {
        return Some(convert_paragraph(&para));
    }

    // Plain list
    if let Some(list) = List::cast(node.clone()) {
        return Some(convert_list(&list));
    }

    // Source block
    if let Some(sb) = SourceBlock::cast(node.clone()) {
        return Some(convert_source_block(&sb));
    }

    // Example block
    if let Some(_eb) = ExampleBlock::cast(node.clone()) {
        return Some(Element::ExampleBlock {
            value: block_content_text(node),
        });
    }

    // Export block
    if let Some(eb) = ExportBlock::cast(node.clone()) {
        return Some(Element::ExportBlock {
            backend: eb.ty().map(|t| t.to_string()).unwrap_or_default(),
            value: eb.value(),
        });
    }

    // Comment block
    if let Some(_cb) = CommentBlock::cast(node.clone()) {
        return Some(Element::CommentBlock {
            value: block_content_text(node),
        });
    }

    // Quote block
    if let Some(qb) = QuoteBlock::cast(node.clone()) {
        return Some(convert_quote_block(&qb));
    }

    // Center block
    if let Some(cb) = CenterBlock::cast(node.clone()) {
        return Some(convert_center_block(&cb));
    }

    // Verse block
    if let Some(_vb) = VerseBlock::cast(node.clone()) {
        return Some(Element::VerseBlock {
            value: block_content_text(node),
        });
    }

    // Special block
    if let Some(sb) = SpecialBlock::cast(node.clone()) {
        return Some(convert_special_block(&sb));
    }

    // Drawer
    if let Some(drawer) = Drawer::cast(node.clone()) {
        return Some(Element::Drawer {
            name: drawer.name().to_string(),
            value: drawer.content_raw(),
        });
    }

    // Table (org table)
    if let Some(table) = OrgTable::cast(node.clone()) {
        return Some(convert_table(&table));
    }

    // Horizontal rule
    if Rule::cast(node.clone()).is_some() {
        return Some(Element::HorizontalRule);
    }

    // Keyword
    if let Some(kw) = Keyword::cast(node.clone()) {
        return Some(Element::Keyword {
            key: kw.key().to_string(),
            value: kw.value().to_string().trim().to_string(),
        });
    }

    // Comment
    if let Some(comment) = Comment::cast(node.clone()) {
        return Some(Element::Comment {
            value: comment.value(),
        });
    }

    // Fixed width
    if let Some(fw) = FixedWidth::cast(node.clone()) {
        return Some(Element::FixedWidth { value: fw.value() });
    }

    // Clock
    if let Some(clock) = Clock::cast(node.clone()) {
        return Some(Element::Clock {
            value: clock.raw().trim().to_string(),
        });
    }

    // Footnote definition
    if let Some(fndef) = FnDef::cast(node.clone()) {
        return Some(convert_fn_def(&fndef));
    }

    // Latex environment
    if let Some(latex) = LatexEnvironment::cast(node.clone()) {
        return Some(Element::LatexEnvironment { value: latex.raw() });
    }

    // Dynamic block
    if let Some(dyn_block) = DynBlock::cast(node.clone()) {
        return Some(convert_dyn_block(&dyn_block));
    }

    // Affiliated keyword (we emit it as a keyword-like element)
    if let Some(ak) = orgize::ast::AffiliatedKeyword::cast(node.clone()) {
        return Some(Element::AffiliatedKeyword {
            key: ak.key().to_string(),
            value: ak.value().map(|v| v.to_string()).unwrap_or_default(),
        });
    }

    // Property drawer appearing in zeroth section (file-level properties)
    if kind == SyntaxKind::PROPERTY_DRAWER {
        // File-level property drawers should be preserved as raw text
        return Some(Element::Raw {
            value: node.to_string(),
        });
    }

    // Skip blank lines, headlines (handled separately), and other structural
    // nodes that are not meaningful as standalone elements.
    if kind == SyntaxKind::BLANK_LINE
        || kind == SyntaxKind::HEADLINE
        || kind == SyntaxKind::PLANNING
    {
        return None;
    }

    // Fallback: emit raw text for anything we do not explicitly handle.
    let text = node.to_string();
    if text.trim().is_empty() {
        return None;
    }
    Some(Element::Raw { value: text })
}

// ---------------------------------------------------------------------------
// Paragraph
// ---------------------------------------------------------------------------

fn convert_paragraph(para: &Paragraph) -> Element {
    let contents = convert_inline_children(para.syntax());
    Element::Paragraph { contents }
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

fn convert_list(list: &List) -> Element {
    let kind = if list.is_descriptive() {
        ListKind::Descriptive
    } else if list.is_ordered() {
        ListKind::Ordered
    } else {
        ListKind::Unordered
    };

    // Convert items and detect post_blank from trailing newlines in raw text.
    //
    // In orgize's AST, blank lines between list items are included as
    // trailing newlines in the preceding LIST_ITEM's raw text (often buried
    // inside the last nested descendant). We count trailing '\n' characters
    // and subtract 1 (the mandatory line-ending newline).
    //
    // For the LAST item in a list, we always set post_blank = 0 because any
    // trailing blank lines belong to the parent level (the parent list item
    // or the element-level spacing), not to this item.
    let item_nodes: Vec<_> = list.items().collect();
    let item_count = item_nodes.len();
    let items: Vec<ListItem> = item_nodes
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let mut li = convert_list_item(item);
            let is_last = idx + 1 == item_count;
            if !is_last {
                let text = item.syntax().to_string();
                let trailing_newlines = text.bytes().rev().take_while(|&b| b == b'\n').count();
                let blank_count = if trailing_newlines > 1 {
                    (trailing_newlines - 1) as u32
                } else {
                    0
                };
                if blank_count > 0 {
                    li.post_blank = Some(blank_count);
                }
            }
            li
        })
        .collect();

    Element::PlainList { kind, items }
}

fn convert_list_item(item: &OrgListItem) -> ListItem {
    let bullet = item.bullet().to_string();

    let checkbox = item.checkbox().map(|cb| {
        let s: &str = &cb;
        match s.trim() {
            "X" => CheckboxState::Checked,
            "-" => CheckboxState::Partial,
            _ => CheckboxState::Unchecked,
        }
    });

    let counter_set = item.counter().map(|c| c.to_string());

    let tag: Option<Vec<InlineContent>> = {
        let tag_elems: Vec<_> = item.tag().collect();
        if tag_elems.is_empty() {
            None
        } else {
            let mut inlines = Vec::new();
            for elem in &tag_elems {
                collect_inline_from_element(elem, &mut inlines);
            }
            // Trim trailing whitespace from the last text element of the tag
            // to avoid compounding spaces around ` :: ` on round-trips.
            if let Some(InlineContent::Text { value }) = inlines.last_mut() {
                *value = value.trim_end().to_string();
            }
            if inlines.is_empty() {
                None
            } else {
                Some(inlines)
            }
        }
    };

    // List item content lives in a LIST_ITEM_CONTENT child node, or
    // directly in the item's children after the bullet/checkbox/tag.
    let (contents, has_blank_lines) = convert_list_item_contents(item);

    ListItem {
        bullet,
        checkbox,
        counter_set,
        tag,
        contents,
        has_blank_lines,
        post_blank: None,
    }
}

/// Count trailing BLANK_LINE tokens inside a syntax node (typically a paragraph).
fn count_trailing_blank_lines(node: &orgize::SyntaxNode) -> u32 {
    let mut count = 0u32;
    for tok in node.children_with_tokens().collect::<Vec<_>>().into_iter().rev() {
        if let orgize::rowan::NodeOrToken::Token(t) = tok {
            if t.kind() == SyntaxKind::BLANK_LINE {
                count += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    count
}

/// Extract the block-level contents of a list item.
///
/// When a paragraph inside a list item has trailing BLANK_LINE tokens
/// (blank lines between the paragraph and a nested sub-list), we detect
/// this as a "loose" list item and set `has_blank_lines` on the ListItem.
fn convert_list_item_contents(item: &OrgListItem) -> (Vec<Element>, bool) {
    let mut elements = Vec::new();
    let mut has_blank_lines = false;
    for child in item.syntax().children() {
        let kind = child.kind();
        // Skip structural tokens that are not content.
        if kind == SyntaxKind::LIST_ITEM_INDENT
            || kind == SyntaxKind::LIST_ITEM_BULLET
            || kind == SyntaxKind::LIST_ITEM_COUNTER
            || kind == SyntaxKind::LIST_ITEM_CHECK_BOX
            || kind == SyntaxKind::LIST_ITEM_TAG
        {
            continue;
        }
        // LIST_ITEM_CONTENT wraps the actual content children.
        if kind == SyntaxKind::LIST_ITEM_CONTENT {
            let grandchildren: Vec<_> = child.children().collect();
            for (gi, grandchild) in grandchildren.iter().enumerate() {
                if let Some(el) = convert_block_element(grandchild) {
                    elements.push(strip_list_item_indentation(el));
                }
                // Check for trailing BLANK_LINE tokens in this child
                // (e.g., a paragraph with blank lines before a nested sub-list).
                if gi + 1 < grandchildren.len()
                    && count_trailing_blank_lines(grandchild) > 0
                {
                    has_blank_lines = true;
                }
            }
            continue;
        }
        if let Some(el) = convert_block_element(&child) {
            elements.push(strip_list_item_indentation(el));
        }
    }
    (elements, has_blank_lines)
}

/// Strip leading whitespace from paragraph text within list items.
///
/// Org-mode indents continuation paragraphs under list items, but we
/// handle indentation in the writer. Keeping the source indentation
/// causes it to compound on each round-trip.
fn strip_list_item_indentation(el: Element) -> Element {
    match el {
        Element::Paragraph { contents } => {
            let contents = strip_leading_whitespace(contents);
            Element::Paragraph { contents }
        }
        other => other,
    }
}

/// Strip leading whitespace from the first Text element in a list of
/// inline content, and from Text elements that follow LineBreak elements.
fn strip_leading_whitespace(mut contents: Vec<InlineContent>) -> Vec<InlineContent> {
    // Strip leading whitespace from first element
    if let Some(InlineContent::Text { value }) = contents.first_mut() {
        *value = value.trim_start().to_string();
        if value.is_empty() {
            contents.remove(0);
        }
    }

    // Strip leading whitespace from text elements that follow LineBreak
    let mut i = 0;
    while i < contents.len() {
        if matches!(contents[i], InlineContent::LineBreak) {
            // Check if next element is a Text element
            if i + 1 < contents.len() {
                if let InlineContent::Text { value } = &mut contents[i + 1] {
                    *value = value.trim_start().to_string();
                    if value.is_empty() {
                        contents.remove(i + 1);
                        continue; // Don't increment i
                    }
                }
            }
        }
        i += 1;
    }

    // Also dedent multi-line text elements
    for item in &mut contents {
        if let InlineContent::Text { value } = item {
            *value = dedent_text(value);
        }
    }
    contents
}

/// Dedent continuation lines in multi-line text.
fn dedent_text(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() <= 1 {
        return text.to_string();
    }

    // Find minimum indentation of non-empty continuation lines (excluding first line)
    let min_indent = lines[1..]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    if min_indent == 0 {
        return text.to_string();
    }

    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        if i == 0 {
            result.push_str(line);
        } else if line.trim().is_empty() {
            // Keep empty lines as-is (already added just '\n')
        } else if line.len() >= min_indent {
            result.push_str(&line[min_indent..]);
        } else {
            result.push_str(line);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Source block
// ---------------------------------------------------------------------------

fn convert_source_block(sb: &SourceBlock) -> Element {
    let language = sb.language().map(|t| t.to_string()).unwrap_or_default();
    let parameters = sb.parameters().map(|t| t.to_string());
    let value = sb.value();

    Element::SrcBlock {
        language,
        parameters,
        value,
    }
}

// ---------------------------------------------------------------------------
// Quote block / Center block
// ---------------------------------------------------------------------------

fn convert_quote_block(qb: &QuoteBlock) -> Element {
    let elements = convert_block_inner_elements(qb.syntax());
    Element::QuoteBlock { elements }
}

fn convert_center_block(cb: &CenterBlock) -> Element {
    let elements = convert_block_inner_elements(cb.syntax());
    Element::CenterBlock { elements }
}

/// For greater blocks (quote, center, special, dyn) that contain parsed
/// elements, extract the child elements between BLOCK_BEGIN and BLOCK_END.
fn convert_block_inner_elements(syntax: &orgize::SyntaxNode) -> Vec<Element> {
    let mut elements = Vec::new();
    for child in syntax.children() {
        let kind = child.kind();
        // Skip the begin/end markers and structural nodes.
        if kind == SyntaxKind::BLOCK_BEGIN
            || kind == SyntaxKind::BLOCK_END
            || kind == SyntaxKind::BLOCK_CONTENT
            || kind == SyntaxKind::DYN_BLOCK_BEGIN
            || kind == SyntaxKind::DYN_BLOCK_END
            || kind == SyntaxKind::AFFILIATED_KEYWORD
        {
            // For BLOCK_CONTENT that wraps parsed children, recurse into it.
            if kind == SyntaxKind::BLOCK_CONTENT {
                for grandchild in child.children() {
                    if let Some(el) = convert_block_element(&grandchild) {
                        elements.push(el);
                    }
                }
            }
            continue;
        }
        if let Some(el) = convert_block_element(&child) {
            elements.push(el);
        }
    }
    elements
}

// ---------------------------------------------------------------------------
// Special block
// ---------------------------------------------------------------------------

fn convert_special_block(sb: &SpecialBlock) -> Element {
    // The name is the first TEXT token inside BLOCK_BEGIN.
    let name = sb
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::BLOCK_BEGIN)
        .and_then(|begin| {
            begin
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == SyntaxKind::TEXT)
                .map(|t| t.to_string())
        })
        .unwrap_or_default();

    // Parameters are the second TEXT token in BLOCK_BEGIN (after the name).
    let parameters = sb
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::BLOCK_BEGIN)
        .and_then(|begin| {
            begin
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::TEXT)
                .nth(1)
                .and_then(|t| {
                    let trimmed = t.to_string().trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                })
        });

    let value = block_content_text(sb.syntax());

    Element::SpecialBlock {
        name,
        parameters,
        value,
    }
}

// ---------------------------------------------------------------------------
// Dynamic block
// ---------------------------------------------------------------------------

fn convert_dyn_block(db: &DynBlock) -> Element {
    // Name is the second TEXT token in DYN_BLOCK_BEGIN
    // (first is "#+BEGIN:", second is the name, third is parameters).
    let (name, parameters) = db
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::DYN_BLOCK_BEGIN)
        .map(|begin| {
            let texts: Vec<String> = begin
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::TEXT)
                .map(|t| t.to_string())
                .collect();
            // texts[0] = "#+BEGIN:", texts[1] = name, texts[2..] = parameters
            let name = texts.get(1).cloned().unwrap_or_default();
            let params = texts.get(2).and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });
            (name, params)
        })
        .unwrap_or_default();

    let elements = convert_block_inner_elements(db.syntax());

    Element::DynamicBlock {
        name,
        parameters,
        elements,
    }
}

// ---------------------------------------------------------------------------
// Table
// ---------------------------------------------------------------------------

fn convert_table(table: &OrgTable) -> Element {
    let rows: Vec<TableRow> = table
        .syntax()
        .children()
        .filter_map(OrgTableRow::cast)
        .map(|row| convert_table_row(&row))
        .collect();

    Element::Table { rows }
}

fn convert_table_row(row: &OrgTableRow) -> TableRow {
    if row.is_rule() {
        return TableRow {
            kind: TableRowKind::Rule,
        };
    }

    let cells: Vec<Vec<InlineContent>> = row
        .syntax()
        .children()
        .filter_map(OrgTableCell::cast)
        .map(|cell| convert_inline_children(cell.syntax()))
        .collect();

    TableRow {
        kind: TableRowKind::Standard { cells },
    }
}

// ---------------------------------------------------------------------------
// Footnote definition
// ---------------------------------------------------------------------------

fn convert_fn_def(fndef: &FnDef) -> Element {
    // The label is the TEXT token that follows the "fn" + ":" tokens.
    // From the syntax tree: L_BRACKET TEXT("fn") COLON TEXT(label) R_BRACKET TEXT(content)
    let mut texts = fndef
        .syntax()
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::TEXT);

    let _fn_text = texts.next(); // "fn"
    let label = texts.next().map(|t| t.to_string()).unwrap_or_default();

    // The body is everything after the closing bracket, which is the
    // remaining TEXT tokens.  For now we treat it as a single paragraph.
    let body_text: String = texts.map(|t| t.to_string()).collect();
    let body_text = body_text.trim().to_string();

    let elements = if body_text.is_empty() {
        vec![]
    } else {
        vec![Element::Paragraph {
            contents: vec![InlineContent::Text { value: body_text }],
        }]
    };

    Element::FootnoteDefinition { label, elements }
}

// ---------------------------------------------------------------------------
// Block content text (for lesser blocks that store raw text)
// ---------------------------------------------------------------------------

/// Extract the raw text content from a block's BLOCK_CONTENT child.
fn block_content_text(syntax: &orgize::SyntaxNode) -> String {
    syntax
        .children()
        .find(|n| n.kind() == SyntaxKind::BLOCK_CONTENT)
        .map(|content| {
            content
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::TEXT)
                .fold(String::new(), |acc, t| acc + t.text())
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Inline / object conversion
// ---------------------------------------------------------------------------

/// Convert all child nodes/tokens of a syntax node into inline content.
///
/// This is used for paragraphs, table cells, emphasis contents, etc.
fn convert_inline_children(syntax: &orgize::SyntaxNode) -> Vec<InlineContent> {
    let mut result = Vec::new();
    for elem in syntax.children_with_tokens() {
        collect_inline_from_element(&elem, &mut result);
    }
    result
}

/// Collect inline content from a single `SyntaxElement` (node or token).
fn collect_inline_from_element(elem: &orgize::SyntaxElement, out: &mut Vec<InlineContent>) {
    match elem {
        orgize::rowan::NodeOrToken::Token(token) => {
            let kind = token.kind();
            if kind == SyntaxKind::TEXT {
                let text = token.text().to_string();
                if !text.is_empty() {
                    out.push(InlineContent::Text { value: text });
                }
            }
            // Other tokens (brackets, whitespace, etc.) that appear between
            // objects are generally ignorable for our model.  However,
            // NEW_LINE inside paragraphs is significant whitespace.
            else if kind == SyntaxKind::NEW_LINE {
                out.push(InlineContent::Text {
                    value: "\n".to_string(),
                });
            } else if kind == SyntaxKind::WHITESPACE {
                out.push(InlineContent::Text {
                    value: token.text().to_string(),
                });
            }
        }
        orgize::rowan::NodeOrToken::Node(node) => {
            if let Some(inline) = convert_inline_node(node) {
                out.push(inline);
            }
        }
    }
}

/// Convert a syntax node that represents an inline object into
/// [`InlineContent`].
fn convert_inline_node(node: &orgize::SyntaxNode) -> Option<InlineContent> {
    let kind = node.kind();

    // Bold
    if let Some(bold) = Bold::cast(node.clone()) {
        let contents = convert_emphasis_children(bold.syntax());
        return Some(InlineContent::Bold { contents });
    }

    // Italic
    if let Some(italic) = Italic::cast(node.clone()) {
        let contents = convert_emphasis_children(italic.syntax());
        return Some(InlineContent::Italic { contents });
    }

    // Underline
    if let Some(underline) = Underline::cast(node.clone()) {
        let contents = convert_emphasis_children(underline.syntax());
        return Some(InlineContent::Underline { contents });
    }

    // Strike-through
    if let Some(strike) = Strike::cast(node.clone()) {
        let contents = convert_emphasis_children(strike.syntax());
        return Some(InlineContent::StrikeThrough { contents });
    }

    // Code (~code~)
    if let Some(code) = Code::cast(node.clone()) {
        let value = code
            .text()
            .map(|t| t.to_string())
            .unwrap_or_else(|| extract_emphasis_text(code.syntax()));
        return Some(InlineContent::Code { value });
    }

    // Verbatim (=verbatim=)
    if let Some(verb) = Verbatim::cast(node.clone()) {
        let value = extract_emphasis_text(verb.syntax());
        return Some(InlineContent::Verbatim { value });
    }

    // Link
    if let Some(link) = Link::cast(node.clone()) {
        return Some(convert_link(&link));
    }

    // Timestamp
    if let Some(ts) = Timestamp::cast(node.clone()) {
        return Some(InlineContent::Timestamp { value: ts.raw() });
    }

    // Footnote reference
    if let Some(fnref) = FnRef::cast(node.clone()) {
        return Some(convert_fn_ref(&fnref));
    }

    // Line break
    if LineBreak::cast(node.clone()).is_some() {
        return Some(InlineContent::LineBreak);
    }

    // Entity
    if let Some(entity) = Entity::cast(node.clone()) {
        return Some(InlineContent::Entity {
            name: entity.name().to_string(),
        });
    }

    // LaTeX fragment
    if let Some(latex) = LatexFragment::cast(node.clone()) {
        return Some(InlineContent::LatexFragment { value: latex.raw() });
    }

    // Export snippet
    if let Some(snippet) = Snippet::cast(node.clone()) {
        return Some(InlineContent::ExportSnippet {
            backend: snippet.backend().to_string(),
            value: snippet.value().to_string(),
        });
    }

    // Inline babel call
    if let Some(call) = InlineCall::cast(node.clone()) {
        return Some(InlineContent::InlineBabel { value: call.raw() });
    }

    // Inline source
    if let Some(src) = InlineSrc::cast(node.clone()) {
        return Some(InlineContent::InlineSrc {
            language: src.language().to_string(),
            value: src.value().to_string(),
        });
    }

    // Macro
    if let Some(macros) = Macros::cast(node.clone()) {
        let raw = macros.raw();
        let inner = raw
            .strip_prefix("{{{")
            .and_then(|s| s.strip_suffix("}}}"))
            .unwrap_or(&raw);
        return Some(InlineContent::Macro {
            value: inner.to_string(),
        });
    }

    // Target
    if let Some(target) = Target::cast(node.clone()) {
        // Target raw includes the <<< >>> markers; extract the inner text.
        let raw = target.raw();
        let inner = raw
            .strip_prefix("<<")
            .and_then(|s| s.strip_suffix(">>"))
            .unwrap_or(&raw);
        return Some(InlineContent::Target {
            value: inner.to_string(),
        });
    }

    // Radio target
    if let Some(radio) = RadioTarget::cast(node.clone()) {
        let raw = radio.raw();
        let inner = raw
            .strip_prefix("<<<")
            .and_then(|s| s.strip_suffix(">>>"))
            .unwrap_or(&raw);
        return Some(InlineContent::RadioTarget {
            value: inner.to_string(),
        });
    }

    // Statistics cookie
    if let Some(cookie) = Cookie::cast(node.clone()) {
        return Some(InlineContent::StatisticsCookie {
            value: cookie.raw(),
        });
    }

    // Subscript
    if let Some(sub) = Subscript::cast(node.clone()) {
        let contents = convert_emphasis_children(sub.syntax());
        return Some(InlineContent::Subscript { contents });
    }

    // Superscript
    if let Some(sup) = Superscript::cast(node.clone()) {
        let contents = convert_emphasis_children(sup.syntax());
        return Some(InlineContent::Superscript { contents });
    }

    // For any inline node kind we do not handle, fall back to raw text.
    if kind.is_object() {
        return Some(InlineContent::Text {
            value: node.to_string(),
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Emphasis helpers
// ---------------------------------------------------------------------------

/// Convert the children of an emphasis node (bold, italic, etc.) into
/// inline content, skipping the opening and closing marker tokens.
fn convert_emphasis_children(syntax: &orgize::SyntaxNode) -> Vec<InlineContent> {
    let mut result = Vec::new();
    let children: Vec<_> = syntax.children_with_tokens().collect();

    // Emphasis nodes look like: STAR TEXT ... STAR (for bold).
    // We skip the first and last tokens which are the markers.
    if children.len() >= 2 {
        for elem in &children[1..children.len() - 1] {
            collect_inline_from_element(elem, &mut result);
        }
    }

    result
}

/// For code/verbatim nodes that store a single TEXT token between markers,
/// extract the inner text.
fn extract_emphasis_text(syntax: &orgize::SyntaxNode) -> String {
    syntax
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::TEXT)
        .map(|t| t.text().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

fn convert_link(link: &Link) -> InlineContent {
    let path = link.path().to_string();

    let description = if link.has_description() {
        let desc_elems: Vec<_> = link.description().collect();
        if desc_elems.is_empty() {
            None
        } else {
            let mut inlines = Vec::new();
            for elem in &desc_elems {
                collect_inline_from_element(elem, &mut inlines);
            }
            if inlines.is_empty() {
                None
            } else {
                Some(inlines)
            }
        }
    } else {
        None
    };

    InlineContent::Link { path, description }
}

// ---------------------------------------------------------------------------
// Footnote reference
// ---------------------------------------------------------------------------

fn convert_fn_ref(fnref: &FnRef) -> InlineContent {
    // Syntax: L_BRACKET TEXT("fn") COLON TEXT(label) [COLON objects...] R_BRACKET
    let mut texts = fnref
        .syntax()
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == SyntaxKind::TEXT);

    let _fn_text = texts.next(); // "fn"
    let label_text = texts.next().map(|t| t.to_string()).unwrap_or_default();
    let label = if label_text.is_empty() {
        None
    } else {
        Some(label_text)
    };

    // If there is a definition part (after the second colon), collect it.
    // We look for objects after the second COLON in the syntax tree.
    let definition = {
        let mut after_second_colon = false;
        let mut colon_count = 0u32;
        let mut def_inlines = Vec::new();

        for elem in fnref.syntax().children_with_tokens() {
            if !after_second_colon {
                if let Some(token) = elem.as_token() {
                    if token.kind() == SyntaxKind::COLON {
                        colon_count += 1;
                        if colon_count >= 2 {
                            after_second_colon = true;
                        }
                    }
                }
            } else {
                // Skip the closing bracket.
                if let Some(token) = elem.as_token() {
                    if token.kind() == SyntaxKind::R_BRACKET {
                        break;
                    }
                }
                collect_inline_from_element(&elem, &mut def_inlines);
            }
        }

        if def_inlines.is_empty() {
            None
        } else {
            Some(def_inlines)
        }
    };

    InlineContent::FootnoteReference { label, definition }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_input() {
        let entries = org_to_entries("");
        assert!(entries.is_empty());
    }

    #[test]
    fn zeroth_section_only() {
        let entries = org_to_entries("Hello world.\n");
        assert_eq!(entries.len(), 1);
        match &entries[0].content {
            EntryContent::Section { elements, .. } => {
                assert!(!elements.is_empty());
            }
            _ => panic!("expected Section"),
        }
    }

    #[test]
    fn single_heading() {
        let entries = org_to_entries("* TODO [#A] My heading :tag1:tag2:\n");
        assert_eq!(entries.len(), 1);
        match &entries[0].content {
            EntryContent::Heading(h) => {
                assert_eq!(h.level, 1);
                assert_eq!(h.keyword.as_deref(), Some("TODO"));
                assert_eq!(h.priority.as_deref(), Some("A"));
                assert_eq!(h.tags, vec!["tag1", "tag2"]);
            }
            _ => panic!("expected Heading"),
        }
    }

    #[test]
    fn nested_headings() {
        let input = "* Level 1\n** Level 2\n*** Level 3\n";
        let entries = org_to_entries(input);
        assert_eq!(entries.len(), 1);
        match &entries[0].content {
            EntryContent::Heading(h) => {
                assert_eq!(h.level, 1);
                assert_eq!(h.children.len(), 1);
                assert_eq!(h.children[0].level, 2);
                assert_eq!(h.children[0].children.len(), 1);
                assert_eq!(h.children[0].children[0].level, 3);
            }
            _ => panic!("expected Heading"),
        }
    }

    #[test]
    fn heading_with_body() {
        let input = "* Heading\nSome body text.\n";
        let entries = org_to_entries(input);
        assert_eq!(entries.len(), 1);
        match &entries[0].content {
            EntryContent::Heading(h) => {
                assert!(!h.body.is_empty());
                match &h.body[0] {
                    Element::Paragraph { contents } => {
                        assert!(!contents.is_empty());
                    }
                    _ => panic!("expected Paragraph"),
                }
            }
            _ => panic!("expected Heading"),
        }
    }

    #[test]
    fn planning_info() {
        let input = "* TODO Task\nSCHEDULED: <2024-01-15 Mon>\n";
        let entries = org_to_entries(input);
        match &entries[0].content {
            EntryContent::Heading(h) => {
                assert!(h.planning.is_some());
                let p = h.planning.as_ref().unwrap();
                assert!(p.scheduled.is_some());
            }
            _ => panic!("expected Heading"),
        }
    }

    #[test]
    fn properties() {
        let input = "* Heading\n:PROPERTIES:\n:ID: abc123\n:END:\n";
        let entries = org_to_entries(input);
        match &entries[0].content {
            EntryContent::Heading(h) => {
                assert_eq!(h.properties.len(), 1);
                assert_eq!(h.properties[0].key, "ID");
                assert_eq!(h.properties[0].value, "abc123");
            }
            _ => panic!("expected Heading"),
        }
    }

    #[test]
    fn source_block() {
        let input = "#+begin_src rust\nfn main() {}\n#+end_src\n";
        let entries = org_to_entries(input);
        assert_eq!(entries.len(), 1);
        match &entries[0].content {
            EntryContent::Section { elements, .. } => {
                assert!(elements
                    .iter()
                    .any(|e| matches!(e, Element::SrcBlock { .. })));
            }
            _ => panic!("expected Section"),
        }
    }

    #[test]
    fn inline_markup() {
        let input = "This is *bold* and /italic/ and ~code~.\n";
        let entries = org_to_entries(input);
        match &entries[0].content {
            EntryContent::Section { elements, .. } => match &elements[0] {
                Element::Paragraph { contents } => {
                    let has_bold = contents
                        .iter()
                        .any(|c| matches!(c, InlineContent::Bold { .. }));
                    let has_italic = contents
                        .iter()
                        .any(|c| matches!(c, InlineContent::Italic { .. }));
                    let has_code = contents
                        .iter()
                        .any(|c| matches!(c, InlineContent::Code { .. }));
                    assert!(has_bold, "expected Bold in {:?}", contents);
                    assert!(has_italic, "expected Italic in {:?}", contents);
                    assert!(has_code, "expected Code in {:?}", contents);
                }
                other => panic!("expected Paragraph, got {:?}", other),
            },
            _ => panic!("expected Section"),
        }
    }

    #[test]
    fn link_with_description() {
        let input = "[[https://example.com][Example]]\n";
        let entries = org_to_entries(input);
        match &entries[0].content {
            EntryContent::Section { elements, .. } => match &elements[0] {
                Element::Paragraph { contents } => {
                    let link = contents
                        .iter()
                        .find(|c| matches!(c, InlineContent::Link { .. }));
                    assert!(link.is_some());
                    match link.unwrap() {
                        InlineContent::Link { path, description } => {
                            assert_eq!(path, "https://example.com");
                            assert!(description.is_some());
                        }
                        _ => unreachable!(),
                    }
                }
                _ => panic!("expected Paragraph"),
            },
            _ => panic!("expected Section"),
        }
    }

    #[test]
    fn plain_list() {
        let input = "- item 1\n- item 2\n";
        let entries = org_to_entries(input);
        match &entries[0].content {
            EntryContent::Section { elements, .. } => {
                assert!(elements
                    .iter()
                    .any(|e| matches!(e, Element::PlainList { .. })));
            }
            _ => panic!("expected Section"),
        }
    }

    #[test]
    fn table() {
        let input = "| a | b |\n|---+---|\n| c | d |\n";
        let entries = org_to_entries(input);
        match &entries[0].content {
            EntryContent::Section { elements, .. } => {
                let table = elements.iter().find(|e| matches!(e, Element::Table { .. }));
                assert!(table.is_some());
                match table.unwrap() {
                    Element::Table { rows } => {
                        assert_eq!(rows.len(), 3);
                    }
                    _ => unreachable!(),
                }
            }
            _ => panic!("expected Section"),
        }
    }

    #[test]
    fn schema_version() {
        let entries = org_to_entries("* Hello\n");
        assert_eq!(entries[0].schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn multiple_top_level_headings() {
        let input = "* First\n* Second\n* Third\n";
        let entries = org_to_entries(input);
        assert_eq!(entries.len(), 3);
        for entry in &entries {
            assert!(matches!(entry.content, EntryContent::Heading(_)));
        }
    }

    #[test]
    fn mixed_section_and_headings() {
        let input = "Preamble text.\n* Heading 1\n* Heading 2\n";
        let entries = org_to_entries(input);
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0].content, EntryContent::Section { .. }));
        assert!(matches!(entries[1].content, EntryContent::Heading(_)));
        assert!(matches!(entries[2].content, EntryContent::Heading(_)));
    }
}

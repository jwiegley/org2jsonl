use serde::{Deserialize, Serialize};

fn is_false(v: &bool) -> bool {
    !v
}

/// A single top-level entry in the JSONL output.
/// Each line of the JSONL file is one of these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrgEntry {
    /// Schema version for forward compatibility
    pub schema_version: u32,
    /// The type of this entry
    #[serde(flatten)]
    pub content: EntryContent,
    /// Number of blank lines after this entry (before the next entry)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_blank: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum EntryContent {
    /// Content before the first heading (zeroth section)
    #[serde(rename = "section")]
    Section {
        elements: Vec<Element>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        body_spacing: Vec<bool>,
    },
    /// A top-level heading with all nested content
    #[serde(rename = "heading")]
    Heading(Box<Heading>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heading {
    /// Heading level (1-based)
    pub level: u32,
    /// TODO keyword if present (e.g., "TODO", "DONE")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
    /// Priority character if present (e.g., "A", "B", "C")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// The title text (may contain inline markup)
    pub title: Vec<InlineContent>,
    /// Tags on this heading
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    /// Planning info (SCHEDULED, DEADLINE, CLOSED)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planning: Option<Planning>,
    /// Property drawer
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub properties: Vec<Property>,
    /// Number of blank lines between heading metadata and body content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_body_blank: Option<u32>,
    /// Body elements of this heading's section
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub body: Vec<Element>,
    /// Whether there is a blank line between consecutive body elements.
    /// Length is body.len() - 1 (one entry per pair of adjacent elements).
    /// true = blank line separator, false = no blank line.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub body_spacing: Vec<bool>,
    /// Number of blank lines after body content (before first child heading)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_body_blank: Option<u32>,
    /// Child headings
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<Heading>,
    /// Number of blank lines after this heading (and all its descendants)
    /// before the next sibling heading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_blank: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Planning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Property {
    pub key: String,
    pub value: String,
}

/// Block-level elements
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Element {
    #[serde(rename = "paragraph")]
    Paragraph { contents: Vec<InlineContent> },
    #[serde(rename = "plain_list")]
    PlainList {
        kind: ListKind,
        items: Vec<ListItem>,
    },
    #[serde(rename = "src_block")]
    SrcBlock {
        language: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<String>,
        value: String,
    },
    #[serde(rename = "example_block")]
    ExampleBlock { value: String },
    #[serde(rename = "quote_block")]
    QuoteBlock { elements: Vec<Element> },
    #[serde(rename = "center_block")]
    CenterBlock { elements: Vec<Element> },
    #[serde(rename = "verse_block")]
    VerseBlock { value: String },
    #[serde(rename = "comment_block")]
    CommentBlock { value: String },
    #[serde(rename = "export_block")]
    ExportBlock { backend: String, value: String },
    #[serde(rename = "special_block")]
    SpecialBlock {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<String>,
        value: String,
    },
    #[serde(rename = "drawer")]
    Drawer { name: String, value: String },
    #[serde(rename = "table")]
    Table {
        rows: Vec<TableRow>,
    },
    #[serde(rename = "horizontal_rule")]
    HorizontalRule {
        #[serde(skip_serializing_if = "Option::is_none")]
        dash_count: Option<usize>
    },
    #[serde(rename = "keyword")]
    Keyword { key: String, value: String },
    #[serde(rename = "comment")]
    Comment { value: String },
    #[serde(rename = "fixed_width")]
    FixedWidth { value: String },
    #[serde(rename = "clock")]
    Clock { value: String },
    #[serde(rename = "diary_sexp")]
    DiarySexp { value: String },
    #[serde(rename = "footnote_definition")]
    FootnoteDefinition {
        label: String,
        elements: Vec<Element>,
    },
    #[serde(rename = "affiliated_keyword")]
    AffiliatedKeyword { key: String, value: String },
    #[serde(rename = "latex_environment")]
    LatexEnvironment { value: String },
    #[serde(rename = "dynamic_block")]
    DynamicBlock {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<String>,
        elements: Vec<Element>,
    },
    /// Fallback for unrecognized elements
    #[serde(rename = "raw")]
    Raw { value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    Ordered,
    Unordered,
    Descriptive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListItem {
    /// Bullet text (e.g., "-", "+", "1.", "1)")
    pub bullet: String,
    /// Checkbox state if present
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkbox: Option<CheckboxState>,
    /// Counter set value if present
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_set: Option<String>,
    /// Tag for descriptive lists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Vec<InlineContent>>,
    /// The content elements of this list item
    pub contents: Vec<Element>,
    /// Number of blank lines after each content element.
    /// Length matches `contents`; each entry is the blank-line count
    /// following that element.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub content_spacing: Vec<u32>,
    /// Number of blank lines after this list item
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_blank: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckboxState {
    Checked,
    Unchecked,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableRow {
    #[serde(flatten)]
    pub kind: TableRowKind,
    /// Per-cell widths for this row (content width excluding padding spaces).
    /// Used to reproduce the original column widths on round-trip.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cell_widths: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum TableRowKind {
    #[serde(rename = "standard")]
    Standard { cells: Vec<Vec<InlineContent>> },
    #[serde(rename = "rule")]
    Rule,
}

/// Inline content (objects in Org-mode terminology)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum InlineContent {
    #[serde(rename = "text")]
    Text { value: String },
    #[serde(rename = "bold")]
    Bold { contents: Vec<InlineContent> },
    #[serde(rename = "italic")]
    Italic { contents: Vec<InlineContent> },
    #[serde(rename = "underline")]
    Underline { contents: Vec<InlineContent> },
    #[serde(rename = "strike_through")]
    StrikeThrough { contents: Vec<InlineContent> },
    #[serde(rename = "code")]
    Code { value: String },
    #[serde(rename = "verbatim")]
    Verbatim { value: String },
    #[serde(rename = "link")]
    Link {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<Vec<InlineContent>>,
    },
    #[serde(rename = "timestamp")]
    Timestamp { value: String },
    #[serde(rename = "footnote_reference")]
    FootnoteReference {
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        definition: Option<Vec<InlineContent>>,
    },
    #[serde(rename = "line_break")]
    LineBreak,
    #[serde(rename = "entity")]
    Entity { name: String },
    #[serde(rename = "latex_fragment")]
    LatexFragment { value: String },
    #[serde(rename = "export_snippet")]
    ExportSnippet { backend: String, value: String },
    #[serde(rename = "inline_babel")]
    InlineBabel { value: String },
    #[serde(rename = "inline_src")]
    InlineSrc { language: String, value: String },
    #[serde(rename = "macro")]
    Macro { value: String },
    #[serde(rename = "target")]
    Target { value: String },
    #[serde(rename = "radio_target")]
    RadioTarget { value: String },
    #[serde(rename = "statistics_cookie")]
    StatisticsCookie { value: String },
    #[serde(rename = "subscript")]
    Subscript {
        contents: Vec<InlineContent>,
        #[serde(skip_serializing_if = "is_false", default)]
        use_braces: bool,
    },
    #[serde(rename = "superscript")]
    Superscript {
        contents: Vec<InlineContent>,
        #[serde(skip_serializing_if = "is_false", default)]
        use_braces: bool,
    },
}

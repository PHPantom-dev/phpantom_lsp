//! Parsing adapter for `mago-docblock`.
//!
//! This module bridges our existing docblock extraction code (which works
//! with raw `&str` slices) and the structured [`mago_docblock::document::Document`]
//! representation.
//!
//! # Design
//!
//! Most call sites currently pass a raw docblock string (`&str`) obtained
//! from [`super::tags::get_docblock_text_for_node`].  The adapter provides
//! one entry point:
//!
//! - [`parse_docblock`]: parses a raw `/** … */` string into a `Document`.
//!
//! It creates a short-lived bumpalo arena, parses the docblock, and
//! returns an owned [`DocblockInfo`] that captures the tag data we need
//! without borrowing from the arena.  This keeps the arena lifetime
//! contained within each call.

use bumpalo::Bump;
use mago_docblock::document::{Element, TagKind, TextSegment};
use mago_span::Span;

/// Owned snapshot of a parsed tag from a `mago-docblock` `Document`.
///
/// This captures the tag name, kind, and description as owned `String`s
/// so callers do not need to worry about arena lifetimes.
#[derive(Debug, Clone)]
pub struct TagInfo {
    /// The raw tag name (e.g. `"param"`, `"return"`, `"deprecated"`).
    pub name: String,
    /// The structured tag kind from `mago-docblock`.
    pub kind: TagKind,
    /// The full description text after the tag name.  For a tag like
    /// `@param string $foo A description`, this would be
    /// `"string $foo A description"`.
    pub description: String,
    /// The span of the entire tag (from `@` to the end of the description)
    /// in the source file.
    pub span: Span,
    /// The span of just the description portion of the tag.
    pub description_span: Span,
}

/// Owned snapshot of a parsed docblock.
///
/// Contains the free-text description (before the first tag) and all
/// structured tag entries extracted from the `Document`.
#[derive(Debug, Clone)]
pub struct DocblockInfo {
    /// The free-text description that appears before the first `@tag`.
    ///
    /// This captures `Element::Text` content from the mago-docblock
    /// `Document`, joining paragraph segments with inline code (wrapped
    /// in backticks) and inline tags (wrapped in `{@tag ...}`).
    ///
    /// Returns `None` when the docblock has no text before the first tag
    /// (e.g. `/** @return string */`).
    pub description: Option<String>,
    /// All tags found in the docblock, in source order.
    pub tags: Vec<TagInfo>,
}

impl DocblockInfo {
    /// Returns an iterator over tags matching the given `TagKind`.
    pub fn tags_by_kind(&self, kind: TagKind) -> impl Iterator<Item = &TagInfo> {
        self.tags.iter().filter(move |t| t.kind == kind)
    }

    /// Returns an iterator over tags matching any of the given `TagKind`s.
    pub fn tags_by_kinds<'a>(&'a self, kinds: &'a [TagKind]) -> impl Iterator<Item = &'a TagInfo> {
        self.tags.iter().filter(move |t| kinds.contains(&t.kind))
    }

    /// Returns the first tag matching the given `TagKind`, if any.
    pub fn first_tag_by_kind(&self, kind: TagKind) -> Option<&TagInfo> {
        self.tags_by_kind(kind).next()
    }
}

/// Parse a raw docblock string (including `/**` and `*/` delimiters) into
/// a [`DocblockInfo`].
///
/// Returns `None` if the string is not a valid docblock comment or if
/// parsing fails.  This function never panics.
///
/// # Arguments
///
/// * `docblock` — The full docblock text, e.g. `"/** @return string */"`.
/// * `base_span` — The span in the source file where this docblock starts.
///   When the caller does not have span information (e.g. unit tests that
///   work with standalone strings), pass [`Span::default()`] or a
///   zero-offset span.
pub fn parse_docblock(docblock: &str, base_span: Span) -> Option<DocblockInfo> {
    let arena = Bump::new();

    // `parse_phpdoc_with_span` requires `content: &'arena [u8]`.
    // We allocate the bytes into the arena so that the borrow lives
    // long enough.
    let content: &[u8] = arena.alloc_slice_copy(docblock.as_bytes());

    let document = mago_docblock::parse_phpdoc_with_span(&arena, content, base_span).ok()?;

    Some(collect_tags(&document))
}

/// Walk a parsed `Document` and collect all `Tag` elements into owned
/// [`TagInfo`] values, and extract the free-text description from
/// `Text` elements that appear before the first tag.
fn collect_tags(document: &mago_docblock::document::Document<'_>) -> DocblockInfo {
    use crate::atom::bytes_to_str;
    let mut tags = Vec::new();
    let mut description_parts: Vec<String> = Vec::new();
    let mut seen_tag = false;

    for element in &document.elements {
        match element {
            Element::Tag(tag) => {
                seen_tag = true;
                tags.push(TagInfo {
                    name: bytes_to_str(tag.name).to_owned(),
                    kind: tag.kind,
                    description: bytes_to_str(tag.description).to_owned(),
                    span: tag.span,
                    description_span: tag.description_span,
                });
            }
            Element::Text(text) if !seen_tag => {
                for seg in &text.segments {
                    match seg {
                        TextSegment::Paragraph { content, .. } => {
                            description_parts.push(bytes_to_str(content).to_owned());
                        }
                        TextSegment::InlineCode(code) => {
                            description_parts.push(format!("`{}`", bytes_to_str(code.content)));
                        }
                        TextSegment::InlineTag(tag) => {
                            description_parts.push(format!(
                                "{{@{} {}}}",
                                bytes_to_str(tag.name),
                                bytes_to_str(tag.description)
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let description = if description_parts.is_empty() {
        None
    } else {
        Some(description_parts.join(""))
    };

    DocblockInfo { description, tags }
}

/// Collapse `\n` (and any surrounding horizontal whitespace) into a
/// single space.
///
/// mago-docblock joins multi-line tag descriptions with `\n`, but the
/// continuation lines may carry leading whitespace from the source
/// indentation.  The old line-by-line scanner trimmed each line before
/// joining with a space; this helper reproduces that behaviour.
pub fn collapse_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            // Trim trailing whitespace already appended
            let trimmed_len = out.trim_end().len();
            out.truncate(trimmed_len);
            // Skip leading whitespace on the next line
            while chars.peek().is_some_and(|&ch| ch == ' ' || ch == '\t') {
                chars.next();
            }
            // Decide whether a separating space is needed.  Skip the
            // space when the last emitted character is a structural
            // opener (`<`, `{`, `(`) or when the next character is a
            // structural closer (`>`, `}`, `)`, `,`, `:`) — these
            // tokens are already unambiguous without whitespace and
            // the old line-by-line scanner never inserted spaces in
            // these positions.
            let last_ch = out.chars().last();
            let next_ch = chars.peek().copied();
            let skip_space = matches!(last_ch, Some('<' | '{' | '('))
                || matches!(next_ch, Some('>' | '}' | ')'));
            if !out.is_empty() && !out.ends_with(' ') && !skip_space {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a raw docblock string into a [`DocblockInfo`] with a zero-offset span.
///
/// This is the standard entry point for all tag extraction functions that
/// receive a raw `&str` docblock.  The span is set to cover the entire
/// string starting at offset 0, which is correct for standalone extraction
/// (the spans are only meaningful when the caller needs source positions).
///
/// Returns `None` if the string is not a valid docblock or parsing fails.
/// For partial docblocks (e.g. during completion when `*/` is missing),
/// use [`parse_docblock_for_tags_lossy`] instead.
pub fn parse_docblock_for_tags(docblock: &str) -> Option<DocblockInfo> {
    use mago_database::file::FileId;
    use mago_span::Position;

    let span = Span::new(
        FileId::zero(),
        Position::new(0),
        Position::new(docblock.len() as u32),
    );
    parse_docblock(docblock, span)
}

/// Like [`parse_docblock_for_tags`], but attempts to fix up partial
/// docblocks before parsing.
///
/// When the standard parse returns `None` (e.g. because the closing `*/`
/// is missing while the user is still typing), this function appends
/// `*/` and retries.  This makes it suitable for completion-time tag
/// detection where the docblock is incomplete.
///
/// Callers that need accurate span information should prefer
/// [`parse_docblock_for_tags`] since the appended `*/` shifts nothing
/// but may produce a slightly different parse tree.
pub fn parse_docblock_for_tags_lossy(docblock: &str) -> Option<DocblockInfo> {
    // Try the normal parse first.
    if let Some(info) = parse_docblock_for_tags(docblock) {
        return Some(info);
    }

    // The docblock contains something mago-docblock can't handle (most
    // commonly a bare `@` where the user is still typing a tag name).
    // Try removing bare `@` tokens and re-parsing.

    // Strip lines that are just ` * @` (bare @ with no tag name).
    // This handles both complete docblocks (`*/` present) and partial
    // ones (`*/` missing).
    let cleaned: String = docblock
        .lines()
        .filter(|line| {
            let t = line.trim().trim_start_matches('*').trim();
            t != "@"
        })
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(info) = parse_docblock_for_tags(&cleaned) {
        return Some(info);
    }

    // If the docblock is still missing `*/`, append it.
    let trimmed = cleaned.trim_end();
    if trimmed.ends_with("*/") {
        return None;
    }

    let fixed = format!("{}\n */", trimmed);
    parse_docblock_for_tags(&fixed)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_return_tag() {
        let doc = "/** @return string */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Return);
        assert_eq!(info.tags[0].description, "string");
    }

    #[test]
    fn parse_multiple_tags() {
        let doc = r#"/**
         * @param string $name The name
         * @param int $age The age
         * @return bool
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 3);

        assert_eq!(info.tags[0].kind, TagKind::Param);
        assert_eq!(info.tags[0].description, "string $name The name");

        assert_eq!(info.tags[1].kind, TagKind::Param);
        assert_eq!(info.tags[1].description, "int $age The age");

        assert_eq!(info.tags[2].kind, TagKind::Return);
        assert_eq!(info.tags[2].description, "bool");
    }

    #[test]
    fn parse_deprecated_tag_bare() {
        let doc = "/** @deprecated */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Deprecated)
            .expect("should have deprecated");
        assert_eq!(tag.description, "");
    }

    #[test]
    fn parse_deprecated_tag_with_message() {
        let doc = "/** @deprecated Use newMethod() instead */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Deprecated)
            .expect("should have deprecated");
        assert_eq!(tag.description, "Use newMethod() instead");
    }

    #[test]
    fn parse_mixin_tag() {
        let doc = "/** @mixin \\App\\Models\\User */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Mixin)
            .expect("should have mixin");
        assert_eq!(tag.description, "\\App\\Models\\User");
    }

    #[test]
    fn parse_throws_tag() {
        let doc = "/** @throws \\InvalidArgumentException When input is bad */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Throws)
            .expect("should have throws");
        assert_eq!(
            tag.description,
            "\\InvalidArgumentException When input is bad"
        );
    }

    #[test]
    fn parse_var_tag() {
        let doc = "/** @var array<int, string> $items */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Var)
            .expect("should have var");
        assert_eq!(tag.description, "array<int, string> $items");
    }

    #[test]
    fn parse_see_tag() {
        let doc = "/** @see MyClass::method() */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::See)
            .expect("should have see");
        assert_eq!(tag.description, "MyClass::method()");
    }

    #[test]
    fn parse_phpstan_assert_tags() {
        let doc = r#"/**
         * @phpstan-assert string $value
         * @phpstan-assert-if-true non-empty-string $value
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let kinds: Vec<TagKind> = info.tags.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![TagKind::PhpstanAssert, TagKind::PhpstanAssertIfTrue]
        );
    }

    #[test]
    fn tags_by_kind_filters_correctly() {
        let doc = r#"/**
         * @param string $a
         * @return int
         * @param bool $b
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let params: Vec<_> = info.tags_by_kind(TagKind::Param).collect();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].description, "string $a");
        assert_eq!(params[1].description, "bool $b");

        let returns: Vec<_> = info.tags_by_kind(TagKind::Return).collect();
        assert_eq!(returns.len(), 1);
    }

    #[test]
    fn tags_by_kinds_filters_multiple() {
        let doc = r#"/**
         * @phpstan-assert int $x
         * @psalm-assert string $y
         * @param bool $z
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let asserts: Vec<_> = info
            .tags_by_kinds(&[TagKind::PhpstanAssert, TagKind::PsalmAssert])
            .collect();
        assert_eq!(asserts.len(), 2);
    }

    #[test]
    fn invalid_docblock_returns_none() {
        assert!(parse_docblock_for_tags("/* not a docblock */").is_none());
        assert!(parse_docblock_for_tags("// not a docblock").is_none());
        assert!(parse_docblock_for_tags("").is_none());
    }

    #[test]
    fn parse_template_tags() {
        let doc = r#"/**
         * @template T
         * @template-covariant TValue of object
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let templates: Vec<_> = info
            .tags_by_kinds(&[TagKind::Template, TagKind::TemplateCovariant])
            .collect();
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].kind, TagKind::Template);
        assert_eq!(templates[0].description, "T");
        assert_eq!(templates[1].kind, TagKind::TemplateCovariant);
        assert_eq!(templates[1].description, "TValue of object");
    }

    #[test]
    fn parse_property_tags() {
        let doc = r#"/**
         * @property string $name
         * @property-read int $id
         * @property-write bool $active
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        assert_eq!(info.tags.len(), 3);
        assert_eq!(info.tags[0].kind, TagKind::Property);
        assert_eq!(info.tags[1].kind, TagKind::PropertyRead);
        assert_eq!(info.tags[2].kind, TagKind::PropertyWrite);
    }

    #[test]
    fn parse_method_tag() {
        let doc = "/** @method static Builder query() */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Method)
            .expect("should have method");
        assert_eq!(tag.description, "static Builder query()");
    }

    #[test]
    fn parse_multiline_param_type() {
        let doc = r#"/**
         * @param array{
         *   name: string,
         *   age: int
         * } $data
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Param)
            .expect("should have param");
        // mago-docblock joins multi-line tag descriptions
        assert!(tag.description.contains("$data"));
        assert!(tag.description.contains("name: string"));
    }

    #[test]
    fn parse_link_tag() {
        let doc = "/** @link https://php.net/array_map */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Link)
            .expect("should have link");
        assert_eq!(tag.description, "https://php.net/array_map");
    }

    #[test]
    fn parse_extends_tag() {
        let doc = "/** @extends Collection<int, User> */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Extends)
            .expect("should have extends");
        assert_eq!(tag.description, "Collection<int, User>");
    }

    #[test]
    fn parse_phpstan_type_tag() {
        let doc = "/** @phpstan-type Money = array{amount: int, currency: string} */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::PhpstanType)
            .expect("should have type");
        assert!(tag.description.contains("Money"));
    }

    #[test]
    fn parse_phpstan_import_type_tag() {
        let doc = "/** @phpstan-import-type Money from PriceCalculator */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::PhpstanImportType)
            .expect("should have import-type");
        assert!(tag.description.contains("Money"));
        assert!(tag.description.contains("PriceCalculator"));
    }

    #[test]
    fn parse_param_closure_this_tag() {
        let doc = "/** @param-closure-this \\App\\Route $callback */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::ParamClosureThis)
            .expect("should have param-closure-this");
        assert!(tag.description.contains("\\App\\Route"));
        assert!(tag.description.contains("$callback"));
    }

    #[test]
    fn phpstan_extends_tag_parsed_as_other() {
        // mago-docblock classifies @phpstan-extends as TagKind::Other
        // since it has no dedicated variant. Our extract_generics_tag
        // handles this via a name-based fallback.
        let doc = "/**\n * @phpstan-extends Collection<int, User>\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Other);
        assert_eq!(info.tags[0].name, "phpstan-extends");
        assert_eq!(info.tags[0].description, "Collection<int, User>");
    }

    #[test]
    fn phpstan_require_extends_tag_parsed() {
        let doc = "/**\n * @phpstan-require-extends JsonResource\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::PhpstanRequireExtends);
        assert_eq!(info.tags[0].name, "phpstan-require-extends");
        assert_eq!(info.tags[0].description, "JsonResource");
        assert!(
            info.tags[0].description_span.start.offset < info.tags[0].description_span.end.offset,
            "description span should be non-empty: {:?}",
            info.tags[0].description_span
        );
    }

    #[test]
    fn phpstan_require_implements_tag_parsed() {
        let doc = "/**\n * @phpstan-require-implements Countable\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::PhpstanRequireImplements);
        assert_eq!(info.tags[0].name, "phpstan-require-implements");
        assert_eq!(info.tags[0].description, "Countable");
        assert!(
            info.tags[0].description_span.start.offset < info.tags[0].description_span.end.offset,
            "description span should be non-empty: {:?}",
            info.tags[0].description_span
        );
    }

    #[test]
    fn phpstan_sealed_tag_parsed() {
        let doc = "/**\n * @phpstan-sealed FooClass|BarClass\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        // mago-docblock doesn't have a dedicated variant for @phpstan-sealed,
        // so it falls through to TagKind::Other.
        assert_eq!(info.tags[0].kind, TagKind::Other);
        assert_eq!(info.tags[0].name, "phpstan-sealed");
        assert_eq!(info.tags[0].description, "FooClass|BarClass");
    }

    #[test]
    fn multiline_return_description_uses_newlines() {
        let doc = "/**\n * @return array an array containing all the elements of arr1\n * after applying the callback function to each one.\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Return)
            .expect("should have return");
        // mago-docblock joins continuation lines with \n, not spaces
        assert_eq!(
            tag.description,
            "array an array containing all the elements of arr1\nafter applying the callback function to each one."
        );
    }

    #[test]
    fn multiline_type_in_return_tag() {
        let doc =
            "/**\n * @return array{\n *   name: string,\n *   age: int\n * } the user data\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Return)
            .expect("should have return");
        // mago-docblock joins multi-line type + description with \n
        assert!(
            tag.description.contains("name: string"),
            "should contain shape fields: {:?}",
            tag.description
        );
        assert!(
            tag.description.contains("the user data"),
            "should contain description after type: {:?}",
            tag.description
        );
    }

    #[test]
    fn description_extracted_from_text_elements() {
        let doc = "/**\n * This is a description.\n * Second line.\n *\n * @return string\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(
            info.description.as_deref(),
            Some("This is a description.\nSecond line.")
        );
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Return);
    }

    #[test]
    fn description_none_when_tags_only() {
        let doc = "/** @return string */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.description, None);
    }

    #[test]
    fn description_with_inline_code() {
        let doc = "/**\n * Use `code` here.\n * @return void\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.description.as_deref(), Some("Use `code` here."));
    }

    #[test]
    fn description_with_inline_link_tag() {
        let doc = "/**\n * See {@link https://php.net} for details.\n * @return void\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(
            info.description.as_deref(),
            Some("See {@link https://php.net} for details.")
        );
    }

    #[test]
    fn description_with_html_tags_preserved() {
        let doc = "/**\n * Use <b>bold</b> text.\n * @param string $x\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let desc = info
            .description
            .as_deref()
            .expect("should have description");
        assert!(
            desc.contains("<b>bold</b>"),
            "HTML tags should be preserved in raw description: {desc}"
        );
    }

    #[test]
    fn tag_spans_are_populated() {
        let doc = "/** @return string The result */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = &info.tags[0];
        // The span should cover the @return tag
        assert!(
            tag.span.start.offset < tag.span.end.offset,
            "tag span should be non-empty"
        );
        // The description_span should cover "string The result"
        assert!(
            tag.description_span.start.offset < tag.description_span.end.offset,
            "description span should be non-empty"
        );
    }

    #[test]
    fn description_only_docblock() {
        let doc = "/**\n * Just a description, no tags.\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(
            info.description.as_deref(),
            Some("Just a description, no tags.")
        );
        assert!(info.tags.is_empty());
    }

    #[test]
    fn partial_docblock_without_closing_delimiter() {
        // When the user is still typing, the docblock may not have a
        // closing `*/`.  Verify that mago-docblock either parses what
        // it can or returns None gracefully (no panic).
        let doc = "/**\n * @param string $name\n * @return ";
        let result = parse_docblock_for_tags(doc);
        // Whether it succeeds or returns None is fine — the important
        // thing is that it does not panic.
        if let Some(info) = result {
            // If it does parse, the tags should be reasonable.
            assert!(
                !info.tags.is_empty(),
                "partial parse should find at least one tag"
            );
            // Verify the @param tag was parsed correctly.
            let params: Vec<_> = info.tags_by_kind(TagKind::Param).collect();
            assert_eq!(params.len(), 1, "should find one @param tag");
            assert!(
                params[0].description.contains("$name"),
                "param description should contain $name: {:?}",
                params[0].description
            );
        }
    }

    #[test]
    fn partial_docblock_with_trailing_at_sign() {
        // Simulates a completion scenario: the user has typed `@` on a
        // new line but hasn't finished the tag yet.  The docblock has
        // no closing `*/` because the cursor is mid-edit.
        let doc = "/**\n * @param string $name\n * @";
        // Without fix-up, mago-docblock may return None for partial input.
        let result = parse_docblock_for_tags(doc);
        assert!(
            result.is_none(),
            "bare partial docblock with trailing @ returns None"
        );

        // With the fix-up helper, the bare `@` line is stripped and `*/`
        // is appended, making the docblock parseable.
        let fixed = parse_docblock_for_tags_lossy(doc);
        let info = fixed.expect("fix-up should make partial docblock parseable");
        let params: Vec<_> = info.tags_by_kind(TagKind::Param).collect();
        assert_eq!(
            params.len(),
            1,
            "should find @param after fix-up: tags={:?}",
            info.tags.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        assert!(
            params[0].description.contains("$name"),
            "param should contain $name: {:?}",
            params[0].description
        );
    }

    #[test]
    fn lossy_parse_already_complete_docblock() {
        // When the docblock is already complete, lossy parse behaves
        // identically to the normal parse.
        let doc = "/**\n * @param int $x\n * @return string\n */";
        let info = parse_docblock_for_tags_lossy(doc).expect("should parse");
        assert_eq!(info.tags.len(), 2);
        assert_eq!(info.tags[0].kind, TagKind::Param);
        assert_eq!(info.tags[1].kind, TagKind::Return);
    }

    #[test]
    fn complete_docblock_with_bare_at_mid_body() {
        // Simulates the throws-completion scenario: the docblock has a
        // closing `*/` but contains a bare `@` where the user is typing.
        // `parse_docblock_for_tags` (strict) may fail on the bare `@`;
        // `parse_docblock_for_tags_lossy` must still find the @throws tag.
        let doc = "/**\n * @throws RuntimeException\n * @\n */";

        // Strict parse fails because of the bare `@`.
        assert!(parse_docblock_for_tags(doc).is_none());

        // Lossy strips the bare-@ line and succeeds.
        let info = parse_docblock_for_tags_lossy(doc).expect("lossy should parse");
        let throws: Vec<_> = info.tags_by_kind(TagKind::Throws).collect();
        assert_eq!(
            throws.len(),
            1,
            "should find @throws despite bare @: tags={:?}",
            info.tags
                .iter()
                .map(|t| format!("@{}", t.name))
                .collect::<Vec<_>>()
        );
        assert!(
            throws[0].description.contains("RuntimeException"),
            "throws tag should contain RuntimeException: {:?}",
            throws[0].description
        );
    }
}

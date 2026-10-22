//! Promote Constructor Parameter code action.
//!
//! When the cursor is on a constructor parameter that has a corresponding
//! property declaration and a `$this->name = $name;` assignment in the
//! constructor body, this code action offers to convert it into a
//! constructor-promoted property.
//!
//! **Code action kind:** `refactor.rewrite`.
//!
//! The edit removes the property declaration, removes the assignment
//! statement, and adds a visibility modifier (and optionally `readonly`)
//! to the parameter.  This is a single-file edit — call sites are
//! unaffected because constructor promotion is transparent to callers.

#[cfg(test)]
use mago_allocator::LocalArena;
use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::class_like::property::{PlainProperty, Property, PropertyItem};
use mago_syntax::cst::modifier::Modifier;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::text_position::{find_line_end, find_line_start};

// ── Data types ──────────────────────────────────────────────────────────────

/// All the information needed to generate the promotion edit.
struct PromotionCandidate {
    /// The text to insert before the parameter's type hint (or variable
    /// if there is no type hint) — e.g. `"private "`, `"public readonly "`.
    /// Any attributes carried over from the property declaration are
    /// prepended, since PHP requires them ahead of the modifiers.
    prefix: String,
    /// Byte span of the property declaration to delete (includes leading
    /// whitespace on its line and trailing newline).
    property_delete_start: usize,
    property_delete_end: usize,
    /// Byte span of the assignment statement to delete (includes leading
    /// whitespace and trailing newline).
    assignment_delete_start: usize,
    assignment_delete_end: usize,
    /// Byte offset where the visibility prefix should be inserted
    /// (just before the parameter's type hint or variable).
    param_insert_offset: usize,
    /// Default value text from the property declaration to carry over,
    /// if the parameter doesn't already have a default.  Includes the
    /// ` = <value>` text.
    carry_default: Option<String>,
    /// Byte offset where the default value should be inserted (after
    /// the parameter variable name), if `carry_default` is `Some`.
    default_insert_offset: Option<usize>,
}

impl Backend {
    /// Collect "Promote constructor parameter" code actions.
    pub(crate) fn collect_promote_constructor_param_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = crate::text_position::position_to_offset(content, params.range.start);

        let candidate = crate::parser::with_parsed_program(
            content,
            "promote_constructor_param",
            |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);
                find_promotion_candidate(&ctx, content, cursor_offset)
            },
        );

        let candidate = match candidate {
            Some(c) => c,
            None => return,
        };

        let mut edits = Vec::with_capacity(4);

        edits.push(TextEdit {
            range: crate::text_position::byte_range_to_lsp_range(
                content,
                candidate.property_delete_start,
                candidate.property_delete_end,
            ),
            new_text: String::new(),
        });

        edits.push(TextEdit {
            range: crate::text_position::byte_range_to_lsp_range(
                content,
                candidate.assignment_delete_start,
                candidate.assignment_delete_end,
            ),
            new_text: String::new(),
        });

        edits.push(TextEdit {
            range: crate::text_position::byte_range_to_lsp_range(
                content,
                candidate.param_insert_offset,
                candidate.param_insert_offset,
            ),
            new_text: candidate.prefix,
        });

        if let (Some(default_text), Some(offset)) =
            (candidate.carry_default, candidate.default_insert_offset)
        {
            edits.push(TextEdit {
                range: crate::text_position::byte_range_to_lsp_range(content, offset, offset),
                new_text: default_text,
            });
        }

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Promote to constructor property".to_string(),
            kind: Some(CodeActionKind::new("refactor.rewrite")),
            diagnostics: None,
            edit: Some(crate::code_actions::single_file_edit(doc_uri, edits)),
            command: None,
            is_preferred: None,
            disabled: None,
            data: None,
        }));
    }
}

// ── Core logic ──────────────────────────────────────────────────────────────

/// Examine the cursor context and determine if a promotion is possible.
fn find_promotion_candidate(
    ctx: &CursorContext<'_>,
    content: &str,
    cursor: u32,
) -> Option<PromotionCandidate> {
    let (method, all_members) = match ctx {
        CursorContext::InClassLike {
            member: MemberContext::Method(method, _),
            all_members,
            ..
        } => (*method, *all_members),
        _ => return None,
    };

    if method.name.value != b"__construct" {
        return None;
    }

    let body = match &method.body {
        MethodBody::Concrete(block) => block,
        MethodBody::Abstract(_) => return None,
    };

    // Find the parameter under the cursor.
    let param = method.parameter_list.parameters.iter().find(|p| {
        let span = p.span();
        cursor >= span.start.offset && cursor <= span.end.offset
    })?;

    if param.is_promoted_property() {
        return None;
    }

    // Must not be variadic or by-reference.
    if param.ellipsis.is_some() || param.ampersand.is_some() {
        return None;
    }

    let param_name = bytes_to_str(param.variable.name);
    // Strip the leading `$` for property matching.
    let bare_name = param_name.strip_prefix('$').unwrap_or(param_name);

    let (property, plain_prop) = find_matching_property(all_members, bare_name)?;

    // Only promote a property declared on its own. A multi-variable
    // declaration (`private int $a, $b;`) is a single statement whose span
    // covers every variable, so deleting it would silently drop the
    // siblings. Decline the action rather than corrupt the declaration.
    if plain_prop.items.len() != 1 {
        return None;
    }

    if is_static(plain_prop.modifiers.iter()) {
        return None;
    }

    let visibility = extract_visibility_keyword(plain_prop.modifiers.iter());
    let is_readonly = has_readonly(plain_prop.modifiers.iter());

    let attributes = render_carried_attributes(plain_prop, param, content);

    let vis_str = visibility.unwrap_or("public");
    let prefix = if is_readonly {
        format!("{attributes}{vis_str} readonly ")
    } else {
        format!("{attributes}{vis_str} ")
    };

    // Find the `$this->name = $name;` assignment in the constructor body.
    let body_text = content
        .get(body.left_brace.end.offset as usize..body.right_brace.start.offset as usize)
        .unwrap_or("");
    let body_base = body.left_brace.end.offset as usize;

    let assignment_span = find_assignment_span(body_text, body_base, bare_name, param_name)?;

    // Safety check: the parameter variable must appear exactly twice in
    // the body — once as the RHS of the assignment and zero other times.
    // (The assignment itself contains one occurrence of `$paramName`.)
    let occurrences = count_occurrences(body_text, param_name);
    if occurrences > 1 {
        return None;
    }

    let param_insert_offset = if let Some(hint) = &param.hint {
        hint.span().start.offset as usize
    } else {
        param.variable.span.start.offset as usize
    };

    // Determine the property deletion span (include leading whitespace
    // on its line and trailing newline).  A `/** @var … */` docblock above
    // the declaration goes with it — left behind it would sit orphaned
    // above the constructor.
    let prop_span = property.span();
    let line_start = find_line_start(content, prop_span.start.offset as usize);
    let property_delete_start =
        super::docblock_edit::docblock_start_above_offset(content, line_start)
            .unwrap_or(line_start);
    let property_delete_end = find_line_end(content, prop_span.end.offset as usize);

    // Check if the property has a default value that the parameter lacks.
    let (carry_default, default_insert_offset) = if param.default_value.is_none() {
        if let Some(default_text) = extract_property_default(plain_prop, content) {
            let offset = param.variable.span.end.offset as usize;
            (Some(format!(" = {default_text}")), Some(offset))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    Some(PromotionCandidate {
        prefix,
        property_delete_start,
        property_delete_end,
        assignment_delete_start: assignment_span.0,
        assignment_delete_end: assignment_span.1,
        param_insert_offset,
        carry_default,
        default_insert_offset,
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Find a non-static, non-promoted property with the given bare name in
/// the class members.  Returns the Property node and its inner
/// PlainProperty (we only promote plain properties, not hooked ones).
fn find_matching_property<'a>(
    members: &'a Sequence<'a, ClassLikeMember<'a>>,
    bare_name: &str,
) -> Option<(&'a Property<'a>, &'a PlainProperty<'a>)> {
    for member in members.iter() {
        if let ClassLikeMember::Property(property) = member
            && let Property::Plain(plain) = property
        {
            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name);
                let item_bare = var_name.strip_prefix('$').unwrap_or(var_name);
                if item_bare == bare_name {
                    return Some((property, plain));
                }
            }
        }
    }
    None
}

/// Render the property's attributes as the leading text of the promoted
/// parameter, e.g. `"#[SomeAttr] "`.  Returns an empty string when the
/// property has no attributes.
///
/// An attribute is executable metadata, so dropping it along with the
/// property declaration would change runtime behaviour.  PHP applies an
/// attribute written on a promoted parameter to both the parameter and the
/// synthesised property, and has no syntax for targeting only one of the
/// two, so the promoted form is not a perfect equivalent of a
/// property-only attribute.  Keeping it is still much closer than losing
/// it, and only reflection over the constructor's parameters can observe
/// the difference.
///
/// Each attribute gets its own `#[…]` group (`#[A, B]` becomes
/// `#[A] #[B]`, which PHP treats identically) so that one the parameter
/// already carries can be skipped individually: repeating an attribute
/// that is not `Attribute::IS_REPEATABLE` is a fatal error.
fn render_carried_attributes(
    plain: &PlainProperty<'_>,
    param: &FunctionLikeParameter<'_>,
    content: &str,
) -> String {
    let mut out = String::new();

    for attribute in plain
        .attribute_lists
        .iter()
        .flat_map(|list| list.attributes.iter())
    {
        let already_on_param = param
            .attribute_lists
            .iter()
            .flat_map(|list| list.attributes.iter())
            .any(|existing| attribute_names_match(existing, attribute));
        if already_on_param {
            continue;
        }

        let span = attribute.span();
        let Some(text) = content.get(span.start.offset as usize..span.end.offset as usize) else {
            continue;
        };

        out.push_str("#[");
        out.push_str(text);
        out.push_str("] ");
    }

    out
}

/// Whether two attribute names are spelled the same, case-insensitively as
/// PHP resolves class names.
///
/// Only identical spellings count.  `\Foo` and `Foo` name the same class in
/// the global namespace but different ones inside a `namespace`, and
/// wrongly treating them as equal would drop an attribute — the very thing
/// this carry-over exists to prevent.  Leaving a genuine duplicate in place
/// is the safer error: PHP reports it immediately.
fn attribute_names_match(a: &Attribute<'_>, b: &Attribute<'_>) -> bool {
    a.name.value().eq_ignore_ascii_case(b.name.value())
}

fn extract_visibility_keyword<'a>(
    modifiers: impl Iterator<Item = &'a Modifier<'a>>,
) -> Option<&'static str> {
    for m in modifiers {
        match m {
            Modifier::Public(_) => return Some("public"),
            Modifier::Protected(_) => return Some("protected"),
            Modifier::Private(_) => return Some("private"),
            _ => continue,
        }
    }
    None
}

fn is_static<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers
        .into_iter()
        .any(|m| matches!(m, Modifier::Static(_)))
}

fn has_readonly<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers
        .into_iter()
        .any(|m| matches!(m, Modifier::Readonly(_)))
}

/// Find the byte span of the `$this->name = $name;` assignment statement
/// in the constructor body, including leading whitespace and trailing newline.
///
/// `body_text` is the text between `{` and `}` of the constructor body.
/// `body_base` is the byte offset of the start of `body_text` in the file.
///
/// Returns `(start, end)` byte offsets in the full file content.
fn find_assignment_span(
    body_text: &str,
    body_base: usize,
    bare_name: &str,
    param_name: &str,
) -> Option<(usize, usize)> {
    // Build the exact pattern we're looking for.
    let pattern = format!("$this->{bare_name} = {param_name};");

    let idx = body_text.find(&pattern)?;

    // Expand backward to include leading whitespace on the same line.
    let mut start = idx;
    while start > 0 {
        let prev = body_text.as_bytes()[start - 1];
        if prev == b' ' || prev == b'\t' {
            start -= 1;
        } else if prev == b'\n' {
            // Include the newline so we delete the whole line.
            break;
        } else {
            break;
        }
    }

    // Expand forward past the semicolon to include trailing whitespace/newline.
    let mut end = idx + pattern.len();
    while end < body_text.len() {
        let ch = body_text.as_bytes()[end];
        if ch == b'\n' {
            end += 1;
            break;
        } else if ch == b'\r' {
            end += 1;
            if end < body_text.len() && body_text.as_bytes()[end] == b'\n' {
                end += 1;
            }
            break;
        } else if ch == b' ' || ch == b'\t' {
            end += 1;
        } else {
            break;
        }
    }

    Some((body_base + start, body_base + end))
}

/// Count occurrences of `needle` in `haystack`, matching whole-identifier
/// boundaries (preceded by non-alphanumeric/underscore, followed by
/// non-alphanumeric/underscore).
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    let mut count = 0;
    let mut start = 0;
    let bytes = haystack.as_bytes();
    while let Some(pos) = haystack[start..].find(needle) {
        let abs_pos = start + pos;
        let after = abs_pos + needle.len();

        // Check that the character after is not an identifier character.
        let ok_after =
            after >= bytes.len() || (!bytes[after].is_ascii_alphanumeric() && bytes[after] != b'_');

        if ok_after {
            count += 1;
        }
        start = abs_pos + 1;
    }
    count
}

/// Extract the default value expression text from a plain property.
fn extract_property_default<'a>(plain: &PlainProperty<'a>, content: &str) -> Option<String> {
    // Only single-item properties can be promoted.
    let item = plain.items.first()?;
    if let PropertyItem::Concrete(concrete) = item {
        let start = concrete.value.span().start.offset as usize;
        let end = concrete.value.span().end.offset as usize;
        let text = content.get(start..end)?;
        Some(text.to_string())
    } else {
        None
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse PHP and find a promotion candidate at the given byte offset.
    fn find_candidate(php: &str, offset: u32) -> Option<PromotionCandidate> {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());
        let ctx = find_cursor_context(&program.statements, offset);
        find_promotion_candidate(&ctx, php, offset)
    }

    /// Helper: apply a candidate's edits to the source text and return the result.
    fn apply_candidate(php: &str, c: &PromotionCandidate) -> String {
        let mut edits: Vec<(usize, usize, &str)> = vec![
            (c.property_delete_start, c.property_delete_end, ""),
            (c.assignment_delete_start, c.assignment_delete_end, ""),
            (c.param_insert_offset, c.param_insert_offset, &c.prefix),
        ];
        if let (Some(default_text), Some(offset)) = (&c.carry_default, c.default_insert_offset) {
            edits.push((offset, offset, default_text));
        }
        // Sort by start offset descending so applying them doesn't shift positions.
        edits.sort_by_key(|x| std::cmp::Reverse(x.0));

        let mut result = php.to_string();
        for (start, end, text) in edits {
            result.replace_range(start..end, text);
        }
        result
    }

    // ── Basic promotion ─────────────────────────────────────────────────

    #[test]
    fn promotes_private_property() {
        let php = "\
<?php
class Foo {
    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("private string $name)"),
            "parameter should have visibility prefix: {result}"
        );
        assert!(
            !result.contains("private string $name;"),
            "property declaration should be removed: {result}"
        );
        assert!(
            !result.contains("$this->name = $name;"),
            "assignment should be removed: {result}"
        );
    }

    #[test]
    fn promotes_protected_property() {
        let php = "\
<?php
class Foo {
    protected int $age;

    public function __construct(int $age) {
        $this->age = $age;
    }
}
";
        let pos = php.find("int $age)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("protected int $age)"),
            "should use protected visibility: {result}"
        );
    }

    #[test]
    fn promotes_public_property() {
        let php = "\
<?php
class Foo {
    public string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("public string $name)"),
            "should use public visibility: {result}"
        );
    }

    // ── Readonly ────────────────────────────────────────────────────────

    #[test]
    fn promotes_readonly_property() {
        let php = "\
<?php
class Foo {
    private readonly string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("private readonly string $name)"),
            "should include readonly: {result}"
        );
    }

    // ── Default value carry-over ────────────────────────────────────────

    #[test]
    fn carries_over_default_value() {
        let php = "\
<?php
class Foo {
    private string $status = 'active';

    public function __construct(string $status) {
        $this->status = $status;
    }
}
";
        let pos = php.find("string $status)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("private string $status = 'active')"),
            "should carry default value: {result}"
        );
    }

    #[test]
    fn does_not_duplicate_default_value() {
        let php = "\
<?php
class Foo {
    private string $status = 'active';

    public function __construct(string $status = 'active') {
        $this->status = $status;
    }
}
";
        // Find the parameter occurrence (inside __construct), not the property.
        let construct_pos = php.find("__construct").unwrap();
        let pos = (construct_pos + php[construct_pos..].find("string $status").unwrap()) as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        let count = result.matches("= 'active'").count();
        assert_eq!(count, 1, "should not duplicate default: {result}");
    }

    // ── Attribute carry-over ────────────────────────────────────────────

    #[test]
    fn carries_over_attribute() {
        let php = "\
<?php
class Foo {
    #[SomeAttr]
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[SomeAttr] private int $bar)"),
            "attribute should move onto the parameter: {result}"
        );
        assert_eq!(
            result.matches("#[SomeAttr]").count(),
            1,
            "attribute should not be duplicated: {result}"
        );
    }

    #[test]
    fn carries_over_attribute_with_arguments() {
        let php = "\
<?php
class Foo {
    #[Column(type: 'integer', nullable: true)]
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[Column(type: 'integer', nullable: true)] private int $bar)"),
            "arguments should carry over verbatim: {result}"
        );
    }

    #[test]
    fn carries_over_multiple_attribute_lists() {
        let php = "\
<?php
class Foo {
    #[First]
    #[Second]
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[First] #[Second] private int $bar)"),
            "both attributes should carry over on one line: {result}"
        );
    }

    #[test]
    fn splits_grouped_attributes_into_separate_groups() {
        let php = "\
<?php
class Foo {
    #[First, Second]
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[First] #[Second] private int $bar)"),
            "a grouped list should become one group per attribute: {result}"
        );
    }

    #[test]
    fn carries_attribute_ahead_of_readonly() {
        let php = "\
<?php
class Foo {
    #[SomeAttr]
    private readonly int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[SomeAttr] private readonly int $bar)"),
            "attributes must precede the modifiers: {result}"
        );
    }

    #[test]
    fn carries_attribute_when_parameter_has_no_type_hint() {
        let php = "\
<?php
class Foo {
    #[SomeAttr]
    private $bar;

    public function __construct($bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("__construct($bar").unwrap() as u32 + 13;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[SomeAttr] private $bar)"),
            "attribute should sit before the visibility keyword: {result}"
        );
    }

    #[test]
    fn skips_attribute_the_parameter_already_carries() {
        // Repeating an attribute that is not `IS_REPEATABLE` is a fatal
        // error, so the duplicate must be dropped rather than emitted.
        let php = "\
<?php
class Foo {
    #[SomeAttr]
    private int $bar;

    public function __construct(#[SomeAttr] int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("] int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert_eq!(
            result.matches("#[SomeAttr]").count(),
            1,
            "the attribute should appear exactly once: {result}"
        );
        assert!(
            result.contains("#[SomeAttr] private int $bar)"),
            "the parameter's own attribute should be kept: {result}"
        );
    }

    #[test]
    fn keeps_a_parameter_attribute_the_property_lacks() {
        let php = "\
<?php
class Foo {
    #[FromProperty]
    private int $bar;

    public function __construct(#[FromParam] int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("] int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("#[FromParam] #[FromProperty] private int $bar)"),
            "both attributes should survive, in source order: {result}"
        );
    }

    #[test]
    fn removes_docblock_but_keeps_attribute() {
        let php = "\
<?php
class Foo {
    /** @var int */
    #[SomeAttr]
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
        let pos = php.find("int $bar)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        let result = apply_candidate(php, &c);
        assert!(
            !result.contains("@var int"),
            "the docblock should go with the property: {result}"
        );
        assert!(
            result.contains("#[SomeAttr] private int $bar)"),
            "the attribute should carry over: {result}"
        );
    }

    // ── No visibility on property ───────────────────────────────────────

    #[test]
    fn defaults_to_public_when_no_visibility() {
        let php = "\
<?php
class Foo {
    var $name;

    public function __construct($name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("$name)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate");
        assert!(
            c.prefix.starts_with("public"),
            "should default to public: {}",
            c.prefix
        );
    }

    // ── Rejection cases ─────────────────────────────────────────────────

    #[test]
    fn rejects_when_not_constructor() {
        let php = "\
<?php
class Foo {
    private string $name;

    public function setName(string $name): void {
        $this->name = $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer for non-constructor methods");
    }

    #[test]
    fn rejects_when_already_promoted() {
        let php = "\
<?php
class Foo {
    public function __construct(private string $name) {}
}
";
        let pos = php.find("private string $name").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer for already-promoted params");
    }

    #[test]
    fn rejects_when_no_matching_property() {
        let php = "\
<?php
class Foo {
    public function __construct(string $name) {
        echo $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer when no matching property");
    }

    #[test]
    fn rejects_when_no_assignment() {
        let php = "\
<?php
class Foo {
    private string $name;

    public function __construct(string $name) {
        echo $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer when no assignment found");
    }

    #[test]
    fn rejects_when_param_used_elsewhere() {
        let php = "\
<?php
class Foo {
    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
        echo $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(
            c.is_none(),
            "should not offer when param is used elsewhere in the body"
        );
    }

    #[test]
    fn rejects_multi_variable_declaration() {
        // Promoting `$name` from a shared declaration would delete `$other`
        // too, so the action must decline rather than drop a property.
        let php = "\
<?php
class Foo {
    private int $name, $other;

    public function __construct(int $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("int $name)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(
            c.is_none(),
            "should not offer for multi-variable property declarations"
        );
    }

    #[test]
    fn rejects_multi_variable_declaration_second_item() {
        // The rejection must hold regardless of which variable in the
        // shared declaration is being promoted: promoting `$other` here
        // would otherwise delete `$name` too.
        let php = "\
<?php
class Foo {
    private int $name, $other;

    public function __construct(int $other) {
        $this->other = $other;
    }
}
";
        let pos = php.find("int $other)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(
            c.is_none(),
            "should not offer when promoting a later variable in a shared declaration"
        );
    }

    #[test]
    fn rejects_static_property() {
        let php = "\
<?php
class Foo {
    private static string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer for static properties");
    }

    #[test]
    fn rejects_variadic_param() {
        let php = "\
<?php
class Foo {
    private array $items;

    public function __construct(string ...$items) {
        $this->items = $items;
    }
}
";
        let pos = php.find("string ...$items").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer for variadic params");
    }

    // ── Cursor outside constructor ──────────────────────────────────────

    #[test]
    fn rejects_cursor_outside_class() {
        let php = "<?php\nfunction foo(string $name) {}";
        let pos = php.find("string $name").unwrap() as u32;
        let c = find_candidate(php, pos);
        assert!(c.is_none(), "should not offer outside a class");
    }

    // ── Multiple parameters ─────────────────────────────────────────────

    #[test]
    fn promotes_correct_parameter_among_multiple() {
        let php = "\
<?php
class Foo {
    private string $name;
    private int $age;

    public function __construct(string $name, int $age) {
        $this->name = $name;
        $this->age = $age;
    }
}
";
        let pos = php.find("int $age)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should find candidate for $age");
        let result = apply_candidate(php, &c);
        // $age should be promoted, $name should remain as-is.
        assert!(
            result.contains("private int $age)"),
            "should promote $age: {result}"
        );
        assert!(
            result.contains("private string $name;"),
            "$name property should remain: {result}"
        );
        assert!(
            result.contains("$this->name = $name;"),
            "$name assignment should remain: {result}"
        );
        assert!(
            !result.contains("$this->age = $age;"),
            "$age assignment should be removed: {result}"
        );
    }

    // ── In namespace ────────────────────────────────────────────────────

    #[test]
    fn works_in_namespace() {
        let php = "\
<?php
namespace App;

class Foo {
    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("string $name)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should work in namespace");
        let result = apply_candidate(php, &c);
        assert!(result.contains("private string $name)"));
    }

    // ── count_occurrences ───────────────────────────────────────────────

    #[test]
    fn count_occurrences_basic() {
        assert_eq!(count_occurrences("$a + $a + $a", "$a"), 3);
    }

    #[test]
    fn count_occurrences_no_match() {
        assert_eq!(count_occurrences("$abc", "$a"), 0);
    }

    #[test]
    fn count_occurrences_at_end() {
        assert_eq!(count_occurrences("x = $name", "$name"), 1);
    }

    #[test]
    fn count_occurrences_not_prefix() {
        assert_eq!(count_occurrences("$names + $name", "$name"), 1);
    }

    // ── Union types ─────────────────────────────────────────────────────

    #[test]
    fn promotes_with_union_type() {
        let php = "\
<?php
class Foo {
    private int|string $id;

    public function __construct(int|string $id) {
        $this->id = $id;
    }
}
";
        let pos = php.find("int|string $id)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should handle union types");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("private int|string $id)"),
            "should preserve union type: {result}"
        );
    }

    // ── Nullable type ───────────────────────────────────────────────────

    #[test]
    fn promotes_with_nullable_type() {
        let php = "\
<?php
class Foo {
    private ?string $name;

    public function __construct(?string $name) {
        $this->name = $name;
    }
}
";
        let pos = php.find("?string $name)").unwrap() as u32;
        let c = find_candidate(php, pos).expect("should handle nullable types");
        let result = apply_candidate(php, &c);
        assert!(
            result.contains("private ?string $name)"),
            "should preserve nullable type: {result}"
        );
    }
}

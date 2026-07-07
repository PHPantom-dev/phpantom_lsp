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
use bumpalo::Bump;
use mago_span::HasSpan;
use mago_syntax::ast::class_like::member::ClassLikeMember;
use mago_syntax::ast::class_like::method::MethodBody;
use mago_syntax::ast::class_like::property::{PlainProperty, Property, PropertyItem};
use mago_syntax::ast::modifier::Modifier;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::util::offset_to_position;

// ── Data types ──────────────────────────────────────────────────────────────

/// All the information needed to generate the promotion edit.
struct PromotionCandidate {
    /// The text to insert before the parameter's type hint (or variable
    /// if there is no type hint) — e.g. `"private "`, `"public readonly "`.
    prefix: String,
    /// Byte span of the property declaration to delete (includes any
    /// preceding docblock and trailing newline).
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

        let cursor_offset = crate::util::position_to_offset(content, params.range.start);

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

        // 1. Delete the property declaration.
        edits.push(TextEdit {
            range: Range {
                start: offset_to_position(content, candidate.property_delete_start),
                end: offset_to_position(content, candidate.property_delete_end),
            },
            new_text: String::new(),
        });

        // 2. Delete the assignment statement.
        edits.push(TextEdit {
            range: Range {
                start: offset_to_position(content, candidate.assignment_delete_start),
                end: offset_to_position(content, candidate.assignment_delete_end),
            },
            new_text: String::new(),
        });

        // 3. Insert the visibility prefix before the parameter.
        edits.push(TextEdit {
            range: Range {
                start: offset_to_position(content, candidate.param_insert_offset),
                end: offset_to_position(content, candidate.param_insert_offset),
            },
            new_text: candidate.prefix,
        });

        // 4. Carry over default value from property if needed.
        if let (Some(default_text), Some(offset)) =
            (candidate.carry_default, candidate.default_insert_offset)
        {
            edits.push(TextEdit {
                range: Range {
                    start: offset_to_position(content, offset),
                    end: offset_to_position(content, offset),
                },
                new_text: default_text,
            });
        }

        let mut changes = std::collections::HashMap::new();
        changes.insert(doc_uri, edits);

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Promote to constructor property".to_string(),
            kind: Some(CodeActionKind::new("refactor.rewrite")),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
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

    // Must be __construct.
    if method.name.value != b"__construct" {
        return None;
    }

    // Must have a concrete body.
    let body = match &method.body {
        MethodBody::Concrete(block) => block,
        MethodBody::Abstract(_) => return None,
    };

    // Find the parameter under the cursor.
    let param = method.parameter_list.parameters.iter().find(|p| {
        let span = p.span();
        cursor >= span.start.offset && cursor <= span.end.offset
    })?;

    // Must not already be promoted.
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

    // Find the matching property declaration in the class body.
    let (property, plain_prop) = find_matching_property(all_members, bare_name)?;

    // Only promote a property declared on its own. A multi-variable
    // declaration (`private int $a, $b;`) is a single statement whose span
    // covers every variable, so deleting it would silently drop the
    // siblings. Decline the action rather than corrupt the declaration.
    if plain_prop.items.len() != 1 {
        return None;
    }

    // Property must not be static.
    if is_static(plain_prop.modifiers.iter()) {
        return None;
    }

    // Extract property visibility and readonly status.
    let visibility = extract_visibility_keyword(plain_prop.modifiers.iter());
    let is_readonly = has_readonly(plain_prop.modifiers.iter());

    // Build the prefix string.
    let vis_str = visibility.unwrap_or("public");
    let prefix = if is_readonly {
        format!("{vis_str} readonly ")
    } else {
        format!("{vis_str} ")
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

    // Determine the insertion offset for the visibility prefix.
    // Insert before the type hint if present, otherwise before the variable.
    let param_insert_offset = if let Some(hint) = &param.hint {
        hint.span().start.offset as usize
    } else {
        param.variable.span.start.offset as usize
    };

    // Determine the property deletion span (include leading whitespace /
    // docblock and trailing newline).
    let prop_span = property.span();
    let property_delete_start = find_line_start(content, prop_span.start.offset as usize);
    let property_delete_end = find_line_end(content, prop_span.end.offset as usize);

    // Check if the property has a default value that the parameter lacks.
    let (carry_default, default_insert_offset) = if param.default_value.is_none() {
        if let Some(default_text) = extract_property_default(plain_prop, content) {
            // Insert after the parameter variable name.
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

/// Extract the visibility keyword string from a modifier list.
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

/// Check if the modifier list includes `static`.
fn is_static<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers
        .into_iter()
        .any(|m| matches!(m, Modifier::Static(_)))
}

/// Check if the modifier list includes `readonly`.
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

/// Find the start of the line containing `offset` (the position after the
/// preceding newline, or 0 if at the start of the file).
fn find_line_start(content: &str, offset: usize) -> usize {
    let bytes = content.as_bytes();
    let mut pos = offset;
    while pos > 0 {
        let prev = bytes[pos - 1];
        if prev == b'\n' {
            break;
        }
        pos -= 1;
    }
    pos
}

/// Find the end of the line containing `offset` (past the trailing newline).
fn find_line_end(content: &str, offset: usize) -> usize {
    let bytes = content.as_bytes();
    let mut pos = offset;
    while pos < bytes.len() {
        if bytes[pos] == b'\n' {
            return pos + 1;
        }
        if bytes[pos] == b'\r' {
            pos += 1;
            if pos < bytes.len() && bytes[pos] == b'\n' {
                pos += 1;
            }
            return pos;
        }
        pos += 1;
    }
    pos
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
        let arena = Box::leak(Box::new(Bump::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());
        let ctx = find_cursor_context(&program.statements, offset);
        find_promotion_candidate(&ctx, php, offset)
    }

    /// Helper: apply a candidate's edits to the source text and return the result.
    fn apply_candidate(php: &str, c: &PromotionCandidate) -> String {
        // Collect all edits as (start, end, new_text) sorted by start descending
        // so we can apply them back-to-front without invalidating offsets.
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
        // Should not have two defaults.
        let count = result.matches("= 'active'").count();
        assert_eq!(count, 1, "should not duplicate default: {result}");
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
        // Put cursor on $age parameter.
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

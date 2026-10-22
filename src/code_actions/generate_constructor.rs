//! "Generate constructor" code action.
//!
//! When the cursor is on a property declaration inside a class that has
//! no `__construct` method, this module offers two code actions:
//!
//! 1. **Generate constructor** — inserts a traditional `__construct`
//!    with parameters and `$this->name = $name;` assignments.
//! 2. **Generate promoted constructor** — removes the property
//!    declarations and inserts a constructor with promoted parameters
//!    (`public string $name`, etc.), requiring no body assignments.
//!
//! **Code action kind:** `refactor.rewrite` (both).

#[cfg(test)]
use mago_allocator::LocalArena;
use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::class_like::property::{Property, PropertyItem};
use mago_syntax::cst::modifier::Modifier;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use super::detect_indent_from_members;
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::docblock::{extract_var_type, get_docblock_text_for_node};
use crate::parser::extract_hint_type;
use crate::php_type::{PhpType, TypeKind};
use crate::text_position::{
    byte_range_to_lsp_range, find_line_end, find_line_start, offset_to_position,
};

// ── Data types ──────────────────────────────────────────────────────────────

/// A property that qualifies for inclusion in the generated constructor.
struct QualifyingProperty {
    /// Property name without the `$` prefix.
    name: String,
    /// Structured type hint for the constructor parameter, if available.
    type_hint: Option<PhpType>,
    /// Default value text (e.g. `'active'`, `[]`), if the property has one.
    default_value: Option<String>,
    /// Visibility keyword (`"public"`, `"protected"`, `"private"`).
    /// Falls back to `"public"` when none is declared.
    visibility: &'static str,
    is_readonly: bool,
    /// Byte span of the entire property declaration (for deletion in the
    /// promoted variant).  `(start, end)` where `end` is past the trailing
    /// newline so that removing the declaration leaves no blank line.
    declaration_span: (usize, usize),
}

// ── Public API ──────────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Generate constructor" and "Generate promoted constructor"
    /// code actions for the cursor position.
    ///
    /// When the cursor is on a non-static property declaration inside a
    /// class body that has no existing `__construct` method, this produces
    /// up to two code actions that insert a constructor.
    pub(crate) fn collect_generate_constructor_actions(
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

        // Resolve the cursor context and gather the (owned) data needed to
        // build the edits.  The borrowed AST does not escape the closure.
        let Some((props, indent, insert_offset)) = crate::parser::with_parsed_program(
            content,
            "generate_constructor",
            |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);

                let all_members = match &ctx {
                    CursorContext::InClassLike {
                        member: MemberContext::Property(prop),
                        all_members,
                        ..
                    } => {
                        // Only offer on non-static properties — static
                        // properties won't be included in the constructor.
                        if prop.modifiers().iter().any(|m| m.is_static()) {
                            return None;
                        }
                        *all_members
                    }
                    _ => return None,
                };

                if has_constructor(all_members) {
                    return None;
                }

                let trivia = program.trivia.as_slice();

                let props = collect_qualifying_properties(all_members, content, trivia);
                if props.is_empty() {
                    return None;
                }

                let indent = detect_indent_from_members(all_members, content);

                let insert_offset = find_insertion_offset(all_members, content);
                Some((props, indent, insert_offset))
            },
        ) else {
            return;
        };

        let insert_pos = offset_to_position(content, insert_offset);

        // ── Traditional constructor ─────────────────────────────────────
        {
            let constructor_text = build_constructor(&props, &indent);

            let edit = TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: constructor_text,
            };

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate constructor".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                diagnostics: None,
                edit: Some(crate::code_actions::single_file_edit(
                    doc_uri.clone(),
                    vec![edit],
                )),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }

        // ── Promoted constructor ────────────────────────────────────────
        {
            let constructor_text = build_promoted_constructor(&props, &indent);

            // Build the list of edits: one deletion per property
            // declaration, plus one insertion for the constructor.
            // Sort deletions back-to-front so byte offsets stay valid.
            let mut edits: Vec<TextEdit> = Vec::new();

            // Properties declared together (`private int $a, $b;`) share
            // one declaration span and are adjacent in `props`; delete it
            // once, since clients reject overlapping edits.
            let mut last_span = None;
            for prop in props.iter().rev() {
                if last_span.replace(prop.declaration_span) == Some(prop.declaration_span) {
                    continue;
                }
                edits.push(TextEdit {
                    range: byte_range_to_lsp_range(
                        content,
                        prop.declaration_span.0,
                        prop.declaration_span.1,
                    ),
                    new_text: String::new(),
                });
            }

            // Insert the constructor after all property declarations
            // (same position as the traditional constructor).  This
            // keeps static properties above the constructor rather
            // than displacing them below it.
            edits.push(TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: constructor_text,
            });

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate promoted constructor".to_string(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                diagnostics: None,
                edit: Some(crate::code_actions::single_file_edit(doc_uri, edits)),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Check whether the class already has a `__construct` method.
fn has_constructor<'a>(members: &Sequence<'a, ClassLikeMember<'a>>) -> bool {
    members.iter().any(|m| {
        if let ClassLikeMember::Method(method) = m {
            method.name.value.eq_ignore_ascii_case(b"__construct")
        } else {
            false
        }
    })
}

/// Collect all non-static properties from the class members,
/// in declaration order.  Readonly properties are included because they
/// *must* be initialized in the constructor.
fn collect_qualifying_properties<'a>(
    members: &Sequence<'a, ClassLikeMember<'a>>,
    content: &str,
    trivia: &[Trivia<'a>],
) -> Vec<QualifyingProperty> {
    let mut result = Vec::new();

    for member in members.iter() {
        let member_prop = match member {
            ClassLikeMember::Property(p) => p,
            _ => continue,
        };
        let plain = match member_prop {
            Property::Plain(p) => p,
            _ => continue,
        };

        if is_static(member_prop) {
            continue;
        }

        let visibility = extract_visibility(plain.modifiers.iter());
        let is_readonly = has_readonly(plain.modifiers.iter());

        let native_hint = plain.hint.as_ref().map(|h| extract_hint_type(h));

        // Try to get a docblock @var type if there's no native hint
        // or if we want to use it as a fallback.
        let docblock_type =
            get_docblock_text_for_node(trivia, content, plain).and_then(extract_var_type);

        // Compute the declaration span for deletion (promoted variant).
        // Start from the beginning of the line containing the property
        // (to include leading whitespace), end past the trailing newline.
        // A `/** @var … */` docblock above the declaration goes with it;
        // left behind it would document whatever member follows.
        let prop_span = member.span();
        let line_start = find_line_start(content, prop_span.start.offset as usize);
        let decl_start = super::docblock_edit::docblock_start_above_offset(content, line_start)
            .unwrap_or(line_start);
        let decl_end = find_line_end(content, prop_span.end.offset as usize);

        for item in plain.items.iter() {
            let var_name = bytes_to_str(item.variable().name);
            let bare_name = var_name.strip_prefix('$').unwrap_or(var_name);

            let type_hint = if let Some(ref hint) = native_hint {
                Some(hint.clone())
            } else if let Some(ref doc_type) = docblock_type {
                // Only use docblock type if it's a single, non-compound type.
                // Skip complex types like `array{key: value}` or `int|string`.
                if is_simple_php_type(doc_type) {
                    Some(doc_type.clone())
                } else {
                    None::<PhpType>
                }
            } else {
                None
            };

            let default_value = if let PropertyItem::Concrete(concrete) = item {
                let span = concrete.value.span();
                let start = span.start.offset as usize;
                let end = span.end.offset as usize;
                content.get(start..end).map(|s| s.trim().to_string())
            } else {
                None
            };

            result.push(QualifyingProperty {
                name: bare_name.to_string(),
                type_hint,
                default_value,
                visibility,
                is_readonly,
                declaration_span: (decl_start, decl_end),
            });
        }
    }

    result
}

/// Check whether a [`PhpType`] is a simple (non-compound) type suitable
/// for use as a parameter type hint.
///
/// Accepts `Named` and `Nullable(Named)` types; rejects unions,
/// intersections, array shapes, generics, etc.  A late-static type is
/// rejected as well: PHP allows `static` as a return type only, and its
/// bounded display form (`static(App\Foo)`) is not source syntax at all, so
/// writing either into a parameter list would not parse.
fn is_simple_php_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(_) => true,
        TypeKind::Nullable(inner) => matches!(inner.kind(), TypeKind::Named(_)),
        _ => false,
    }
}

/// Find the byte offset where the constructor should be inserted.
///
/// The constructor is inserted after the last property declaration and
/// before any methods or other non-property members.  If there are no
/// properties before any methods, it's inserted after the class opening
/// brace.
fn find_insertion_offset<'a>(members: &Sequence<'a, ClassLikeMember<'a>>, content: &str) -> usize {
    let mut last_property_end: Option<u32> = None;
    let mut first_non_property_start: Option<u32> = None;

    for member in members.iter() {
        match member {
            ClassLikeMember::Property(_) => {
                let span = member.span();
                last_property_end = Some(span.end.offset);
            }
            ClassLikeMember::Method(_)
            | ClassLikeMember::Constant(_)
            | ClassLikeMember::TraitUse(_)
            | ClassLikeMember::EnumCase(_) => {
                if first_non_property_start.is_none() && last_property_end.is_some() {
                    first_non_property_start = Some(member.span().start.offset);
                }
                // If we haven't seen any properties yet, record this as
                // the first non-property so we know where to insert
                // relative to the class opening brace.
                if last_property_end.is_none() && first_non_property_start.is_none() {
                    first_non_property_start = Some(member.span().start.offset);
                }
            }
        }
    }

    if let Some(end) = last_property_end {
        // Insert after the last property.  Find the end of the line
        // containing the property's semicolon.
        find_line_end(content, end as usize)
    } else {
        // No properties at all — shouldn't happen because we check for
        // qualifying properties, but handle gracefully.
        0
    }
}

/// Build the traditional constructor source text from the qualifying
/// properties.  Each property becomes a parameter with matching type
/// hint and an assignment in the body.
fn build_constructor(props: &[QualifyingProperty], indent: &str) -> String {
    let mut result = String::new();

    result.push('\n');
    result.push_str(indent);
    result.push_str("public function __construct(");

    // Build parameter list.
    // Parameters with default values must come after required parameters.
    // We preserve declaration order but PHP requires defaults at the end,
    // so we separate them.
    let params = required_first_params(props, |prop| match prop.type_hint {
        Some(ref hint) => format!("{hint} ${}", prop.name),
        None => format!("${}", prop.name),
    });
    result.push_str(&params.join(", "));
    result.push_str(")\n");
    result.push_str(indent);
    result.push_str("{\n");

    // Build assignment body — use declaration order for assignments,
    // not the reordered parameter order.
    for prop in props {
        result.push_str(indent);
        result.push_str(indent);
        result.push_str("$this->");
        result.push_str(&prop.name);
        result.push_str(" = $");
        result.push_str(&prop.name);
        result.push_str(";\n");
    }

    result.push_str(indent);
    result.push_str("}\n");

    result
}

/// One constructor parameter per property, rendered by `declare` with the
/// property's default appended, the ones without a default first: PHP
/// deprecates a required parameter after an optional one.
fn required_first_params(
    props: &[QualifyingProperty],
    declare: impl Fn(&QualifyingProperty) -> String,
) -> Vec<String> {
    let required = props.iter().filter(|prop| prop.default_value.is_none());
    let optional = props.iter().filter(|prop| prop.default_value.is_some());
    required
        .chain(optional)
        .map(|prop| {
            let mut param = declare(prop);
            if let Some(ref default) = prop.default_value {
                param.push_str(" = ");
                param.push_str(default);
            }
            param
        })
        .collect()
}

/// Build the promoted constructor source text.  Each property becomes a
/// promoted parameter (`visibility [readonly] type $name [= default]`)
/// and the property declarations are removed by the caller.
fn build_promoted_constructor(props: &[QualifyingProperty], indent: &str) -> String {
    let mut result = String::new();

    result.push('\n');
    result.push_str(indent);
    result.push_str("public function __construct(\n");

    // Parameters with default values must come after required parameters.
    let params = required_first_params(props, |prop| {
        let mut param = format!("{indent}{indent}{}", prop.visibility);
        if prop.is_readonly {
            param.push_str(" readonly");
        }
        if let Some(ref hint) = prop.type_hint {
            param.push(' ');
            param.push_str(&hint.to_string());
        }
        param.push_str(" $");
        param.push_str(&prop.name);
        param
    });
    result.push_str(&params.join(",\n"));
    result.push_str(",\n");

    result.push_str(indent);
    result.push_str(") {}\n");

    result
}

/// Extract the visibility keyword from a modifier list, defaulting to
/// `"public"` if none is present.
fn extract_visibility<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> &'static str {
    for m in modifiers {
        match m {
            Modifier::Public(_) => return "public",
            Modifier::Protected(_) => return "protected",
            Modifier::Private(_) => return "private",
            _ => continue,
        }
    }
    "public"
}

fn has_readonly<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers
        .into_iter()
        .any(|m| matches!(m, Modifier::Readonly(_)))
}

fn is_static(property: &Property<'_>) -> bool {
    property.modifiers().iter().any(|m| m.is_static())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;

    // ── is_simple_type ──────────────────────────────────────────────────

    #[test]
    fn simple_type_accepts_basic() {
        assert!(is_simple_php_type(&PhpType::parse("string")));
        assert!(is_simple_php_type(&PhpType::parse("int")));
        assert!(is_simple_php_type(&PhpType::parse("array")));
        assert!(is_simple_php_type(&PhpType::parse("bool")));
    }

    #[test]
    fn simple_type_accepts_nullable() {
        assert!(is_simple_php_type(&PhpType::parse("?string")));
        assert!(is_simple_php_type(&PhpType::parse("?Foo")));
    }

    #[test]
    fn simple_type_accepts_fqn() {
        assert!(is_simple_php_type(&PhpType::parse("App\\Models\\User")));
        assert!(is_simple_php_type(&PhpType::parse("?App\\Models\\User")));
    }

    /// `static` is not a valid parameter type in PHP, and the bounded display
    /// form is not source syntax, so neither may reach a generated signature.
    #[test]
    fn simple_type_rejects_late_static() {
        assert!(!is_simple_php_type(&PhpType::parse("static(App\\Foo)")));
        assert!(!is_simple_php_type(&PhpType::parse("$this(App\\Foo)")));
        assert!(!is_simple_php_type(&PhpType::static_type(atom("App\\Foo"))));
        assert!(!is_simple_php_type(&PhpType::nullable(PhpType::this_type(
            atom("App\\Foo")
        ))));
    }

    #[test]
    fn simple_type_rejects_union() {
        assert!(!is_simple_php_type(&PhpType::parse("int|string")));
    }

    #[test]
    fn simple_type_rejects_intersection() {
        assert!(!is_simple_php_type(&PhpType::parse("Foo&Bar")));
    }

    #[test]
    fn simple_type_rejects_array_shape() {
        assert!(!is_simple_php_type(&PhpType::parse("array{name: string}")));
    }

    #[test]
    fn simple_type_rejects_generic() {
        assert!(!is_simple_php_type(&PhpType::parse("Collection<User>")));
    }

    #[test]
    fn simple_type_rejects_empty() {
        assert!(!is_simple_php_type(&PhpType::parse("")));
    }

    // ── build_constructor ───────────────────────────────────────────────

    fn prop(name: &str, type_hint: Option<&str>, default: Option<&str>) -> QualifyingProperty {
        QualifyingProperty {
            name: name.to_string(),
            type_hint: type_hint.map(PhpType::parse),
            default_value: default.map(|s| s.to_string()),
            visibility: "public",
            is_readonly: false,
            declaration_span: (0, 0),
        }
    }

    #[test]
    fn builds_basic_constructor() {
        let props = vec![
            prop("name", Some("string"), None),
            prop("age", Some("int"), None),
        ];

        let result = build_constructor(&props, "    ");
        assert!(result.contains("public function __construct(string $name, int $age)"));
        assert!(result.contains("$this->name = $name;"));
        assert!(result.contains("$this->age = $age;"));
    }

    #[test]
    fn builds_constructor_with_defaults() {
        let props = vec![
            prop("name", Some("string"), None),
            prop("status", Some("string"), Some("'active'")),
        ];

        let result = build_constructor(&props, "    ");
        assert!(
            result.contains("string $name, string $status = 'active'"),
            "required params before optional: {result}"
        );
    }

    #[test]
    fn defaults_reordered_before_required() {
        let props = vec![
            prop("status", Some("string"), Some("'draft'")),
            prop("name", Some("string"), None),
        ];

        let result = build_constructor(&props, "    ");
        // Required parameter $name should come before optional $status.
        let name_pos = result.find("$name").unwrap();
        let status_pos = result.find("$status").unwrap();
        assert!(
            name_pos < status_pos,
            "required params should come first: {result}"
        );
    }

    #[test]
    fn builds_constructor_without_type_hints() {
        let props = vec![prop("data", None, None)];

        let result = build_constructor(&props, "    ");
        assert!(
            result.contains("($data)"),
            "untyped param should not have type: {result}"
        );
    }

    #[test]
    fn builds_constructor_with_nullable_type() {
        let props = vec![prop("label", Some("?string"), None)];

        let result = build_constructor(&props, "    ");
        assert!(
            result.contains("?string $label"),
            "nullable type preserved: {result}"
        );
    }

    #[test]
    fn builds_constructor_with_union_type() {
        let props = vec![prop("id", Some("int|string"), None)];

        let result = build_constructor(&props, "    ");
        assert!(
            result.contains("int|string $id"),
            "union type preserved: {result}"
        );
    }

    #[test]
    fn respects_tab_indentation() {
        let props = vec![prop("name", Some("string"), None)];

        let result = build_constructor(&props, "\t");
        assert!(
            result.contains("\tpublic function __construct("),
            "should use tab indent: {result}"
        );
        assert!(
            result.contains("\t\t$this->name = $name;"),
            "body should use double tab: {result}"
        );
    }

    // ── build_promoted_constructor ───────────────────────────────────────

    fn pprop(
        name: &str,
        type_hint: Option<&str>,
        default: Option<&str>,
        visibility: &'static str,
        is_readonly: bool,
    ) -> QualifyingProperty {
        QualifyingProperty {
            name: name.to_string(),
            type_hint: type_hint.map(PhpType::parse),
            default_value: default.map(|s| s.to_string()),
            visibility,
            is_readonly,
            declaration_span: (0, 0),
        }
    }

    #[test]
    fn builds_promoted_constructor_basic() {
        let props = vec![
            pprop("name", Some("string"), None, "public", false),
            pprop("age", Some("int"), None, "private", false),
        ];

        let result = build_promoted_constructor(&props, "    ");
        assert!(
            result.contains("public string $name"),
            "should have public visibility: {result}"
        );
        assert!(
            result.contains("private int $age"),
            "should have private visibility: {result}"
        );
        assert!(result.contains(") {}"), "should have empty body: {result}");
        assert!(
            !result.contains("$this->"),
            "should not have assignments: {result}"
        );
    }

    #[test]
    fn builds_promoted_constructor_with_readonly() {
        let props = vec![pprop("id", Some("string"), None, "public", true)];

        let result = build_promoted_constructor(&props, "    ");
        assert!(
            result.contains("public readonly string $id"),
            "should have readonly modifier: {result}"
        );
    }

    #[test]
    fn builds_promoted_constructor_with_defaults() {
        let props = vec![
            pprop("name", Some("string"), None, "public", false),
            pprop(
                "status",
                Some("string"),
                Some("'active'"),
                "protected",
                false,
            ),
        ];

        let result = build_promoted_constructor(&props, "    ");
        // Required comes before optional.
        let name_pos = result.find("$name").unwrap();
        let status_pos = result.find("$status").unwrap();
        assert!(name_pos < status_pos, "required before optional: {result}");
        assert!(
            result.contains("protected string $status = 'active'"),
            "should carry over default: {result}"
        );
    }

    #[test]
    fn builds_promoted_constructor_trailing_comma() {
        let props = vec![pprop("name", Some("string"), None, "public", false)];

        let result = build_promoted_constructor(&props, "    ");
        // Should have a trailing comma after the last parameter.
        assert!(
            result.contains("$name,\n"),
            "should have trailing comma: {result}"
        );
    }

    #[test]
    fn builds_promoted_constructor_multiline() {
        let props = vec![
            pprop("name", Some("string"), None, "public", false),
            pprop("age", Some("int"), None, "private", false),
        ];

        let result = build_promoted_constructor(&props, "    ");
        // Each parameter should be on its own line.
        assert!(
            result.contains("        public string $name,\n        private int $age,\n"),
            "parameters should be on separate lines: {result}"
        );
    }

    #[test]
    fn promoted_constructor_tabs() {
        let props = vec![pprop("name", Some("string"), None, "public", false)];

        let result = build_promoted_constructor(&props, "\t");
        assert!(
            result.contains("\tpublic function __construct(\n"),
            "should use tab indent: {result}"
        );
        assert!(
            result.contains("\t\tpublic string $name,\n"),
            "params should use double tab: {result}"
        );
    }

    #[test]
    fn promoted_constructor_no_type_hint() {
        let props = vec![pprop("data", None, None, "private", false)];

        let result = build_promoted_constructor(&props, "    ");
        assert!(
            result.contains("private $data"),
            "untyped param should have no type hint: {result}"
        );
    }

    // ── has_constructor ─────────────────────────────────────────────────

    #[test]
    fn detects_existing_constructor() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public function __construct() {}\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            assert!(has_constructor(all_members));
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn detects_no_constructor() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $name;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            assert!(!has_constructor(all_members));
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn detects_constructor_case_insensitive() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public function __CONSTRUCT() {}\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            assert!(has_constructor(all_members));
        } else {
            panic!("should find class");
        }
    }

    // ── collect_qualifying_properties ───────────────────────────────────

    #[test]
    fn collects_non_static() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $name;\n    private int $age;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 2);
            assert_eq!(props[0].name, "name");
            assert_eq!(
                props[0]
                    .type_hint
                    .as_ref()
                    .map(|t| t.to_string())
                    .as_deref(),
                Some("string")
            );
            assert_eq!(props[0].visibility, "public");
            assert_eq!(props[1].name, "age");
            assert_eq!(
                props[1]
                    .type_hint
                    .as_ref()
                    .map(|t| t.to_string())
                    .as_deref(),
                Some("int")
            );
            assert_eq!(props[1].visibility, "private");
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn skips_static_properties() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $name;\n    public static int $count;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].name, "name");
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn includes_readonly_properties() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $name;\n    public readonly int $id;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 2);
            assert_eq!(props[0].name, "name");
            assert!(!props[0].is_readonly);
            assert_eq!(props[1].name, "id");
            assert_eq!(
                props[1]
                    .type_hint
                    .as_ref()
                    .map(|t| t.to_string())
                    .as_deref(),
                Some("int")
            );
            assert!(props[1].is_readonly);
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn extracts_default_values() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $status = 'active';\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].name, "status");
            assert_eq!(props[0].default_value.as_deref(), Some("'active'"));
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn extracts_docblock_type_when_no_native_hint() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    /** @var string */\n    public $name;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].name, "name");
            assert_eq!(
                props[0]
                    .type_hint
                    .as_ref()
                    .map(|t| t.to_string())
                    .as_deref(),
                Some("string")
            );
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn skips_compound_docblock_type() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    /** @var int|string */\n    public $id;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            assert_eq!(props[0].name, "id");
            assert!(
                props[0].type_hint.is_none(),
                "compound docblock type should be skipped"
            );
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn preserves_nullable_native_type() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public ?string $name;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            assert_eq!(
                props[0]
                    .type_hint
                    .as_ref()
                    .map(|t| t.to_string())
                    .as_deref(),
                Some("?string")
            );
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn preserves_union_native_type() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public int|string $id;\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            assert_eq!(
                props[0]
                    .type_hint
                    .as_ref()
                    .map(|t| t.to_string())
                    .as_deref(),
                Some("int|string")
            );
        } else {
            panic!("should find class");
        }
    }

    #[test]
    fn captures_declaration_span() {
        let arena = Box::leak(Box::new(LocalArena::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $name;\n}\n";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        if let CursorContext::InClassLike { all_members, .. } = &ctx {
            let props = collect_qualifying_properties(all_members, php, program.trivia.as_slice());
            assert_eq!(props.len(), 1);
            let (start, end) = props[0].declaration_span;
            let deleted = &php[start..end];
            assert!(
                deleted.contains("public string $name;"),
                "span should cover property declaration: {deleted:?}"
            );
        } else {
            panic!("should find class");
        }
    }
}

//! "Generate getter/setter" code action.
//!
//! When the cursor is on a property declaration inside a class, this
//! module offers up to three code actions:
//!
//! 1. **Generate getter** — inserts a `getX()` method (or `isX()` for
//!    `bool` properties) that returns the property value.
//! 2. **Generate setter** — inserts a `setX()` method that assigns the
//!    property value and returns `$this` for fluent chaining.
//! 3. **Generate getter and setter** — inserts both methods.
//!
//! **Code action kind:** `refactor`.
//!
//! Readonly properties only get a getter (no setter). Static properties
//! generate static methods. Promoted constructor properties are treated
//! the same as regular properties. If a `getX()` or `setX()` method
//! already exists, the corresponding action is not offered.

use std::collections::HashMap;

#[cfg(test)]
use bumpalo::Bump;
use mago_span::HasSpan;
use mago_syntax::ast::class_like::member::ClassLikeMember;
use mago_syntax::ast::class_like::property::Property;
use mago_syntax::ast::modifier::Modifier;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use super::detect_indent_from_members;
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::docblock::{extract_var_type, get_docblock_text_for_node};
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::util::offset_to_position;

// ── Data types ──────────────────────────────────────────────────────────────

/// A property for which getter/setter methods can be generated.
struct AccessorProperty {
    /// Property name without the `$` prefix.
    name: String,
    /// Structured type hint, if available (native or docblock).
    type_hint: Option<PhpType>,
    /// Whether the type hint came from a docblock rather than a native
    /// type declaration.  When `true`, generated methods use `@return` /
    /// `@param` tags instead of native type hints.
    type_from_docblock: bool,
    /// Whether the property has the `readonly` modifier.
    is_readonly: bool,
    /// Whether the property has the `static` modifier.
    is_static: bool,
}

/// Which accessors can be generated for a property, considering what
/// already exists.
struct AvailableAccessors {
    /// The property to generate accessors for.
    prop: AccessorProperty,
    /// Whether a getter can be generated (no existing `getX`/`isX`).
    can_getter: bool,
    /// Whether a setter can be generated (no existing `setX`, not readonly).
    can_setter: bool,
}

// ── Public API ──────────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Generate getter", "Generate setter", and
    /// "Generate getter and setter" code actions for the cursor position.
    ///
    /// When the cursor is on a property declaration inside a class body,
    /// this produces up to three code actions that insert accessor methods.
    pub(crate) fn collect_generate_getter_setter_actions(
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

        // Resolve the cursor context and gather the (owned) accessor data.
        // The borrowed AST does not escape the closure.
        let Some((available, indent, insert_offset)) = crate::parser::with_parsed_program(
            content,
            "generate_getter_setter",
            |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);

                let (property, all_members) = match &ctx {
                    CursorContext::InClassLike {
                        member: MemberContext::Property(prop),
                        all_members,
                        ..
                    } => (*prop, *all_members),
                    _ => return None,
                };

                let trivia = program.trivia.as_slice();

                // Collect properties from the declaration under the cursor.
                let props = collect_accessor_properties(property, content, trivia);
                if props.is_empty() {
                    return None;
                }

                // Check which methods already exist.
                let existing_methods = collect_existing_method_names(all_members);

                // Determine available accessors for each property.
                let mut available: Vec<AvailableAccessors> = Vec::new();
                for prop in props {
                    let getter_name = getter_method_name(&prop.name, prop.type_hint.as_ref());
                    let setter_name = setter_method_name(&prop.name);

                    let has_getter = existing_methods
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(&getter_name));
                    let has_setter = existing_methods
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(&setter_name));

                    // For bool properties, also check the `isX` variant.
                    let has_is = if is_bool_type(prop.type_hint.as_ref()) {
                        let is_name = is_method_name(&prop.name);
                        existing_methods
                            .iter()
                            .any(|m| m.eq_ignore_ascii_case(&is_name))
                    } else {
                        false
                    };

                    let can_getter = !has_getter && !has_is;
                    let can_setter = !has_setter && !prop.is_readonly;

                    if can_getter || can_setter {
                        available.push(AvailableAccessors {
                            prop,
                            can_getter,
                            can_setter,
                        });
                    }
                }

                if available.is_empty() {
                    return None;
                }

                // Detect indentation from existing class members.
                let indent = detect_indent_from_members(all_members, content);

                // Insertion point: after the last method, or after the
                // last member if there are no methods.
                let insert_offset = find_accessor_insertion_offset(all_members, content);
                Some((available, indent, insert_offset))
            },
        ) else {
            return;
        };

        let insert_pos = offset_to_position(content, insert_offset);

        let any_can_getter = available.iter().any(|a| a.can_getter);
        let any_can_setter = available.iter().any(|a| a.can_setter);

        // ── Generate getter ─────────────────────────────────────────────
        if any_can_getter {
            let mut methods_text = String::new();
            for avail in &available {
                if avail.can_getter {
                    methods_text.push_str(&build_getter(&avail.prop, &indent));
                }
            }

            let edit = TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: methods_text,
            };

            let mut changes = HashMap::new();
            changes.insert(doc_uri.clone(), vec![edit]);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate getter".to_string(),
                kind: Some(CodeActionKind::REFACTOR),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }

        // ── Generate setter ─────────────────────────────────────────────
        if any_can_setter {
            let mut methods_text = String::new();
            for avail in &available {
                if avail.can_setter {
                    methods_text.push_str(&build_setter(&avail.prop, &indent));
                }
            }

            let edit = TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: methods_text,
            };

            let mut changes = HashMap::new();
            changes.insert(doc_uri.clone(), vec![edit]);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate setter".to_string(),
                kind: Some(CodeActionKind::REFACTOR),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }

        // ── Generate getter and setter ──────────────────────────────────
        if any_can_getter && any_can_setter {
            let mut methods_text = String::new();
            for avail in &available {
                if avail.can_getter {
                    methods_text.push_str(&build_getter(&avail.prop, &indent));
                }
                if avail.can_setter {
                    methods_text.push_str(&build_setter(&avail.prop, &indent));
                }
            }

            let edit = TextEdit {
                range: Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                new_text: methods_text,
            };

            let mut changes = HashMap::new();
            changes.insert(doc_uri, vec![edit]);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate getter and setter".to_string(),
                kind: Some(CodeActionKind::REFACTOR),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Collect accessor-eligible properties from a single property declaration
/// node (which may declare multiple variables, e.g. `private int $a, $b;`).
fn collect_accessor_properties<'a>(
    property: &Property<'a>,
    content: &str,
    trivia: &[Trivia<'a>],
) -> Vec<AccessorProperty> {
    let mut result = Vec::new();

    match property {
        Property::Plain(plain) => {
            let is_readonly = has_readonly(plain.modifiers.iter());
            let is_static = has_static(plain.modifiers.iter());

            // Extract the native type hint for the property.
            let native_hint = plain.hint.as_ref().map(|h| extract_hint_type(h));

            // Try to get a docblock @var type if there's no native hint.
            let docblock_type =
                get_docblock_text_for_node(trivia, content, plain).and_then(extract_var_type);

            for item in plain.items.iter() {
                let var_name = bytes_to_str(item.variable().name);
                let bare_name = var_name.strip_prefix('$').unwrap_or(var_name);

                let (type_hint, type_from_docblock) = if let Some(ref hint) = native_hint {
                    (Some(hint.clone()), false)
                } else if let Some(ref doc_type) = docblock_type {
                    (Some(doc_type.clone()), true)
                } else {
                    (None, false)
                };

                result.push(AccessorProperty {
                    name: bare_name.to_string(),
                    type_hint,
                    type_from_docblock,
                    is_readonly,
                    is_static,
                });
            }
        }
        Property::Hooked(_) => {
            // Properties with hooks already have accessor behaviour.
            // Do not offer to generate getter/setter for them.
        }
    }

    result
}

/// Collect all method names in the class (lowercased for comparison).
fn collect_existing_method_names<'a>(members: &Sequence<'a, ClassLikeMember<'a>>) -> Vec<String> {
    members
        .iter()
        .filter_map(|m| {
            if let ClassLikeMember::Method(method) = m {
                Some(bytes_to_str(method.name.value).to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Convert a property name to PascalCase for use in method names.
///
/// `name` → `Name`, `first_name` → `FirstName`, `firstName` → `FirstName`.
fn to_pascal_case(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }

    // If it contains underscores, treat as snake_case.
    if name.contains('_') {
        return name
            .split('_')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        format!("{}{}", upper, chars.as_str())
                    }
                    None => String::new(),
                }
            })
            .collect();
    }

    // Simple case: just capitalize the first letter.
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{}{}", upper, chars.as_str())
        }
        None => String::new(),
    }
}

/// Compute the getter method name for a property.
///
/// Uses `get` + PascalCase. For `bool` properties, uses `is` + PascalCase
/// instead.
fn getter_method_name(prop_name: &str, type_hint: Option<&PhpType>) -> String {
    let pascal = to_pascal_case(prop_name);
    if is_bool_type(type_hint) {
        format!("is{pascal}")
    } else {
        format!("get{pascal}")
    }
}

/// Compute the `is`-prefixed method name for bool properties.
fn is_method_name(prop_name: &str) -> String {
    let pascal = to_pascal_case(prop_name);
    format!("is{pascal}")
}

/// Compute the setter method name for a property.
fn setter_method_name(prop_name: &str) -> String {
    let pascal = to_pascal_case(prop_name);
    format!("set{pascal}")
}

/// Check whether a type hint represents a boolean type.
///
/// Handles bare `bool`, `boolean`, and nullable `?bool` / `?boolean`.
fn is_bool_type(type_hint: Option<&PhpType>) -> bool {
    match type_hint {
        Some(t) => t.is_bool(),
        None => false,
    }
}

/// Build the getter method source text for a property.
fn build_getter(prop: &AccessorProperty, indent: &str) -> String {
    let mut result = String::new();
    let method_name = getter_method_name(&prop.name, prop.type_hint.as_ref());

    result.push('\n');

    // If the type came from a docblock, add a @return tag.
    if prop.type_from_docblock
        && let Some(ref hint) = prop.type_hint
    {
        let hint_str = hint.to_string();
        result.push_str(indent);
        result.push_str("/**\n");
        result.push_str(indent);
        result.push_str(" * @return ");
        result.push_str(&hint_str);
        result.push('\n');
        result.push_str(indent);
        result.push_str(" */\n");
    }

    result.push_str(indent);
    if prop.is_static {
        result.push_str("public static function ");
    } else {
        result.push_str("public function ");
    }
    result.push_str(&method_name);
    result.push('(');
    result.push(')');

    // Add native return type if we have a non-docblock type.
    if !prop.type_from_docblock
        && let Some(ref hint) = prop.type_hint
    {
        result.push_str(": ");
        result.push_str(&hint.to_string());
    }

    result.push('\n');
    result.push_str(indent);
    result.push_str("{\n");

    result.push_str(indent);
    result.push_str(indent);
    result.push_str("return ");
    if prop.is_static {
        result.push_str("self::$");
    } else {
        result.push_str("$this->");
    }
    result.push_str(&prop.name);
    result.push_str(";\n");

    result.push_str(indent);
    result.push_str("}\n");

    result
}

/// Build the setter method source text for a property.
///
/// The setter returns `$this` (or `static` for static) for fluent chaining,
/// with a return type of `self`.
fn build_setter(prop: &AccessorProperty, indent: &str) -> String {
    let mut result = String::new();
    let method_name = setter_method_name(&prop.name);

    result.push('\n');

    // If the type came from a docblock, add a @param tag.
    if prop.type_from_docblock
        && let Some(ref hint) = prop.type_hint
    {
        let hint_str = hint.to_string();
        result.push_str(indent);
        result.push_str("/**\n");
        result.push_str(indent);
        result.push_str(" * @param ");
        result.push_str(&hint_str);
        result.push_str(" $");
        result.push_str(&prop.name);
        result.push('\n');
        result.push_str(indent);
        result.push_str(" */\n");
    }

    result.push_str(indent);
    if prop.is_static {
        result.push_str("public static function ");
    } else {
        result.push_str("public function ");
    }
    result.push_str(&method_name);
    result.push('(');

    // Parameter with type hint.
    if !prop.type_from_docblock
        && let Some(ref hint) = prop.type_hint
    {
        result.push_str(&hint.to_string());
        result.push(' ');
    }
    result.push('$');
    result.push_str(&prop.name);
    result.push_str("): ");
    if prop.is_static {
        result.push_str("void");
    } else {
        result.push_str("self");
    }

    result.push('\n');
    result.push_str(indent);
    result.push_str("{\n");

    result.push_str(indent);
    result.push_str(indent);
    if prop.is_static {
        result.push_str("self::$");
    } else {
        result.push_str("$this->");
    }
    result.push_str(&prop.name);
    result.push_str(" = $");
    result.push_str(&prop.name);
    result.push_str(";\n");

    if !prop.is_static {
        result.push('\n');
        result.push_str(indent);
        result.push_str(indent);
        result.push_str("return $this;\n");
    }

    result.push_str(indent);
    result.push_str("}\n");

    result
}

/// Find the byte offset where accessor methods should be inserted.
///
/// Inserts after the last existing method in the class, or after the
/// last member if there are no methods.
fn find_accessor_insertion_offset<'a>(
    members: &Sequence<'a, ClassLikeMember<'a>>,
    content: &str,
) -> usize {
    // Prefer inserting after the last method.
    let mut last_method_end: Option<u32> = None;
    let mut last_member_end: Option<u32> = None;

    for member in members.iter() {
        let span = member.span();
        last_member_end = Some(span.end.offset);

        if matches!(member, ClassLikeMember::Method(_)) {
            last_method_end = Some(span.end.offset);
        }
    }

    let end = last_method_end.or(last_member_end).unwrap_or(0);
    find_line_end(content, end as usize)
}

/// Find the end of the line at or after the given offset (past the newline).
fn find_line_end(content: &str, offset: usize) -> usize {
    if let Some(nl) = content[offset..].find('\n') {
        offset + nl + 1
    } else {
        content.len()
    }
}

/// Check if the modifier list includes `readonly`.
fn has_readonly<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers
        .into_iter()
        .any(|m| matches!(m, Modifier::Readonly(_)))
}

/// Check if the modifier list includes `static`.
fn has_static<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers.into_iter().any(|m| m.is_static())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_pascal_case ──────────────────────────────────────────────────

    #[test]
    fn pascal_case_simple() {
        assert_eq!(to_pascal_case("name"), "Name");
    }

    #[test]
    fn pascal_case_already_capitalized() {
        assert_eq!(to_pascal_case("Name"), "Name");
    }

    #[test]
    fn pascal_case_snake_case() {
        assert_eq!(to_pascal_case("first_name"), "FirstName");
    }

    #[test]
    fn pascal_case_single_char() {
        assert_eq!(to_pascal_case("x"), "X");
    }

    #[test]
    fn pascal_case_empty() {
        assert_eq!(to_pascal_case(""), "");
    }

    #[test]
    fn pascal_case_camel_case_input() {
        assert_eq!(to_pascal_case("firstName"), "FirstName");
    }

    #[test]
    fn pascal_case_multiple_underscores() {
        assert_eq!(to_pascal_case("is_user_active"), "IsUserActive");
    }

    // ── getter_method_name / setter_method_name ─────────────────────────

    #[test]
    fn getter_name_for_string() {
        let ty = PhpType::parse("string");
        assert_eq!(getter_method_name("name", Some(&ty)), "getName");
    }

    #[test]
    fn getter_name_for_bool() {
        let ty = PhpType::parse("bool");
        assert_eq!(getter_method_name("active", Some(&ty)), "isActive");
    }

    #[test]
    fn getter_name_for_boolean() {
        let ty = PhpType::parse("boolean");
        assert_eq!(getter_method_name("active", Some(&ty)), "isActive");
    }

    #[test]
    fn getter_name_no_type() {
        assert_eq!(getter_method_name("data", None), "getData");
    }

    #[test]
    fn setter_name_simple() {
        assert_eq!(setter_method_name("name"), "setName");
    }

    #[test]
    fn setter_name_snake_case() {
        assert_eq!(setter_method_name("first_name"), "setFirstName");
    }

    // ── is_bool_type ────────────────────────────────────────────────────

    #[test]
    fn bool_type_recognized() {
        assert!(is_bool_type(Some(&PhpType::parse("bool"))));
        assert!(is_bool_type(Some(&PhpType::parse("boolean"))));
        assert!(is_bool_type(Some(&PhpType::parse("Bool"))));
    }

    #[test]
    fn non_bool_type_not_recognized() {
        assert!(!is_bool_type(Some(&PhpType::parse("string"))));
        assert!(!is_bool_type(Some(&PhpType::parse("int"))));
        assert!(!is_bool_type(None));
    }

    #[test]
    fn nullable_bool_type_recognized() {
        assert!(is_bool_type(Some(&PhpType::parse("?bool"))));
        assert!(is_bool_type(Some(&PhpType::parse("?boolean"))));
    }

    // ── build_getter ────────────────────────────────────────────────────

    fn make_prop(
        name: &str,
        type_hint: Option<&str>,
        type_from_docblock: bool,
        is_readonly: bool,
        is_static: bool,
    ) -> AccessorProperty {
        AccessorProperty {
            name: name.to_string(),
            type_hint: type_hint.map(PhpType::parse),
            type_from_docblock,
            is_readonly,
            is_static,
        }
    }

    #[test]
    fn builds_simple_getter() {
        let prop = make_prop("name", Some("string"), false, false, false);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("public function getName(): string"),
            "getter signature: {result}"
        );
        assert!(
            result.contains("return $this->name;"),
            "getter body: {result}"
        );
    }

    #[test]
    fn builds_bool_getter_with_is_prefix() {
        let prop = make_prop("active", Some("bool"), false, false, false);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("public function isActive(): bool"),
            "bool getter uses is prefix: {result}"
        );
    }

    #[test]
    fn builds_getter_without_type() {
        let prop = make_prop("data", None, false, false, false);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("public function getData()"),
            "no return type: {result}"
        );
        assert!(
            !result.contains(": "),
            "should not have return type separator: {result}"
        );
    }

    #[test]
    fn builds_getter_with_nullable_type() {
        let prop = make_prop("label", Some("?string"), false, false, false);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("public function getLabel(): ?string"),
            "nullable return type: {result}"
        );
    }

    #[test]
    fn builds_getter_with_union_type() {
        let prop = make_prop("id", Some("int|string"), false, false, false);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("public function getId(): int|string"),
            "union return type: {result}"
        );
    }

    #[test]
    fn builds_static_getter() {
        let prop = make_prop("count", Some("int"), false, false, true);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("public static function getCount(): int"),
            "static getter: {result}"
        );
        assert!(
            result.contains("return self::$count;"),
            "static getter uses self:: {result}"
        );
    }

    #[test]
    fn builds_getter_with_docblock_type() {
        let prop = make_prop("items", Some("Collection<User>"), true, false, false);
        let result = build_getter(&prop, "    ");
        assert!(
            result.contains("@return Collection<User>"),
            "docblock return tag: {result}"
        );
        assert!(
            !result.contains("): Collection"),
            "should not have native return type: {result}"
        );
        assert!(
            result.contains("public function getItems()"),
            "no native return type on signature: {result}"
        );
    }

    #[test]
    fn getter_respects_tab_indentation() {
        let prop = make_prop("name", Some("string"), false, false, false);
        let result = build_getter(&prop, "\t");
        assert!(
            result.contains("\tpublic function getName(): string"),
            "tab indent on signature: {result}"
        );
        assert!(
            result.contains("\t\treturn $this->name;"),
            "double tab in body: {result}"
        );
    }

    // ── build_setter ────────────────────────────────────────────────────

    #[test]
    fn builds_simple_setter() {
        let prop = make_prop("name", Some("string"), false, false, false);
        let result = build_setter(&prop, "    ");
        assert!(
            result.contains("public function setName(string $name): self"),
            "setter signature: {result}"
        );
        assert!(
            result.contains("$this->name = $name;"),
            "setter assignment: {result}"
        );
        assert!(
            result.contains("return $this;"),
            "setter returns $this: {result}"
        );
    }

    #[test]
    fn builds_setter_without_type() {
        let prop = make_prop("data", None, false, false, false);
        let result = build_setter(&prop, "    ");
        assert!(
            result.contains("public function setData($data): self"),
            "untyped setter: {result}"
        );
    }

    #[test]
    fn builds_setter_with_nullable_type() {
        let prop = make_prop("label", Some("?string"), false, false, false);
        let result = build_setter(&prop, "    ");
        assert!(
            result.contains("public function setLabel(?string $label): self"),
            "nullable param: {result}"
        );
    }

    #[test]
    fn builds_setter_with_union_type() {
        let prop = make_prop("id", Some("int|string"), false, false, false);
        let result = build_setter(&prop, "    ");
        assert!(
            result.contains("public function setId(int|string $id): self"),
            "union param: {result}"
        );
    }

    #[test]
    fn builds_static_setter() {
        let prop = make_prop("count", Some("int"), false, false, true);
        let result = build_setter(&prop, "    ");
        assert!(
            result.contains("public static function setCount(int $count): void"),
            "static setter returns void: {result}"
        );
        assert!(
            result.contains("self::$count = $count;"),
            "static setter uses self:: {result}"
        );
        assert!(
            !result.contains("return $this;"),
            "static setter should not return $this: {result}"
        );
    }

    #[test]
    fn builds_setter_with_docblock_type() {
        let prop = make_prop("items", Some("Collection<User>"), true, false, false);
        let result = build_setter(&prop, "    ");
        assert!(
            result.contains("@param Collection<User> $items"),
            "docblock param tag: {result}"
        );
        assert!(
            result.contains("public function setItems($items): self"),
            "no native param type on signature: {result}"
        );
    }

    #[test]
    fn setter_respects_tab_indentation() {
        let prop = make_prop("name", Some("string"), false, false, false);
        let result = build_setter(&prop, "\t");
        assert!(
            result.contains("\tpublic function setName(string $name): self"),
            "tab indent on signature: {result}"
        );
        assert!(
            result.contains("\t\t$this->name = $name;"),
            "double tab in body: {result}"
        );
    }

    // ── Integration-level tests using AST parsing ───────────────────────

    /// Find the byte offset of the first property declaration in the PHP
    /// source.  Looks for common property markers (`public `, `private `,
    /// `protected `, `/** @var`).
    fn find_property_offset(php: &str) -> u32 {
        for marker in &["private ", "protected ", "public ", "/** @var"] {
            if let Some(pos) = php.find(marker) {
                return pos as u32;
            }
        }
        panic!("no property marker found in test PHP");
    }

    fn parse_and_collect(php: &str) -> Vec<AccessorProperty> {
        let arena = Box::leak(Box::new(Bump::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let offset = find_property_offset(php);
        let ctx = find_cursor_context(&program.statements, offset);
        match &ctx {
            CursorContext::InClassLike {
                member: MemberContext::Property(prop),
                ..
            } => collect_accessor_properties(prop, php, program.trivia.as_slice()),
            _ => panic!("should find property context at offset {offset}"),
        }
    }

    #[test]
    fn collects_simple_property() {
        let php = "<?php\nclass Foo {\n    public string $name;\n}";
        let props = parse_and_collect(php);
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
        assert!(!props[0].type_from_docblock);
        assert!(!props[0].is_readonly);
        assert!(!props[0].is_static);
    }

    #[test]
    fn collects_readonly_property() {
        let php = "<?php\nclass Foo {\n    public readonly int $id;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name, "id");
        assert!(props[0].is_readonly);
    }

    #[test]
    fn collects_static_property() {
        let php = "<?php\nclass Foo {\n    public static int $count;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name, "count");
        assert!(props[0].is_static);
    }

    #[test]
    fn collects_nullable_type() {
        let php = "<?php\nclass Foo {\n    private ?string $label;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(
            props[0]
                .type_hint
                .as_ref()
                .map(|t| t.to_string())
                .as_deref(),
            Some("?string")
        );
    }

    #[test]
    fn collects_union_type() {
        let php = "<?php\nclass Foo {\n    protected int|string $id;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(
            props[0]
                .type_hint
                .as_ref()
                .map(|t| t.to_string())
                .as_deref(),
            Some("int|string")
        );
    }

    #[test]
    fn collects_docblock_type_when_no_native() {
        let php = "<?php\nclass Foo {\n    /** @var Collection<User> */\n    public $items;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(
            props[0]
                .type_hint
                .as_ref()
                .map(|t| t.to_string())
                .as_deref(),
            Some("Collection<User>")
        );
        assert!(props[0].type_from_docblock);
    }

    #[test]
    fn prefers_native_type_over_docblock() {
        let php = "<?php\nclass Foo {\n    /** @var array<string> */\n    public array $items;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(
            props[0]
                .type_hint
                .as_ref()
                .map(|t| t.to_string())
                .as_deref(),
            Some("array")
        );
        assert!(!props[0].type_from_docblock);
    }

    #[test]
    fn collects_untyped_property() {
        let php = "<?php\nclass Foo {\n    public $data;\n}";
        let props = parse_and_collect(php);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name, "data");
        assert!(props[0].type_hint.is_none());
    }

    #[test]
    fn skips_hooked_property() {
        let php = "<?php\nclass Foo {\n    public string $name { get => $this->name; }\n}";
        let arena = Box::leak(Box::new(Bump::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        match &ctx {
            CursorContext::InClassLike {
                member: MemberContext::Property(prop),
                ..
            } => {
                let props = collect_accessor_properties(prop, php, program.trivia.as_slice());
                assert!(props.is_empty(), "hooked properties should be skipped");
            }
            _ => {
                // The cursor might not land on the property depending on
                // parser behaviour, which is also acceptable.
            }
        }
    }

    // ── collect_existing_method_names ────────────────────────────────────

    #[test]
    fn finds_existing_methods() {
        let arena = Box::leak(Box::new(Bump::new()));
        let file_id = mago_database::file::FileId::new(b"input.php");
        let php = "<?php\nclass Foo {\n    public string $name;\n    public function getName(): string { return $this->name; }\n    public function setName(string $name): self { $this->name = $name; return $this; }\n}";
        let program = mago_syntax::parser::parse_file_content(arena, file_id, php.as_bytes());

        let ctx = find_cursor_context(&program.statements, 20);
        match &ctx {
            CursorContext::InClassLike { all_members, .. } => {
                let methods = collect_existing_method_names(all_members);
                assert_eq!(methods.len(), 2);
                assert!(methods.iter().any(|m| m == "getName"));
                assert!(methods.iter().any(|m| m == "setName"));
            }
            _ => panic!("should find class"),
        }
    }

    // ── Full code action integration tests ──────────────────────────────

    #[test]
    fn offers_all_three_actions_for_regular_property() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n}";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let titles: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
                _ => None,
            })
            .collect();

        assert!(
            titles.iter().any(|t| t == "Generate getter"),
            "should offer getter: {titles:?}"
        );
        assert!(
            titles.iter().any(|t| t == "Generate setter"),
            "should offer setter: {titles:?}"
        );
        assert!(
            titles.iter().any(|t| t == "Generate getter and setter"),
            "should offer both: {titles:?}"
        );
    }

    #[test]
    fn readonly_property_only_offers_getter() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    public readonly int $id;\n}";
        let prop_offset = content.find("public readonly").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let titles: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
                _ => None,
            })
            .collect();

        assert!(
            titles.iter().any(|t| t == "Generate getter"),
            "should offer getter for readonly: {titles:?}"
        );
        assert!(
            !titles.iter().any(|t| t == "Generate setter"),
            "should NOT offer setter for readonly: {titles:?}"
        );
        assert!(
            !titles.iter().any(|t| t == "Generate getter and setter"),
            "should NOT offer both for readonly: {titles:?}"
        );
    }

    #[test]
    fn skips_when_getter_already_exists() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n    public function getName(): string { return $this->name; }\n}";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let titles: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
                _ => None,
            })
            .collect();

        assert!(
            !titles.iter().any(|t| t == "Generate getter"),
            "should NOT offer getter when it exists: {titles:?}"
        );
        assert!(
            titles.iter().any(|t| t == "Generate setter"),
            "should still offer setter: {titles:?}"
        );
        assert!(
            !titles.iter().any(|t| t == "Generate getter and setter"),
            "should NOT offer both when getter exists: {titles:?}"
        );
    }

    #[test]
    fn skips_when_setter_already_exists() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n    public function setName(string $name): self { $this->name = $name; return $this; }\n}";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let titles: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
                _ => None,
            })
            .collect();

        assert!(
            titles.iter().any(|t| t == "Generate getter"),
            "should still offer getter: {titles:?}"
        );
        assert!(
            !titles.iter().any(|t| t == "Generate setter"),
            "should NOT offer setter when it exists: {titles:?}"
        );
        assert!(
            !titles.iter().any(|t| t == "Generate getter and setter"),
            "should NOT offer both when setter exists: {titles:?}"
        );
    }

    #[test]
    fn no_actions_when_both_exist() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n    public function getName(): string { return $this->name; }\n    public function setName(string $name): self { $this->name = $name; return $this; }\n}";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let getter_setter_titles: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca)
                    if ca.title.starts_with("Generate getter")
                        || ca.title.starts_with("Generate setter") =>
                {
                    Some(ca.title.clone())
                }
                _ => None,
            })
            .collect();

        assert!(
            getter_setter_titles.is_empty(),
            "should not offer any getter/setter actions: {getter_setter_titles:?}"
        );
    }

    #[test]
    fn getter_edit_contains_correct_php() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n}\n";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let getter_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate getter" => Some(ca),
            _ => None,
        });

        let ca = getter_action.expect("should have getter action");
        let edit = ca.edit.as_ref().expect("should have edit");
        let changes = edit.changes.as_ref().expect("should have changes");
        let edits = changes.values().next().expect("should have file edits");
        let new_text = &edits[0].new_text;

        assert!(
            new_text.contains("public function getName(): string"),
            "correct getter signature: {new_text}"
        );
        assert!(
            new_text.contains("return $this->name;"),
            "correct getter body: {new_text}"
        );
    }

    #[test]
    fn setter_edit_contains_correct_php() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n}\n";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let setter_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate setter" => Some(ca),
            _ => None,
        });

        let ca = setter_action.expect("should have setter action");
        let edit = ca.edit.as_ref().expect("should have edit");
        let changes = edit.changes.as_ref().expect("should have changes");
        let edits = changes.values().next().expect("should have file edits");
        let new_text = &edits[0].new_text;

        assert!(
            new_text.contains("public function setName(string $name): self"),
            "correct setter signature: {new_text}"
        );
        assert!(
            new_text.contains("$this->name = $name;"),
            "correct setter assignment: {new_text}"
        );
        assert!(
            new_text.contains("return $this;"),
            "correct setter return: {new_text}"
        );
    }

    #[test]
    fn bool_property_uses_is_prefix_in_action() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private bool $active;\n}\n";
        let prop_offset = content.find("private bool $active").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let getter_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate getter" => Some(ca),
            _ => None,
        });

        let ca = getter_action.expect("should have getter action");
        let edit = ca.edit.as_ref().expect("should have edit");
        let changes = edit.changes.as_ref().expect("should have changes");
        let edits = changes.values().next().expect("should have file edits");
        let new_text = &edits[0].new_text;

        assert!(
            new_text.contains("public function isActive(): bool"),
            "bool getter uses is prefix: {new_text}"
        );
    }

    #[test]
    fn static_property_generates_static_methods() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private static int $count;\n}\n";
        let prop_offset = content.find("private static int $count").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);

        // Check getter.
        let getter_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate getter" => Some(ca),
            _ => None,
        });
        let ca = getter_action.expect("should have getter action");
        let edit = ca.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = changes.values().next().unwrap();
        assert!(
            edits[0]
                .new_text
                .contains("public static function getCount(): int"),
            "static getter: {}",
            edits[0].new_text
        );
        assert!(
            edits[0].new_text.contains("return self::$count;"),
            "static getter body: {}",
            edits[0].new_text
        );

        // Check setter.
        let setter_action = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate setter" => Some(ca),
            _ => None,
        });
        let ca = setter_action.expect("should have setter action");
        let edit = ca.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = changes.values().next().unwrap();
        assert!(
            edits[0]
                .new_text
                .contains("public static function setCount(int $count): void"),
            "static setter: {}",
            edits[0].new_text
        );
        assert!(
            edits[0].new_text.contains("self::$count = $count;"),
            "static setter body: {}",
            edits[0].new_text
        );
    }

    #[test]
    fn case_insensitive_method_check() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nclass Foo {\n    private string $name;\n    public function GETNAME(): string { return $this->name; }\n}";
        let prop_offset = content.find("private string $name").unwrap();
        let start = offset_to_position(content, prop_offset);
        let end = start;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range { start, end },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let titles: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca)
                    if ca.title.starts_with("Generate getter")
                        || ca.title.starts_with("Generate setter") =>
                {
                    Some(ca.title.clone())
                }
                _ => None,
            })
            .collect();

        assert!(
            !titles.iter().any(|t| t == "Generate getter"),
            "GETNAME should count as existing getter (case insensitive): {titles:?}"
        );
    }
}

//! "Generate property hooks" code action (PHP 8.4+).
//!
//! When the cursor is on a property declaration inside a class-like body,
//! this module offers up to three code actions:
//!
//! 1. **Generate get hook** — adds a `get` hook to the property.
//! 2. **Generate set hook** — adds a `set` hook to the property.
//! 3. **Generate get and set hooks** — adds both hooks.
//!
//! **Code action kind:** `refactor`.
//!
//! - **Static properties** are skipped (PHP 8.4 does not support hooks on
//!   static properties).
//! - **Readonly properties** only get the "Generate get hook" action.
//! - **Interface / abstract class properties** generate abstract hook
//!   signatures (no body).
//! - **Properties that already have hooks** only offer the missing hook(s).
//! - **Default values** are preserved when rewriting.
//! - **Constructor-promoted properties** are supported.

use mago_span::HasSpan;
use mago_syntax::cst::class_like::property::Property;
use mago_syntax::cst::modifier::Modifier;
use tower_lsp::lsp_types::*;

use super::cursor_context::{
    ClassLikeContextKind, CursorContext, MemberContext, find_cursor_context,
};
use super::detect_indent_from_members;
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::text_position::offset_to_position;

// ── Data types ──────────────────────────────────────────────────────────────

/// Describes a property for which hooks can be generated.
struct HookableProperty {
    /// Property name without the `$` prefix.
    name: String,
}

// ── Which hooks already exist ───────────────────────────────────────────────

fn existing_hook_names<'a>(property: &Property<'a>) -> (bool, bool) {
    match property {
        Property::Hooked(hooked) => {
            let mut has_get = false;
            let mut has_set = false;
            for hook in hooked.hook_list.hooks.iter() {
                match hook.name.value {
                    b"get" => has_get = true,
                    b"set" => has_set = true,
                    _ => {}
                }
            }
            (has_get, has_set)
        }
        Property::Plain(_) => (false, false),
    }
}

fn has_readonly<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers
        .into_iter()
        .any(|m| matches!(m, Modifier::Readonly(_)))
}

fn has_static<'a>(modifiers: impl Iterator<Item = &'a Modifier<'a>>) -> bool {
    modifiers.into_iter().any(|m| m.is_static())
}

// ── Hook text generation ────────────────────────────────────────────────────

/// Build the replacement text for a property declaration with hooks.
///
/// This replaces the entire property declaration (from its start to its
/// end) with a new declaration that includes the requested hooks.
fn build_hooked_property_text(
    prop: &HookableProperty,
    original_text: &str,
    indent: &str,
    gen_get: bool,
    gen_set: bool,
    is_interface: bool,
    existing_hooks_text: Option<&str>,
) -> String {
    let mut result = String::new();

    if let Some(existing) = existing_hooks_text {
        // We're adding hooks to an already-hooked property.
        // `existing` is the full property text including the hook block.
        // We need to insert the new hook(s) before the closing `}`.
        let trimmed = existing.trim_end();
        let close_brace_pos = trimmed.rfind('}');
        if let Some(pos) = close_brace_pos {
            result.push_str(&trimmed[..pos]);
            if gen_get {
                result.push_str(&build_single_hook("get", prop, indent, is_interface));
            }
            if gen_set {
                result.push_str(&build_single_hook("set", prop, indent, is_interface));
            }
            result.push_str(indent);
            result.push('}');
        }
    } else {
        // Plain property: strip the semicolon, add the hook block.
        let trimmed = original_text.trim_end();
        let base = if let Some(stripped) = trimmed.strip_suffix(';') {
            stripped.trim_end()
        } else {
            trimmed
        };
        result.push_str(base);
        result.push_str(" {\n");

        if gen_get {
            result.push_str(&build_single_hook("get", prop, indent, is_interface));
        }
        if gen_set {
            result.push_str(&build_single_hook("set", prop, indent, is_interface));
        }

        result.push_str(indent);
        result.push('}');
    }

    result
}

/// Build a single hook body (`get` or `set`).
fn build_single_hook(
    kind: &str,
    prop: &HookableProperty,
    indent: &str,
    is_interface: bool,
) -> String {
    let mut s = String::new();
    let hook_indent = format!(
        "{indent}{indent_unit}",
        indent_unit = detect_indent_unit(indent)
    );

    if is_interface {
        // Abstract hook: just the signature with a semicolon.
        s.push_str(&hook_indent);
        s.push_str(kind);
        s.push_str(";\n");
    } else {
        // Concrete hook with arrow expression.
        s.push_str(&hook_indent);
        s.push_str(kind);
        s.push_str(" => ");
        match kind {
            "get" => {
                s.push_str("$this->");
                s.push_str(&prop.name);
                s.push_str(";\n");
            }
            "set" => {
                s.push_str("$this->");
                s.push_str(&prop.name);
                s.push_str(" = $value;\n");
            }
            _ => {}
        }
    }

    s
}

/// Detect the indent unit from the member indent string.
///
/// If the indent is tabs, return a single tab.  Otherwise return four
/// spaces (matching the most common convention).
fn detect_indent_unit(indent: &str) -> &str {
    if indent.contains('\t') { "\t" } else { "    " }
}

// ── Public API ──────────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Generate get hook", "Generate set hook", and
    /// "Generate get and set hooks" code actions for the cursor position.
    pub(crate) fn collect_generate_property_hook_actions(
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

        // Resolve the cursor context and extract the (owned) data needed to
        // build the hook edits.  The borrowed AST does not escape the
        // closure, so we capture whether the property is already hooked
        // (`is_hooked`) and recompute the source slice from offsets below.
        let Some((
            can_get,
            can_set,
            prop_info,
            indent,
            prop_start,
            prop_end,
            is_interface,
            is_hooked,
        )) = crate::parser::with_parsed_program(
            content,
            "generate_property_hooks",
            |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);

                let (property, all_members, class_kind, class_readonly) = match &ctx {
                    CursorContext::InClassLike {
                        kind,
                        class_readonly,
                        member: MemberContext::Property(prop),
                        all_members,
                    } => (*prop, *all_members, *kind, *class_readonly),
                    _ => return None,
                };

                // Enums cannot have properties (only backed enum cases), and
                // PHP 8.4 does not support hooks on enum properties anyway.
                if class_kind == ClassLikeContextKind::Enum {
                    return None;
                }

                let is_interface = class_kind == ClassLikeContextKind::Interface;

                // Hooks are not supported on static properties.
                let is_static = match property {
                    Property::Plain(plain) => has_static(plain.modifiers.iter()),
                    Property::Hooked(hooked) => has_static(hooked.modifiers.iter()),
                };
                if is_static {
                    return None;
                }

                let prop_readonly = match property {
                    Property::Plain(plain) => has_readonly(plain.modifiers.iter()),
                    Property::Hooked(hooked) => has_readonly(hooked.modifiers.iter()),
                };
                // PHP 8.4 does not allow hooks on readonly properties at all.
                // A `readonly class` makes every property readonly implicitly.
                if prop_readonly || class_readonly {
                    return None;
                }

                // Get the property name.
                let prop_name = match property {
                    Property::Plain(plain) => {
                        // For multi-variable declarations like
                        // `public int $a, $b;`, use the first variable name.
                        // Multi-variable declarations can't have hooks anyway.
                        {
                            let first_item = plain.items.first()?;
                            let var = first_item.variable();
                            bytes_to_str(var.name)
                                .strip_prefix('$')
                                .unwrap_or(bytes_to_str(var.name))
                                .to_string()
                        }
                    }
                    Property::Hooked(hooked) => {
                        let var = hooked.item.variable();
                        bytes_to_str(var.name)
                            .strip_prefix('$')
                            .unwrap_or(bytes_to_str(var.name))
                            .to_string()
                    }
                };

                // For plain properties with multiple variables, hooks cannot
                // be generated (PHP does not support hooks on multi-variable
                // declarations).
                if let Property::Plain(plain) = property
                    && plain.items.len() > 1
                {
                    return None;
                }

                let (has_get, has_set) = existing_hook_names(property);
                let can_get = !has_get;
                let can_set = !has_set;
                if !can_get && !can_set {
                    return None;
                }

                let prop_info = HookableProperty { name: prop_name };
                let indent = detect_indent_from_members(all_members, content);

                // The property's full span so we can replace it.
                let prop_span = property.span();
                let prop_start = prop_span.start.offset as usize;
                let prop_end = prop_span.end.offset as usize;

                let is_hooked = matches!(property, Property::Hooked(_));

                Some((
                    can_get,
                    can_set,
                    prop_info,
                    indent,
                    prop_start,
                    prop_end,
                    is_interface,
                    is_hooked,
                ))
            },
        )
        else {
            return;
        };

        let original_text = &content[prop_start..prop_end];

        let start_pos = offset_to_position(content, prop_start);
        let end_pos = offset_to_position(content, prop_end);
        let replace_range = Range {
            start: start_pos,
            end: end_pos,
        };

        // Hooked properties carry their existing hooks forward.
        let existing_hooks_text = if is_hooked { Some(original_text) } else { None };

        // ── Generate get hook ───────────────────────────────────────────
        if can_get {
            let new_text = build_hooked_property_text(
                &prop_info,
                original_text,
                &indent,
                true,
                false,
                is_interface,
                existing_hooks_text,
            );

            let edit = TextEdit {
                range: replace_range,
                new_text,
            };

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate get hook".to_string(),
                kind: Some(CodeActionKind::REFACTOR),
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

        // ── Generate set hook ───────────────────────────────────────────
        if can_set {
            let new_text = build_hooked_property_text(
                &prop_info,
                original_text,
                &indent,
                false,
                true,
                is_interface,
                existing_hooks_text,
            );

            let edit = TextEdit {
                range: replace_range,
                new_text,
            };

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate set hook".to_string(),
                kind: Some(CodeActionKind::REFACTOR),
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

        // ── Generate get and set hooks ──────────────────────────────────
        if can_get && can_set {
            let new_text = build_hooked_property_text(
                &prop_info,
                original_text,
                &indent,
                true,
                true,
                is_interface,
                existing_hooks_text,
            );

            let edit = TextEdit {
                range: replace_range,
                new_text,
            };

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Generate get and set hooks".to_string(),
                kind: Some(CodeActionKind::REFACTOR),
                diagnostics: None,
                edit: Some(crate::code_actions::single_file_edit(doc_uri, vec![edit])),
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: None,
            }));
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests for hook text builders ────────────────────────────────

    fn make_prop(name: &str) -> HookableProperty {
        HookableProperty {
            name: name.to_string(),
        }
    }

    #[test]
    fn builds_get_hook_for_plain_property() {
        let prop = make_prop("name");
        let original = "public string $name;";
        let result = build_hooked_property_text(&prop, original, "    ", true, false, false, None);
        assert!(result.contains("get => $this->name;"), "got: {result}");
        assert!(!result.contains("set"), "got: {result}");
        assert!(result.starts_with("public string $name {"), "got: {result}");
        assert!(result.ends_with('}'), "got: {result}");
    }

    #[test]
    fn builds_set_hook_for_plain_property() {
        let prop = make_prop("name");
        let original = "public string $name;";
        let result = build_hooked_property_text(&prop, original, "    ", false, true, false, None);
        assert!(
            result.contains("set => $this->name = $value;"),
            "got: {result}"
        );
        assert!(!result.contains("get"), "got: {result}");
    }

    #[test]
    fn builds_both_hooks_for_plain_property() {
        let prop = make_prop("name");
        let original = "public string $name;";
        let result = build_hooked_property_text(&prop, original, "    ", true, true, false, None);
        assert!(result.contains("get => $this->name;"), "got: {result}");
        assert!(
            result.contains("set => $this->name = $value;"),
            "got: {result}"
        );
    }

    #[test]
    fn builds_abstract_hooks_for_interface() {
        let prop = make_prop("name");
        let original = "public string $name;";
        let result = build_hooked_property_text(&prop, original, "    ", true, true, true, None);
        assert!(result.contains("get;"), "got: {result}");
        assert!(result.contains("set;"), "got: {result}");
        assert!(!result.contains("=>"), "got: {result}");
    }

    #[test]
    fn preserves_default_value() {
        let prop = make_prop("name");
        let original = "public string $name = 'default';";
        let result = build_hooked_property_text(&prop, original, "    ", true, false, false, None);
        assert!(
            result.contains("$name = 'default'"),
            "default value should be preserved, got: {result}"
        );
        assert!(result.contains("get => $this->name;"), "got: {result}");
    }

    #[test]
    fn adds_hook_to_existing_hooked_property() {
        let prop = make_prop("name");
        let existing = "public string $name {\n        get => $this->name;\n    }";
        let result =
            build_hooked_property_text(&prop, existing, "    ", false, true, false, Some(existing));
        assert!(result.contains("get => $this->name;"), "got: {result}");
        assert!(
            result.contains("set => $this->name = $value;"),
            "got: {result}"
        );
    }

    #[test]
    fn tab_indentation() {
        let prop = make_prop("name");
        let original = "public string $name;";
        let result = build_hooked_property_text(&prop, original, "\t", true, false, false, None);
        assert!(result.contains("\t\tget =>"), "got: {result}");
    }

    #[test]
    fn detect_indent_unit_spaces() {
        assert_eq!(detect_indent_unit("    "), "    ");
    }

    #[test]
    fn detect_indent_unit_tabs() {
        assert_eq!(detect_indent_unit("\t"), "\t");
    }
}

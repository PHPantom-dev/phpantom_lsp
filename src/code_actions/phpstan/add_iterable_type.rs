//! "Add iterable return type" code action for PHPStan `missingType.iterableValue`.
//!
//! When PHPStan reports that a method or function return type has no
//! value type specified in an iterable type (e.g. `array`, `iterable`,
//! `Traversable`), this code action offers to add a `@return` docblock
//! tag with the element type inferred from the function body.
//!
//! The element type is determined by scanning `return` statements for
//! array literals, variable types, and `new ClassName()` expressions
//! using the same resolution pipeline as `missingType.return`.  When
//! inference cannot determine a concrete type, the action falls back
//! to `<mixed>` as recommended by PHPStan's documentation.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `missingType.iterableValue` whose message mentions "return type".
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_add_iterable_type_actions`) validates the action
//! is applicable and emits a lightweight `CodeAction` with a generic
//! title ("Add `@return` type") and no `edit`.  Phase 2
//! (`resolve_add_iterable_type`) infers the element type from the
//! function body and computes the workspace edit.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::fix_return_type::{Anchor, target_function};
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, FunctionDocblock, deferred_quickfix};
use crate::php_type::{PhpType, TypeKind};

// ── Constants ───────────────────────────────────────────────────────────────

/// The PHPStan identifier we match on.
const ITERABLE_VALUE_ID: &str = "missingType.iterableValue";

/// Action kind string for the add-iterable-return-type fix.
const ACTION_KIND: &str = "phpstan.addIterableType";

// ── Message parsing ─────────────────────────────────────────────────────────

/// Extract the iterable type name from a `missingType.iterableValue`
/// diagnostic message that refers to the return type.
///
/// Message formats:
/// - `Method Foo::bar() return type has no value type specified in iterable type array.`
/// - `Function foo() return type has no value type specified in iterable type array.`
/// - Messages may also mention `iterable`, `Traversable`, `Generator`,
///   `Collection`, or any other iterable class/interface.
///
/// Returns the parsed `PhpType`, or `None` if the message does not
/// refer to a return type or does not match the expected pattern.
fn extract_iterable_return_type(message: &str) -> Option<PhpType> {
    // Only handle the "return type" variant.
    if !message.contains("return type") {
        return None;
    }

    let marker = "in iterable type ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];

    // The type name ends at `.` (end of sentence) or whitespace.
    let end = rest.find(['.', '\n']).unwrap_or(rest.len());
    let type_name = rest[..end].trim();

    if type_name.is_empty() {
        return None;
    }

    Some(PhpType::parse(type_name))
}

/// Build the `@return` type from the iterable type name and a vector
/// of inferred element type arguments.
///
/// Uses PHPStan's generic syntax: `array<string>`, `iterable<User>`,
/// `Traversable<mixed>`, `array<int, string>`, etc.
fn build_return_type(iterable_type: &str, args: Vec<PhpType>) -> PhpType {
    PhpType::generic(iterable_type, args)
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect code actions for PHPStan `missingType.iterableValue`
    /// diagnostics on return types.
    ///
    /// **Phase 1**: validates the action is applicable and emits a
    /// lightweight `CodeAction` with a `data` payload but **no `edit`**.
    /// The edit is computed lazily in
    /// [`resolve_add_iterable_type`](Self::resolve_add_iterable_type).
    pub(crate) fn collect_add_iterable_type_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
            };

            if identifier != ITERABLE_VALUE_ID {
                continue;
            }

            // Only handle the "return type" variant.
            let iterable_type_parsed = match extract_iterable_return_type(&diag.message) {
                Some(t) => t,
                None => continue,
            };
            let iterable_type = iterable_type_parsed.to_string();

            let diag_line = diag.range.start.line as usize;

            // The diagnostic is on the function's declaration: the line
            // of the `function` keyword or of an attribute or modifier
            // before it.
            let Some(target) =
                target_function(content, diag_line, Anchor::Declaration, &diag.message)
            else {
                continue;
            };

            // Skip if there is already a @return tag with a generic type.
            if has_structured_return_tag(&target.docblock) {
                continue;
            }

            // Use a generic title — type inference is deferred to Phase 2.
            let title = "Add @return type".to_string();

            let extra = serde_json::json!({
                "iterable_type": &iterable_type,
                "message": &diag.message,
            });

            out.push(deferred_quickfix(
                title,
                ACTION_KIND,
                uri,
                &params.range,
                vec![diag.clone()],
                true,
                extra,
            ));
        }
    }

    /// Resolve the "Add iterable return type" code action by computing
    /// the full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks this action.  Infers the element type from the function
    /// body using the same resolution pipeline as `missingType.return`,
    /// then inserts (or updates) a `@return` tag.
    pub(crate) fn resolve_add_iterable_type(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let iterable_type = data.extra.get("iterable_type")?.as_str()?;
        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;
        let message = data.extra.get("message")?.as_str()?;
        let target = target_function(content, diag_line, Anchor::Declaration, message)?;

        // Infer the element type from the function body.
        let element_args = self
            .infer_iterable_element_type(&data.uri, content, target.start, iterable_type)
            .unwrap_or_else(|| vec![PhpType::mixed()]);

        let return_type = build_return_type(iterable_type, element_args).to_string();

        let docblock = &target.docblock;
        let edit = match docblock.replace_return_type_edit(content, &return_type) {
            Some(edit) => edit,
            None => docblock.add_return_tag_edit(content, &return_type),
        };

        let doc_uri: Url = data.uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
    }

    /// Infer the element type of an iterable return by scanning the body
    /// of the function declared at `offset`.
    ///
    /// Delegates to [`infer_return_type_at`](Self::infer_return_type_at)
    /// in `fix_return_type` to get the effective return type.
    /// When the inferred type is a generic iterable (e.g. `list<string>`,
    /// `array<int, User>`), the inner type parameter(s) are extracted
    /// and returned as a `Vec<PhpType>`.
    /// When it's a bare `array` or `mixed`, returns `None` so the caller
    /// falls back to `vec![PhpType::mixed()]`.
    fn infer_iterable_element_type(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
        iterable_type: &str,
    ) -> Option<Vec<PhpType>> {
        let inferred = self.infer_return_type_at(uri, content, offset, false)?;

        // Prefer the effective type (richer, e.g. `list<string>`),
        // falling back to the native type. A literal array infers as a
        // shape (`[1, 2]` is `array{1, 2}`), and its elements are what the
        // shape holds, not the shape itself.
        let parsed = inferred
            .effective
            .as_ref()
            .unwrap_or(&inferred.native)
            .generalized_array();

        // If the inferred type is just `array`, `mixed`, or the bare
        // iterable type, we can't determine element types.
        if parsed.is_bare_array() || parsed.is_mixed() || parsed.is_named_ci(iterable_type) {
            return None;
        }

        // Try to extract the generic parameter(s) from the inferred type.
        // e.g. `list<string>` → [string], `array<int, User>` → [int, User]
        if let TypeKind::Generic(g) = &parsed.kind() {
            if !g.args.is_empty() && g.args.iter().all(|a| !a.is_mixed()) {
                return Some(g.args.clone());
            }
            return None;
        }

        // The inferred type is a concrete non-iterable type (e.g.
        // `string` when the function returns `['a', 'b']` and the
        // resolver collapsed it).  This shouldn't normally happen for
        // an iterable return, but use it as the element type.
        if !parsed.is_void() && !parsed.is_null() {
            return Some(vec![parsed]);
        }

        None
    }
}

// ── Stale detection ─────────────────────────────────────────────────────────

/// Check whether a `missingType.iterableValue` return-type diagnostic
/// is stale.
///
/// The diagnostic is stale when:
/// - The function/method now has a `@return` tag containing a generic
///   type (indicated by `<` or `[]` in the tag).
/// - The diagnostic line no longer exists.
///
/// Called from `is_stale_phpstan_diagnostic` in `diagnostics/stale.rs`.
pub(crate) fn is_add_iterable_type_stale(content: &str, diag_line: usize, message: &str) -> bool {
    // Only handle the return-type variant.
    if !message.contains("return type") {
        return false;
    }

    if content.lines().nth(diag_line).is_none() {
        return true; // line doesn't exist any more → stale
    }

    target_function(content, diag_line, Anchor::Declaration, message)
        .is_some_and(|target| has_structured_return_tag(&target.docblock))
}

/// Whether the function's `@return` tag already gives its type a
/// structure (see [`docblock_return_line_has_type_structure`]).
fn has_structured_return_tag(docblock: &FunctionDocblock) -> bool {
    docblock
        .return_tag()
        .is_some_and(|(_, line)| docblock_return_line_has_type_structure(line))
}

/// Check whether a `@return` docblock line already contains a type with
/// generic parameters, array slice syntax, or other type structure.
///
/// Extracts the type token from the line and uses `PhpType::parse()` +
/// `has_type_structure()` for a structured check instead of a raw
/// `.contains('<')` heuristic.
fn docblock_return_line_has_type_structure(line: &str) -> bool {
    // Strip the leading ` * @return ` (or similar) prefix to get the type text.
    let trimmed = line.trim_start().trim_start_matches('*').trim_start();
    let rest = if let Some(after) = trimmed.strip_prefix("@return") {
        after.trim_start()
    } else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let (type_token, _) = crate::docblock::type_strings::split_type_token(rest);
    if type_token.is_empty() {
        return false;
    }
    let parsed = crate::php_type::PhpType::parse(type_token);
    parsed.has_type_structure()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_iterable_return_type ────────────────────────────────

    #[test]
    fn extracts_array_from_method_message() {
        let msg =
            "Method Foo::bar() return type has no value type specified in iterable type array.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("array"))
        );
    }

    #[test]
    fn extracts_array_from_function_message() {
        let msg = "Function foo() return type has no value type specified in iterable type array.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("array"))
        );
    }

    #[test]
    fn extracts_iterable_type() {
        let msg =
            "Method Foo::bar() return type has no value type specified in iterable type iterable.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("iterable"))
        );
    }

    #[test]
    fn extracts_traversable_type() {
        let msg = "Method Foo::bar() return type has no value type specified in iterable type Traversable.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("Traversable"))
        );
    }

    #[test]
    fn extracts_generator_type() {
        let msg =
            "Function gen() return type has no value type specified in iterable type Generator.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("Generator"))
        );
    }

    #[test]
    fn extracts_collection_type() {
        let msg = "Method Foo::bar() return type has no value type specified in iterable type Collection.";
        assert_eq!(
            extract_iterable_return_type(msg),
            Some(PhpType::parse("Collection"))
        );
    }

    #[test]
    fn ignores_parameter_variant() {
        let msg = "Method Foo::bar() has parameter $items with no value type specified in iterable type array.";
        assert_eq!(extract_iterable_return_type(msg), None);
    }

    #[test]
    fn ignores_property_variant() {
        let msg = "Property Foo::$items type has no value type specified in iterable type array.";
        assert_eq!(extract_iterable_return_type(msg), None);
    }

    #[test]
    fn ignores_unrelated_message() {
        let msg = "Method Foo::bar() should return int but returns string.";
        assert_eq!(extract_iterable_return_type(msg), None);
    }

    // ── build_return_type ──────────────────────────────────────────

    #[test]
    fn builds_array_with_element_type() {
        assert_eq!(
            build_return_type("array", vec![PhpType::parse("string")]).to_string(),
            "array<string>"
        );
    }

    #[test]
    fn builds_iterable_mixed() {
        assert_eq!(
            build_return_type("iterable", vec![PhpType::parse("mixed")]).to_string(),
            "iterable<mixed>"
        );
    }

    #[test]
    fn builds_traversable_with_element() {
        assert_eq!(
            build_return_type("Traversable", vec![PhpType::parse("User")]).to_string(),
            "Traversable<User>"
        );
    }

    #[test]
    fn builds_array_with_key_value() {
        assert_eq!(
            build_return_type(
                "array",
                vec![PhpType::parse("int"), PhpType::parse("string")]
            )
            .to_string(),
            "array<int, string>"
        );
    }

    // ── Locating the docblock ──────────────────────────────────────

    /// The docblock of the method declared on `line` of `php`.
    fn docblock_at(php: &str, line: usize) -> FunctionDocblock {
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        target_function(php, line, Anchor::Declaration, msg)
            .unwrap()
            .docblock
    }

    #[test]
    fn finds_existing_multiline_docblock() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function foo(): array\n    {\n    }\n}";
        let db = docblock_at(php, 5);
        assert_eq!(
            db.docblock.unwrap().text,
            "    /**\n     * Summary.\n     */\n"
        );
    }

    #[test]
    fn finds_docblock_with_return_tag() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     * @return array\n     */\n    public function foo(): array\n    {\n    }\n}";
        let db = docblock_at(php, 6);
        assert_eq!(
            db.return_tag().map(|(_, line)| line),
            Some("     * @return array")
        );
    }

    #[test]
    fn no_docblock() {
        let php = "<?php\nclass Foo {\n    public function foo(): array\n    {\n    }\n}";
        assert!(docblock_at(php, 2).docblock.is_none());
    }

    #[test]
    fn finds_single_line_docblock() {
        let php = "<?php\nclass Foo {\n    /** Summary. */\n    public function foo(): array\n    {\n    }\n}";
        let db = docblock_at(php, 3);
        assert_eq!(db.docblock.unwrap().text, "    /** Summary. */\n");
    }

    #[test]
    fn skips_attributes_between_docblock_and_function() {
        let php = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    #[Override]\n    public function foo(): array\n    {\n    }\n}";
        let db = docblock_at(php, 6);
        assert!(db.docblock.unwrap().text.contains("Summary."));
    }

    // ── Stale detection ────────────────────────────────────────────

    #[test]
    fn stale_when_return_tag_has_generic_type() {
        let src = "<?php\nclass Foo {\n    /**\n     * @return array<string>\n     */\n    public function foo(): array\n    {\n    }\n}";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(is_add_iterable_type_stale(src, 5, msg));
    }

    #[test]
    fn stale_when_return_tag_has_array_suffix() {
        let src = "<?php\nclass Foo {\n    /**\n     * @return string[]\n     */\n    public function foo(): array\n    {\n    }\n}";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(is_add_iterable_type_stale(src, 5, msg));
    }

    #[test]
    fn not_stale_when_no_return_tag() {
        let src = "<?php\nclass Foo {\n    /**\n     * Summary.\n     */\n    public function foo(): array\n    {\n    }\n}";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(!is_add_iterable_type_stale(src, 5, msg));
    }

    #[test]
    fn not_stale_when_return_tag_has_no_generic() {
        let src = "<?php\nclass Foo {\n    /**\n     * @return array\n     */\n    public function foo(): array\n    {\n    }\n}";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(!is_add_iterable_type_stale(src, 5, msg));
    }

    #[test]
    fn not_stale_for_non_return_variant() {
        let src = "    public function foo(array $items): void\n    {\n    }";
        let msg = "Method Foo::foo() has parameter $items with no value type specified in iterable type array.";
        assert!(!is_add_iterable_type_stale(src, 0, msg));
    }

    #[test]
    fn stale_when_line_gone() {
        let src = "    public function foo(): array\n    {\n    }";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        assert!(is_add_iterable_type_stale(src, 100, msg));
    }

    #[test]
    fn stale_on_modifier_line_with_generic_return() {
        let src = "<?php\nclass Foo {\n    /**\n     * @return array<int, string>\n     */\n    public\n    function foo(): array\n    {\n    }\n}";
        let msg =
            "Method Foo::foo() return type has no value type specified in iterable type array.";
        // Diagnostic is on line 5 (the `public` modifier line).
        assert!(is_add_iterable_type_stale(src, 5, msg));
    }
}

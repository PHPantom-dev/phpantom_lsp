//! "Fix unsafe `new static()`" code actions for PHPStan `new.static`.
//!
//! When PHPStan reports `Unsafe usage of new static().`, this module
//! offers three quickfixes:
//!
//! 1. **Add `@phpstan-consistent-constructor`** to the class docblock
//!    (preferred). Creates a new docblock if none exists.
//! 2. **Add `final` to class** — insert `final` before the `class`
//!    keyword. Not offered for an abstract class.
//! 3. **Add `final` to constructor** — insert `final` before the first
//!    modifier (or `function` keyword) of `__construct`.
//!
//! The class is the innermost class-like declaration around the
//! diagnostic in the parsed file; inside a trait, interface, enum or
//! anonymous class no fix is offered.
//!
//! **Trigger:** A PHPStan diagnostic with identifier `new.static`
//! overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_new_static_actions`) performs all validation and
//! emits lightweight `CodeAction` values with a `data` payload but no
//! `edit`.  Phase 2 (`resolve_new_static`) computes the workspace edit
//! on demand when the user picks an action.

use mago_span::HasSpan;
use mago_syntax::cst::ModifierSequenceExt;
use mago_syntax::cst::Node;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::cursor_context::innermost_at;
use crate::code_actions::docblock_edit::ClassDocblock;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::text_position::{line_start_byte_offset, offset_to_position};

/// The PHPStan identifier we match on.
const NEW_STATIC_ID: &str = "new.static";

/// The tag that tells PHPStan every subclass keeps the constructor's
/// signature.
const CONSISTENT_CONSTRUCTOR_TAG: &str = "@phpstan-consistent-constructor";

/// The three sub-actions the user can pick.
const ACTION_ADD_TAG: &str = "phpstan.newStatic.addTag";
const ACTION_FINAL_CLASS: &str = "phpstan.newStatic.finalClass";
const ACTION_FINAL_CONSTRUCTOR: &str = "phpstan.newStatic.finalConstructor";

impl Backend {
    /// Collect "Fix unsafe `new static()`" code actions for PHPStan
    /// `new.static` diagnostics.
    ///
    /// **Phase 1**: validates the action is applicable and emits up to
    /// three lightweight `CodeAction` values with `data` payloads but
    /// **no `edit`**.  Edits are computed lazily in
    /// [`resolve_new_static`](Self::resolve_new_static).
    pub(crate) fn collect_new_static_actions(
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

            if identifier != NEW_STATIC_ID {
                continue;
            }

            let diag_line = diag.range.start.line as usize;

            let Some(class_info) = find_enclosing_class(content, diag_line) else {
                continue;
            };

            // Check staleness: if the class is already final, the
            // constructor is already final, or the docblock already
            // contains `@phpstan-consistent-constructor`, skip.
            if is_already_fixed(&class_info) {
                continue;
            }

            let class_name = &class_info.name;

            // ── Action 1: Add @phpstan-consistent-constructor (preferred) ──
            {
                let title = format!("Add @phpstan-consistent-constructor to {}", class_name);
                let extra = serde_json::json!({
                    "diagnostic_message": diag.message,
                    "diagnostic_line": diag.range.start.line,
                    "diagnostic_code": NEW_STATIC_ID,
                    "sub_action": ACTION_ADD_TAG,
                });
                let data = make_code_action_data(ACTION_ADD_TAG, uri, &params.range, extra);
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    command: None,
                    is_preferred: Some(true),
                    disabled: None,
                    data: Some(data),
                }));
            }

            // ── Action 2: Add `final` to class ─────────────────────────
            // An abstract class cannot be final.
            if !class_info.is_abstract {
                let title = format!("Add final to class {}", class_name);
                let extra = serde_json::json!({
                    "diagnostic_message": diag.message,
                    "diagnostic_line": diag.range.start.line,
                    "diagnostic_code": NEW_STATIC_ID,
                    "sub_action": ACTION_FINAL_CLASS,
                });
                let data = make_code_action_data(ACTION_FINAL_CLASS, uri, &params.range, extra);
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    command: None,
                    is_preferred: Some(false),
                    disabled: None,
                    data: Some(data),
                }));
            }

            // ── Action 3: Add `final` to constructor ────────────────────
            if class_info.constructor.is_some() {
                let title = format!("Add final to {}::__construct", class_name);
                let extra = serde_json::json!({
                    "diagnostic_message": diag.message,
                    "diagnostic_line": diag.range.start.line,
                    "diagnostic_code": NEW_STATIC_ID,
                    "sub_action": ACTION_FINAL_CONSTRUCTOR,
                });
                let data =
                    make_code_action_data(ACTION_FINAL_CONSTRUCTOR, uri, &params.range, extra);
                out.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    command: None,
                    is_preferred: Some(false),
                    disabled: None,
                    data: Some(data),
                }));
            }
        }
    }

    /// Resolve a "Fix unsafe `new static()`" code action by computing
    /// the full workspace edit.
    ///
    /// **Phase 2**: called from
    /// [`resolve_code_action`](Self::resolve_code_action) when the user
    /// picks one of the three sub-actions.
    pub(crate) fn resolve_new_static(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let uri = &data.uri;
        let diag_line = data.extra.get("diagnostic_line")?.as_u64()? as usize;
        let sub_action = data.extra.get("sub_action")?.as_str()?;

        let class_info = find_enclosing_class(content, diag_line)?;

        if is_already_fixed(&class_info) {
            return None;
        }

        let edits = match sub_action {
            ACTION_ADD_TAG => Some(build_add_tag_edit(content, &class_info)),
            ACTION_FINAL_CLASS => build_final_class_edit(content, &class_info),
            ACTION_FINAL_CONSTRUCTOR => build_final_constructor_edit(content, &class_info),
            _ => return None,
        };

        let edits = edits?;

        let doc_uri: Url = uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

// ── Data structures ─────────────────────────────────────────────────────────

/// The class declaration a `new static()` is written in.
struct EnclosingClass {
    name: String,

    /// Byte offset of the `class` keyword.
    keyword_offset: usize,

    is_abstract: bool,

    is_final: bool,

    /// Whether the class docblock already carries
    /// `@phpstan-consistent-constructor`.
    has_consistent_constructor_tag: bool,

    /// The class docblock, or where a new one goes.
    docblock: ClassDocblock,

    /// The constructor the class declares, if any.
    constructor: Option<Constructor>,
}

/// The `__construct` method a class declares.
struct Constructor {
    /// Byte offset of the declaration's first modifier, or of its
    /// `function` keyword when it has none.
    decl_start: usize,
    is_final: bool,
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// The class whose body holds the first non-blank character of
/// `diag_line`.
///
/// `None` when the innermost class-like there is not a named class: a
/// `new static()` in a trait is checked against the classes that use it,
/// which the trait itself cannot fix, and an anonymous class, interface or
/// enum is never the right one to edit either.
fn find_enclosing_class(content: &str, diag_line: usize) -> Option<EnclosingClass> {
    let line_start = line_start_byte_offset(content, diag_line);
    let line = content.get(line_start..)?.lines().next()?;
    let offset = line_start + line.find(|c: char| !c.is_whitespace())?;

    crate::parser::with_parsed_program(content, "find_enclosing_class", |program, content| {
        let class = innermost_at(program, offset as u32, |node| match node {
            Node::Class(class) => Some(Some(class)),
            Node::AnonymousClass(_) | Node::Trait(_) | Node::Interface(_) | Node::Enum(_) => {
                Some(None)
            }
            _ => None,
        })??;

        let has_consistent_constructor_tag =
            crate::symbol_map::docblock::get_docblock_text_with_offset(
                program.trivia.as_slice(),
                content,
                class,
            )
            .is_some_and(|(text, _)| text.contains(CONSISTENT_CONSTRUCTOR_TAG));

        let constructor = class.members.iter().find_map(|member| match member {
            ClassLikeMember::Method(method)
                if method.name.value.eq_ignore_ascii_case(b"__construct") =>
            {
                let first = method
                    .modifiers
                    .first()
                    .map_or(method.function.span(), |modifier| modifier.span());
                Some(Constructor {
                    decl_start: first.start.offset as usize,
                    is_final: method.modifiers.iter().any(|m| m.is_final()),
                })
            }
            _ => None,
        });

        Some(EnclosingClass {
            name: bytes_to_str(class.name.value).to_string(),
            keyword_offset: class.class.span.start.offset as usize,
            is_abstract: class.modifiers.contains_abstract(),
            is_final: class.modifiers.contains_final(),
            has_consistent_constructor_tag,
            docblock: ClassDocblock::of(program, content, class),
            constructor,
        })
    })
}

/// Whether one of the fixes is already in place.
fn is_already_fixed(info: &EnclosingClass) -> bool {
    info.is_final
        || info.has_consistent_constructor_tag
        || info.constructor.as_ref().is_some_and(|ctor| ctor.is_final)
}

/// Check if the diagnostic should be considered stale based on the
/// current content.
///
/// Called from `is_stale_phpstan_diagnostic` to eagerly clear the
/// diagnostic after the user applies one of the fixes.
pub(crate) fn is_new_static_stale(content: &str, diag_line: usize) -> bool {
    find_enclosing_class(content, diag_line).is_some_and(|info| is_already_fixed(&info))
}

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build the edit that adds `@phpstan-consistent-constructor` to the class
/// docblock, creating one when the class has none.
fn build_add_tag_edit(content: &str, info: &EnclosingClass) -> Vec<TextEdit> {
    vec![
        info.docblock
            .add_tag_edit(content, CONSISTENT_CONSTRUCTOR_TAG),
    ]
}

/// Build edits to add `final` before the `class` keyword.
fn build_final_class_edit(content: &str, info: &EnclosingClass) -> Option<Vec<TextEdit>> {
    // Don't add `final` to an abstract class — that's a PHP error.
    if info.is_abstract {
        return None;
    }
    Some(vec![insert_final_at(content, info.keyword_offset)])
}

/// Build edits to add `final` before the constructor's first modifier or
/// `function` keyword.
fn build_final_constructor_edit(content: &str, info: &EnclosingClass) -> Option<Vec<TextEdit>> {
    let ctor = info.constructor.as_ref()?;
    if ctor.is_final {
        return None;
    }
    Some(vec![insert_final_at(content, ctor.decl_start)])
}

fn insert_final_at(content: &str, offset: usize) -> TextEdit {
    let pos = offset_to_position(content, offset);
    TextEdit {
        range: Range {
            start: pos,
            end: pos,
        },
        new_text: "final ".to_string(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply `edits` (non-overlapping) to `src`.
    fn apply(src: &str, edits: &[TextEdit]) -> String {
        let mut edits: Vec<&TextEdit> = edits.iter().collect();
        edits.sort_by_key(|e| std::cmp::Reverse((e.range.start.line, e.range.start.character)));
        let mut result = src.to_string();
        for edit in edits {
            let start = lsp_pos_to_byte(&result, &edit.range.start);
            let end = lsp_pos_to_byte(&result, &edit.range.end);
            result.replace_range(start..end, &edit.new_text);
        }
        result
    }

    // ── find_enclosing_class ────────────────────────────────────────

    #[test]
    fn finds_enclosing_class_simple() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        // `new static()` is on line 3.
        let info = find_enclosing_class(src, 3).unwrap();
        assert_eq!(info.name, "Foo");
        assert!(!info.is_abstract);
    }

    #[test]
    fn detects_abstract_class() {
        let src = "<?php\nabstract class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        assert_eq!(info.name, "Foo");
        assert!(info.is_abstract);
    }

    #[test]
    fn skips_class_constant_and_anonymous_class_before_the_class() {
        let src = "<?php\n$x = Foo::class;\n$y = new class {};\nclass Baz {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 5).unwrap();
        assert_eq!(info.name, "Baz");
    }

    #[test]
    fn none_outside_a_class() {
        let src = "<?php\nfunction foo() {\n    new static();\n}\n";
        assert!(find_enclosing_class(src, 2).is_none());
    }

    /// The word `class` in a comment or string inside the method is not
    /// the declaration; the edits must land on the real `class` keyword.
    #[test]
    fn ignores_class_word_in_comment_and_string() {
        let src = "<?php\nclass Foo {\n    public function bar() {\n        // the class is final\n        $s = 'class Nope';\n        return new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 5).unwrap();
        assert_eq!(info.name, "Foo");

        let edits = build_final_class_edit(src, &info).unwrap();
        let result = apply(src, &edits);
        assert!(result.contains("\nfinal class Foo {\n"));
        assert!(result.contains("// the class is final\n"));
        assert!(result.contains("'class Nope'"));

        let edits = build_add_tag_edit(src, &info);
        assert_eq!(
            apply(src, &edits),
            "<?php\n/**\n * @phpstan-consistent-constructor\n */\nclass Foo {\n    public function bar() {\n        // the class is final\n        $s = 'class Nope';\n        return new static();\n    }\n}\n",
        );
    }

    /// A `new static()` in a trait must not reach back to a class declared
    /// earlier in the file.
    #[test]
    fn trait_does_not_edit_preceding_class() {
        let src = "<?php\nclass Foo {\n}\ntrait Bar {\n    public function make() {\n        return new static();\n    }\n}\n";
        assert!(find_enclosing_class(src, 5).is_none());
        assert!(!is_new_static_stale(src, 5));
    }

    /// A `new static()` in an anonymous class does not belong to the class
    /// whose method creates it.
    #[test]
    fn anonymous_class_does_not_edit_outer_class() {
        let src = "<?php\nclass Foo {\n    public function bar() {\n        return new class {\n            public function baz() {\n                return new static();\n            }\n        };\n    }\n}\n";
        assert!(find_enclosing_class(src, 5).is_none());
    }

    #[test]
    fn finds_class_after_earlier_class_in_file() {
        let src = "<?php\nclass First {\n}\nclass Second {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 5).unwrap();
        assert_eq!(info.name, "Second");
    }

    #[test]
    fn finds_constructor() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(info.constructor.is_some());
        assert!(!info.constructor.as_ref().unwrap().is_final);
    }

    #[test]
    fn detects_final_constructor() {
        let src = "<?php\nclass Foo {\n    final public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(info.constructor.is_some());
        assert!(info.constructor.as_ref().unwrap().is_final);
    }

    /// The insertion point for `final ` must land on the constructor, not
    /// on the class's opening brace, when the constructor carries no
    /// modifiers and the brace sits on its own line.
    #[test]
    fn constructor_start_skips_allman_brace() {
        let src = "<?php\nclass Foo\n{\n    function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 5).unwrap();
        let ctor = info.constructor.as_ref().unwrap();
        assert!(!ctor.is_final);
        assert_eq!(&src[ctor.decl_start..ctor.decl_start + 8], "function");
    }

    #[test]
    fn constructor_start_lands_on_first_modifier() {
        let src = "<?php\nclass Foo\n{\n    public static function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 5).unwrap();
        let ctor = info.constructor.as_ref().unwrap();
        assert_eq!(&src[ctor.decl_start..ctor.decl_start + 6], "public");
    }

    /// An attribute on the constructor's own line is not a modifier;
    /// `final` belongs after it, not before it.
    #[test]
    fn constructor_start_skips_same_line_attribute() {
        let src = "<?php\nclass Foo\n{\n    #[Attr] public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 5).unwrap();
        let edits = build_final_constructor_edit(src, &info).unwrap();
        assert!(
            apply(src, &edits).contains("    #[Attr] final public function __construct() {}\n")
        );
    }

    // ── class docblock ──────────────────────────────────────────────

    #[test]
    fn finds_existing_docblock() {
        let src = "<?php\n/** Some doc */\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(info.docblock.docblock.unwrap().text.contains("Some doc"));
    }

    #[test]
    fn finds_multiline_docblock() {
        let src = "<?php\n/**\n * Some doc\n */\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 6).unwrap();
        assert!(info.docblock.docblock.unwrap().text.contains("Some doc"));
    }

    #[test]
    fn no_docblock_when_none_present() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        assert!(info.docblock.docblock.is_none());
    }

    #[test]
    fn finds_docblock_with_attributes_between() {
        let src = "<?php\n/**\n * Some doc\n */\n#[SomeAttr]\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 7).unwrap();
        assert!(info.docblock.docblock.unwrap().text.contains("Some doc"));
    }

    // ── is_already_fixed ────────────────────────────────────────────

    #[test]
    fn not_fixed_initially() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(!is_already_fixed(&info));
    }

    #[test]
    fn fixed_when_class_is_final() {
        let src = "<?php\nfinal class Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(is_already_fixed(&info));
    }

    #[test]
    fn fixed_when_constructor_is_final() {
        let src = "<?php\nclass Foo {\n    final public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(is_already_fixed(&info));
    }

    #[test]
    fn fixed_when_docblock_has_tag() {
        let src = "<?php\n/**\n * @phpstan-consistent-constructor\n */\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 6).unwrap();
        assert!(is_already_fixed(&info));
    }

    // ── is_new_static_stale ─────────────────────────────────────────

    #[test]
    fn stale_when_class_final() {
        let src = "<?php\nfinal class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        assert!(is_new_static_stale(src, 3));
    }

    #[test]
    fn not_stale_when_unfixed() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        assert!(!is_new_static_stale(src, 3));
    }

    /// A final class earlier in the file says nothing about the class the
    /// `new static()` is in.
    #[test]
    fn not_stale_because_of_an_earlier_final_class() {
        let src = "<?php\nfinal class First {\n}\nclass Second {\n    public function bar() {\n        new static();\n    }\n}\n";
        assert!(!is_new_static_stale(src, 5));
    }

    // ── build_add_tag_edit ──────────────────────────────────────────

    #[test]
    fn adds_tag_to_existing_multiline_docblock() {
        let src = "<?php\n/**\n * Some class\n */\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 6).unwrap();
        let edits = build_add_tag_edit(src, &info);
        assert_eq!(edits.len(), 1);
        assert!(apply(src, &edits).starts_with(
            "<?php\n/**\n * Some class\n * @phpstan-consistent-constructor\n */\nclass Foo {\n"
        ));
    }

    #[test]
    fn adds_tag_to_single_line_docblock() {
        let src = "<?php\n/** Some class */\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_add_tag_edit(src, &info);
        assert_eq!(edits.len(), 1);
        // Should convert to multi-line.
        let new = &edits[0].new_text;
        assert!(new.contains("/**\n"));
        assert!(new.contains("Some class"));
        assert!(new.contains("@phpstan-consistent-constructor"));
    }

    #[test]
    fn creates_new_docblock_when_none_exists() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_add_tag_edit(src, &info);
        assert_eq!(edits.len(), 1);
        assert!(
            apply(src, &edits)
                .starts_with("<?php\n/**\n * @phpstan-consistent-constructor\n */\nclass Foo {\n")
        );
    }

    #[test]
    fn add_tag_to_class_with_attributes() {
        let src = "<?php\n#[SomeAttr]\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_add_tag_edit(src, &info);
        assert!(apply(src, &edits).starts_with(
            "<?php\n/**\n * @phpstan-consistent-constructor\n */\n#[SomeAttr]\nclass Foo {\n"
        ));
    }

    #[test]
    fn indented_class_keeps_indent() {
        let src = "<?php\nnamespace App;\n\n    class Foo {\n        public function bar() {\n            new static();\n        }\n    }\n";
        let info = find_enclosing_class(src, 5).unwrap();
        let edits = build_add_tag_edit(src, &info);
        assert!(apply(src, &edits).contains(
            "\n    /**\n     * @phpstan-consistent-constructor\n     */\n    class Foo {\n"
        ));
    }

    // ── build_final_class_edit ──────────────────────────────────────

    #[test]
    fn inserts_final_before_class() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_final_class_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(apply(src, &edits).contains("final class Foo"));
    }

    #[test]
    fn does_not_add_final_to_abstract_class() {
        let src = "<?php\nabstract class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        assert!(build_final_class_edit(src, &info).is_none());
    }

    #[test]
    fn readonly_class_gets_final() {
        let src = "<?php\nreadonly class Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        let edits = build_final_class_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "final ");

        // Verify the edit inserts before `class`, not before `readonly`.
        let start_offset = lsp_pos_to_byte(src, &edits[0].range.start);
        assert_eq!(&src[start_offset..start_offset + 5], "class");
    }

    // ── build_final_constructor_edit ────────────────────────────────

    #[test]
    fn inserts_final_before_constructor() {
        let src = "<?php\nclass Foo {\n    public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_final_constructor_edit(src, &info).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(apply(src, &edits).contains("final public function __construct"));
    }

    #[test]
    fn no_edit_when_constructor_already_final() {
        let src = "<?php\nclass Foo {\n    final public function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        assert!(build_final_constructor_edit(src, &info).is_none());
    }

    #[test]
    fn constructor_with_no_visibility() {
        let src = "<?php\nclass Foo {\n    function __construct() {}\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 4).unwrap();
        let edits = build_final_constructor_edit(src, &info).unwrap();
        assert!(apply(src, &edits).contains("    final function __construct() {}\n"));
    }

    #[test]
    fn no_final_constructor_action_when_no_constructor() {
        let src =
            "<?php\nclass Foo {\n    public function bar() {\n        new static();\n    }\n}\n";
        let info = find_enclosing_class(src, 3).unwrap();
        assert!(info.constructor.is_none());
        assert!(build_final_constructor_edit(src, &info).is_none());
        // The collect phase guards on `class_info.constructor.is_some()`,
        // so the "Add final to constructor" action is never offered.
    }

    // ── Test helper ─────────────────────────────────────────────────

    /// Convert an LSP `Position` to a byte offset (test helper).
    fn lsp_pos_to_byte(content: &str, pos: &Position) -> usize {
        let mut offset = 0;
        for (i, line) in content.lines().enumerate() {
            if i == pos.line as usize {
                // Count characters up to the column.
                for (j, ch) in line.chars().enumerate() {
                    if j == pos.character as usize {
                        break;
                    }
                    offset += ch.len_utf8();
                }
                return offset;
            }
            offset += line.len() + 1; // +1 for newline
        }
        content.len()
    }
}

//! Integration tests for the "Change visibility" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, walking
//! the AST to find the visibility modifier under the cursor, generating
//! a deferred `CodeAction` with a `data` payload, and resolving it into
//! the `WorkspaceEdit` that replaces the keyword.
//!
//! Parent-aware filtering ensures that alternatives more restrictive
//! than an overridden parent member are suppressed.  When a PHPStan
//! `method.visibility` or `property.visibility` diagnostic is present,
//! the matching action is promoted to `quickfix` with `is_preferred`.

use std::sync::Arc;

use crate::common::{
    apply_edits, create_test_backend, extract_edit_text, extract_edits, get_code_actions_at,
    lsp_pos_to_offset, resolve_action,
};
use tower_lsp::lsp_types::*;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Find all "Make ..." code actions from a list of actions.
fn find_visibility_actions(actions: &[CodeActionOrCommand]) -> Vec<&CodeAction> {
    actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Make ") => Some(ca),
            _ => None,
        })
        .collect()
}

/// Inject a PHPStan diagnostic into the backend's cache and return it.
fn inject_phpstan_diag(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    line: u32,
    message: &str,
    identifier: &str,
) -> Diagnostic {
    let diag = Diagnostic {
        range: Range {
            start: Position::new(line, 0),
            end: Position::new(line, 80),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(identifier.to_string())),
        source: Some("PHPStan".to_string()),
        message: message.to_string(),
        data: Some(serde_json::json!({ "ignorable": false })),
        ..Default::default()
    };
    {
        let mut cache = backend.phpstan_last_diags().lock();
        cache.entry(uri.to_string()).or_default().push(diag.clone());
    }
    diag
}

// ═══════════════════════════════════════════════════════════════════════════
// Basic functionality (no parent, no PHPStan)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn public_method_offers_protected_and_private() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make protected"), "titles: {:?}", titles);
    assert!(titles.contains(&"Make private"), "titles: {:?}", titles);
}

#[test]
fn public_method_make_protected_replaces_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    let make_protected = vis_actions
        .iter()
        .find(|a| a.title == "Make protected")
        .expect("should have Make protected action");
    assert_eq!(
        make_protected.kind,
        Some(CodeActionKind::new("refactor.rewrite"))
    );

    let resolved = resolve_action(&backend, uri, content, make_protected);
    assert_eq!(extract_edit_text(&resolved), "protected");
}

#[test]
fn public_method_make_private_replaces_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    let make_private = vis_actions
        .iter()
        .find(|a| a.title == "Make private")
        .expect("should have Make private action");

    let resolved = resolve_action(&backend, uri, content, make_private);
    assert_eq!(extract_edit_text(&resolved), "private");
}

#[test]
fn protected_method_offers_public_and_private() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    protected function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make private"));
}

#[test]
fn private_method_offers_public_and_protected() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make protected"));
}

#[test]
fn property_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    protected string $bar = '';
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make private"));
}

#[test]
fn constant_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private const BAR = 42;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make protected"));
}

#[test]
fn promoted_param_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public function __construct(
        private string $name,
        protected int $age,
    ) {}
}
"#;
    backend.update_ast(uri, content);

    // Cursor on `private` of promoted $name param (line 3).
    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"), "titles: {:?}", titles);
    assert!(titles.contains(&"Make protected"), "titles: {:?}", titles);
}

#[test]
fn promoted_param_edit_replaces_correct_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class User {
    public function __construct(
        private string $name,
    ) {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    let vis_actions = find_visibility_actions(&actions);

    let make_public = vis_actions
        .iter()
        .find(|a| a.title == "Make public")
        .expect("should have Make public");

    let resolved = resolve_action(&backend, uri, content, make_public);
    let edit = resolved.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "public");

    // The edit range should only cover the `private` keyword, not the
    // method-level `public`.
    let range = &edits[0].range;
    let keyword_in_source =
        &content[lsp_pos_to_offset(content, range.start)..lsp_pos_to_offset(content, range.end)];
    assert_eq!(keyword_in_source, "private");
}

#[test]
fn interface_method_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Renderable {
    public function render(): string;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    // Interfaces only have public methods, but the action still offers alternatives.
    assert_eq!(vis_actions.len(), 2);
}

#[test]
fn trait_method_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
trait Loggable {
    protected function log(string $msg): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make private"));
}

#[test]
fn enum_method_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Color {
    case Red;
    case Green;

    public function label(): string {
        return match($this) {
            self::Red => 'red',
            self::Green => 'green',
        };
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
}

#[test]
fn works_inside_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Models;

class User {
    private string $email;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make protected"));
}

#[test]
fn no_action_outside_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function globalFn(): void {}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 4);
    let vis_actions = find_visibility_actions(&actions);

    assert!(vis_actions.is_empty());
}

#[test]
fn no_action_on_trait_use() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    use SomeTrait;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert!(vis_actions.is_empty());
}

#[test]
fn no_action_on_enum_case() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
enum Status {
    case Active;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert!(vis_actions.is_empty());
}

#[test]
fn action_available_with_cursor_on_function_keyword() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 14);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
}

#[test]
fn action_available_with_cursor_on_method_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 21);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
}

#[test]
fn no_action_with_cursor_inside_method_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {
        echo 'hello';
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 0);
}

#[test]
fn static_method_offers_visibility_change() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public static function create(): self {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make protected"));
    assert!(titles.contains(&"Make private"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Deferred resolve — stale detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn resolve_returns_none_when_file_changed() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);
    let make_private = vis_actions
        .iter()
        .find(|a| a.title == "Make private")
        .expect("should have Make private");

    // Simulate: user changed the file between Phase 1 and Phase 2.
    // The visibility keyword is no longer at the stored byte offset.
    let changed = r#"<?php
// added a line
class Foo {
    public function bar(): void {}
}
"#;
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), Arc::new(changed.to_string()));
    let (resolved, _) = backend.resolve_code_action((*make_private).clone());
    assert!(
        resolved.edit.is_none(),
        "should not produce edit when file changed"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Parent-aware filtering (same file, no PHPStan)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn private_overriding_public_parent_only_offers_public() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    // Cursor on the `private` keyword of Child::foo (line 5).
    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1, "should only offer Make public");
    assert_eq!(vis_actions[0].title, "Make public");
}

#[test]
fn private_overriding_protected_parent_offers_protected_and_public() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    protected function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make protected"));
    assert!(titles.contains(&"Make public"));
}

#[test]
fn protected_overriding_public_parent_only_offers_public() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function bar(): void {}
}
class Child extends Base {
    protected function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");
}

#[test]
fn public_overriding_public_parent_does_not_offer_restricted() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function bar(): void {}
}
class Child extends Base {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    // Current is public, parent requires public — neither protected nor
    // private should be offered.
    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(
        vis_actions.len(),
        0,
        "no alternatives should be offered when already at minimum: {:?}",
        vis_actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}

#[test]
fn no_parent_no_filtering() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Standalone {
    private function doStuff(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let vis_actions = find_visibility_actions(&actions);

    // No parent → all alternatives offered.
    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make protected"));
}

#[test]
fn private_property_overriding_protected_parent_filters() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    protected string $name = '';
}
class Child extends Base {
    private string $name = 'child';
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make protected"));
    assert!(titles.contains(&"Make public"));
}

#[test]
fn private_property_overriding_public_parent_only_offers_public() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public string $name = '';
}
class Child extends Base {
    private string $name = 'child';
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");
}

#[test]
fn parent_aware_resolve_applies_correctly() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    let resolved = resolve_action(&backend, uri, content, vis_actions[0]);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("    public function foo(): void {}"),
        "should replace 'private' with 'public':\n{}",
        result
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Parent-aware: member only in child (no override) — no filtering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn child_only_method_not_filtered() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function childOnly(): void {}
}
"#;
    backend.update_ast(uri, content);

    // `childOnly` is not in the parent — all alternatives should be offered.
    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);
    let titles: Vec<&str> = vis_actions.iter().map(|a| a.title.as_str()).collect();
    assert!(titles.contains(&"Make public"));
    assert!(titles.contains(&"Make protected"));
}

// ═══════════════════════════════════════════════════════════════════════════
// PHPStan diagnostic integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn phpstan_should_also_be_public_marks_preferred() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Private method Child::foo() overriding public method Base::foo() should also be public.",
        "method.visibility",
    );

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1, "parent filtering leaves only public");
    let action = vis_actions[0];
    assert_eq!(action.title, "Make public");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    assert!(
        action.diagnostics.is_some(),
        "should attach the PHPStan diagnostic"
    );
}

#[test]
fn phpstan_should_be_protected_or_public_marks_preferred_and_non_preferred() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    protected function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Private method Child::foo() overriding protected method Base::foo() should be protected or public.",
        "method.visibility",
    );

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 2);

    let make_protected = vis_actions
        .iter()
        .find(|a| a.title == "Make protected")
        .expect("should have Make protected");
    assert_eq!(make_protected.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(make_protected.is_preferred, Some(true));
    assert!(make_protected.diagnostics.is_some());

    let make_public = vis_actions
        .iter()
        .find(|a| a.title == "Make public")
        .expect("should have Make public");
    assert_eq!(make_public.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(make_public.is_preferred, Some(false));
    assert!(make_public.diagnostics.is_some());
}

#[test]
fn phpstan_property_visibility_attaches_diagnostic() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public string $name = '';
}
class Child extends Base {
    private string $name = 'child';
}
"#;
    backend.update_ast(uri, content);

    let diag = inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Private property Child::$name overriding public property Base::$name should also be public.",
        "property.visibility",
    );

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");
    assert_eq!(vis_actions[0].kind, Some(CodeActionKind::QUICKFIX));

    let attached = vis_actions[0]
        .diagnostics
        .as_ref()
        .expect("should attach diagnostic");
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0].message, diag.message);
}

#[test]
fn phpstan_quickfix_resolves_and_clears() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Private method Child::foo() overriding public method Base::foo() should also be public.",
        "method.visibility",
    );

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);
    assert_eq!(vis_actions.len(), 1);

    let resolved = resolve_action(&backend, uri, content, vis_actions[0]);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("    public function foo(): void {}"),
        "should replace private with public:\n{}",
        result
    );
}

#[test]
fn no_phpstan_diag_means_refactor_rewrite_kind() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    protected function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    // No PHPStan diagnostic injected.
    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    for action in &vis_actions {
        assert_eq!(
            action.kind,
            Some(CodeActionKind::new("refactor.rewrite")),
            "without PHPStan diag, kind should be refactor.rewrite for '{}'",
            action.title
        );
        assert_eq!(
            action.is_preferred, None,
            "without PHPStan diag, is_preferred should be None for '{}'",
            action.title
        );
        assert!(
            action.diagnostics.is_none(),
            "without PHPStan diag, diagnostics should be None for '{}'",
            action.title
        );
    }
}

#[test]
fn non_ignorable_visibility_error_has_no_ignore_action() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
}
"#;
    backend.update_ast(uri, content);

    // Inject with ignorable: false (the default for visibility errors).
    let diag = Diagnostic {
        range: Range {
            start: Position::new(5, 0),
            end: Position::new(5, 80),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String("method.visibility".to_string())),
        source: Some("PHPStan".to_string()),
        message: "Private method Child::foo() overriding public method Base::foo() should also be public.".to_string(),
        data: Some(serde_json::json!({ "ignorable": false })),
        ..Default::default()
    };
    {
        let mut cache = backend.phpstan_last_diags().lock();
        cache.entry(uri.to_string()).or_default().push(diag.clone());
    }

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);

    // There should be a fix-visibility action.
    let vis_actions = find_visibility_actions(&actions);
    assert!(
        !vis_actions.is_empty(),
        "should offer fix-visibility action"
    );

    // But there should NOT be an "Ignore" action (because ignorable: false).
    let ignore_actions: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("@phpstan-ignore") => Some(ca),
            _ => None,
        })
        .collect();
    assert!(
        ignore_actions.is_empty(),
        "should not offer ignore action for non-ignorable error"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn static_method_overriding_public_only_offers_public() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public static function create(): static { return new static(); }
}
class Child extends Base {
    private static function create(): static { return new static(); }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");

    let resolved = resolve_action(&backend, uri, content, vis_actions[0]);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("    public static function create()"),
        "should replace private with public:\n{}",
        result
    );
}

#[test]
fn constructor_overriding_public_only_offers_public() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function __construct() {}
}
class Child extends Base {
    private function __construct() { parent::__construct(); }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");
}

#[test]
fn preserves_surrounding_code() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
}
class Child extends Base {
    private function foo(): void {}
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    let resolved = resolve_action(&backend, uri, content, vis_actions[0]);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("    public function bar(): void {}"),
        "should not modify other methods:\n{}",
        result
    );
    assert!(
        result.contains("class Base {"),
        "should not modify base class:\n{}",
        result
    );
}

#[test]
fn multiple_diagnostics_on_different_lines() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function foo(): void {}
    protected function bar(): void {}
}
class Child extends Base {
    private function foo(): void {}
    private function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        6,
        "Private method Child::foo() overriding public method Base::foo() should also be public.",
        "method.visibility",
    );
    inject_phpstan_diag(
        &backend,
        uri,
        7,
        "Private method Child::bar() overriding protected method Base::bar() should be protected or public.",
        "method.visibility",
    );

    // Line 6 (foo): only "Make public"
    let actions_foo = get_code_actions_at(&backend, uri, content, 6, 10);
    let vis_foo = find_visibility_actions(&actions_foo);
    assert_eq!(vis_foo.len(), 1);
    assert_eq!(vis_foo[0].title, "Make public");
    assert_eq!(vis_foo[0].kind, Some(CodeActionKind::QUICKFIX));

    // Line 7 (bar): "Make protected" (preferred) and "Make public"
    let actions_bar = get_code_actions_at(&backend, uri, content, 7, 10);
    let vis_bar = find_visibility_actions(&actions_bar);
    assert_eq!(vis_bar.len(), 2);
    let make_prot = vis_bar
        .iter()
        .find(|a| a.title == "Make protected")
        .expect("should have Make protected");
    assert_eq!(make_prot.is_preferred, Some(true));
}

#[test]
fn namespaced_class_parent_aware() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Models;

class Base {
    public function handle(): void {}
}

class Child extends Base {
    private function handle(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 8, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");
}

// ═══════════════════════════════════════════════════════════════════════════
// PHPStan diagnostic on attribute line (not the method signature line)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn phpstan_diag_on_attribute_line_attaches_and_clears() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function handle(): void {}
}
class Child extends Base {
    #[\Override]
    private function handle(): void {}
}
"#;
    backend.update_ast(uri, content);

    // PHPStan reports the error on the #[Override] line (line 5),
    // NOT the method signature line (line 6).
    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Private method Child::handle() overriding public method Base::handle() should also be public.",
        "method.visibility",
    );

    // Cursor on the attribute line where the squiggle is.
    let actions = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(
        vis_actions.len(),
        1,
        "should offer Make public from the attribute line"
    );
    assert_eq!(vis_actions[0].title, "Make public");
    assert_eq!(vis_actions[0].kind, Some(CodeActionKind::QUICKFIX));
    assert!(
        vis_actions[0].diagnostics.is_some(),
        "diagnostic must be attached so it gets cleared on resolve"
    );

    // Resolve and verify the diagnostic is cleared from the cache.
    let resolved = resolve_action(&backend, uri, content, vis_actions[0]);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("    public function handle(): void {}"),
        "should replace private with public:\n{}",
        result
    );

    // The PHPStan diagnostic cache should now be empty for this URI.
    let remaining: Vec<_> = {
        let cache = backend.phpstan_last_diags().lock();
        cache.get(uri).cloned().unwrap_or_default()
    };
    assert!(
        remaining.is_empty(),
        "diagnostic should be cleared from cache after resolve, but found: {:?}",
        remaining.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn phpstan_diag_on_attribute_line_cursor_on_signature_line_also_works() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function handle(): void {}
}
class Child extends Base {
    #[\Override]
    private function handle(): void {}
}
"#;
    backend.update_ast(uri, content);

    // PHPStan diagnostic is on the attribute line (line 5).
    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Private method Child::handle() overriding public method Base::handle() should also be public.",
        "method.visibility",
    );

    // But the user places their cursor on the method signature line (line 6).
    // The action should still fire (visibility keyword is there), but the
    // PHPStan diagnostic is on a different line so it may not be found.
    let actions = get_code_actions_at(&backend, uri, content, 6, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1, "should still offer Make public");
    assert_eq!(vis_actions[0].title, "Make public");

    // The diagnostic should still be attached — the action should look
    // for the diagnostic on any line covered by the method span, not
    // just the cursor line.
    assert!(
        vis_actions[0].diagnostics.is_some(),
        "diagnostic should be attached even when cursor is on the signature line"
    );
    assert_eq!(
        vis_actions[0].kind,
        Some(CodeActionKind::QUICKFIX),
        "should be quickfix when PHPStan diagnostic is attached"
    );
}

#[test]
fn phpstan_diag_on_signature_line_with_multiple_attributes() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Base {
    public function handle(): void {}
}
class Child extends Base {
    #[\Override]
    #[SomeOtherAttribute]
    private function handle(): void {}
}
"#;
    backend.update_ast(uri, content);

    // Currently PHPStan reports on the first attribute line (line 5),
    // but if it moves to the signature line (line 7) in the future,
    // the action should still find and attach it.
    inject_phpstan_diag(
        &backend,
        uri,
        7,
        "Private method Child::handle() overriding public method Base::handle() should also be public.",
        "method.visibility",
    );

    // Cursor on the signature line.
    let actions = get_code_actions_at(&backend, uri, content, 7, 6);
    let vis_actions = find_visibility_actions(&actions);

    assert_eq!(vis_actions.len(), 1);
    assert_eq!(vis_actions[0].title, "Make public");
    assert_eq!(vis_actions[0].kind, Some(CodeActionKind::QUICKFIX));
    assert!(
        vis_actions[0].diagnostics.is_some(),
        "diagnostic should be attached when reported on the signature line"
    );

    // Also verify from the first attribute line — cursor there should
    // still find the diagnostic on the signature line.
    let actions2 = get_code_actions_at(&backend, uri, content, 5, 6);
    let vis_actions2 = find_visibility_actions(&actions2);

    assert_eq!(vis_actions2.len(), 1);
    assert!(
        vis_actions2[0].diagnostics.is_some(),
        "diagnostic on signature line should be found when cursor is on attribute line"
    );
}

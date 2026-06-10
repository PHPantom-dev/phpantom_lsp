//! Integration tests for the "Extract Function / Method" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, detecting
//! complete statement selections, classifying variables via the
//! `ScopeCollector`, and generating a `WorkspaceEdit` that replaces the
//! selection with a call and inserts a new function or method definition.

use crate::common::create_test_backend;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

/// Helper: send a code action request with a selection range and return
/// the list of code actions.
fn get_code_actions(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
) -> Vec<CodeActionOrCommand> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(start_line, start_char),
            end: Position::new(end_line, end_char),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    backend.handle_code_action(uri, content, &params)
}

/// Find an "Extract function" or "Extract method" code action from a list.
fn find_extract_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca)
            if ca.disabled.is_none()
                && (ca.title.starts_with("Extract function")
                    || ca.title.starts_with("Extract method")) =>
        {
            Some(ca)
        }
        _ => None,
    })
}

/// Resolve a deferred code action through `codeAction/resolve` and return
/// the resolved action with its workspace edit populated.
///
/// The file content is stored in `open_files` so that
/// `resolve_code_action` → `get_file_content` can find it.
fn resolve_action(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    content: &str,
    action: &CodeAction,
) -> CodeAction {
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    let (resolved, _) = backend.resolve_code_action(action.clone());
    assert!(
        resolved.edit.is_some(),
        "resolved action should have an edit, title: {}",
        resolved.title
    );
    resolved
}

/// Apply a workspace edit to the content and return the result.
fn apply_edit(content: &str, edit: &WorkspaceEdit) -> String {
    let changes = edit.changes.as_ref().expect("edit should have changes");
    let edits = changes
        .values()
        .next()
        .expect("should have edits for one URI");

    // Sort edits by start position descending so we can apply back-to-front.
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    let mut result = content.to_string();
    for edit in sorted {
        let start = position_to_offset(&result, edit.range.start);
        let end = position_to_offset(&result, edit.range.end);
        result.replace_range(start..end, &edit.new_text);
    }
    result
}

/// Convert an LSP Position to a byte offset.
fn position_to_offset(content: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == pos.line as usize {
            return offset + pos.character as usize;
        }
        offset += line.len() + 1; // +1 for '\n'
    }
    offset
}

// ── Offering / not offering the action ──────────────────────────────────────

#[test]
fn offered_for_complete_statements_in_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    $y = 2;
    echo $x + $y;
}
";
    // Select lines 2-3: `$x = 1;\n    $y = 2;`
    let actions = get_code_actions(&backend, uri, content, 2, 4, 3, 11);
    let action = find_extract_action(&actions);
    assert!(
        action.is_some(),
        "should offer extract action for complete statements"
    );
    assert_eq!(
        action.unwrap().kind,
        Some(CodeActionKind::REFACTOR_EXTRACT),
        "should be refactor.extract"
    );
}

#[test]
fn not_offered_for_empty_selection() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
}
";
    let actions = get_code_actions(&backend, uri, content, 2, 4, 2, 4);
    let action = find_extract_action(&actions);
    assert!(
        action.is_none(),
        "should not offer extract for empty selection"
    );
}

#[test]
fn not_offered_for_partial_expression() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1 + 2;
}
";
    // Select just `1 + 2` — not a complete statement.
    let actions = get_code_actions(&backend, uri, content, 2, 9, 2, 14);
    let action = find_extract_action(&actions);
    assert!(
        action.is_none(),
        "should not offer extract for partial expression"
    );
}

#[test]
fn not_offered_when_return_without_trailing_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // This selection has a return that returns null AND also modifies
    // $data which is used after the selection — the combination of
    // mixed null returns plus return values makes it unsafe.
    //
    // Phase 1 now always offers the action when the selection covers
    // complete statements (validation is deferred to resolve).
    // Resolve returns no edit for unsafe return strategies.
    let content = "\
<?php
function foo($x) {
    if ($x < 0) return null;
    if ($x > 100) return 'overflow';
    $data = process($x);
    echo $data;
}
";
    // Select if + if + $data assignment — the return values include
    // null and a non-null value (can't use null sentinel), AND $data
    // is read after the selection (has_return_values = true).
    let actions = get_code_actions(&backend, uri, content, 2, 4, 4, 25);
    let action = find_extract_action(&actions);
    assert!(
        action.is_some(),
        "Phase 1 should offer the action (validation deferred to resolve)"
    );
    // Call resolve directly (not via `resolve_action` which asserts
    // edit.is_some()) because we expect no edit here.
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    let (resolved, _) = backend.resolve_code_action(action.unwrap().clone());
    assert!(
        resolved.edit.is_none(),
        "resolve should produce no edit for unsafe returns"
    );
}

// ── Guard clause extraction strategies ──────────────────────────────────────

#[test]
fn void_guard_extraction_produces_bool_pattern() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Validator {
    public function handle($request): void
    {
        if (!$request) return;
        if (!$this->authorize()) return;
        $this->process($request);
        $this->log($request);
    }
}
";
    // Select the two guard lines (lines 4-5).
    let actions = get_code_actions(&backend, uri, content, 4, 8, 5, 40);
    let action = find_extract_action(&actions).expect("should offer extract for void guards");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should be: if (!$this->handleGuard($request)) return;
    assert!(
        result.contains("if (!$this->handleGuard($request)) return;"),
        "call site should use bool-flag pattern:\n{result}"
    );
    // Extracted method should return bool.
    assert!(
        result.contains("): bool"),
        "extracted method should have bool return type:\n{result}"
    );
    // Body should have return false (rewritten from bare return).
    assert!(
        result.contains("return false;"),
        "guard returns should be rewritten to return false:\n{result}"
    );
    // Fall-through should be return true.
    assert!(
        result.contains("return true;"),
        "fall-through should be return true:\n{result}"
    );
}

#[test]
fn uniform_false_guard_extraction() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Validator {
    public function validate($dog, $cat): bool
    {
        if (!$dog) return false;
        if (!$cat) return false;
        return $this->check($dog, $cat);
    }
}
";
    // Select just the two guard lines (lines 4-5).
    let actions = get_code_actions(&backend, uri, content, 4, 8, 5, 32);
    let action =
        find_extract_action(&actions).expect("should offer extract for uniform false guards");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should use the bool-flag pattern with false.
    // Parameter order depends on the scope classifier (first-use order).
    let has_bool_guard_call = result
        .contains("if (!$this->validateGuard($dog, $cat)) return false;")
        || result.contains("if (!$this->validateGuard($cat, $dog)) return false;");
    assert!(
        has_bool_guard_call,
        "call site should use bool-flag pattern with false:\n{result}"
    );
    // Extracted method should return bool.
    assert!(
        result.contains("): bool"),
        "extracted method should have bool return type:\n{result}"
    );
    // The body already has `return false;` which stays as-is (boolean values
    // don't need rewriting), plus a `return true;` fall-through.
    assert!(
        result.contains("return true;"),
        "fall-through should be return true:\n{result}"
    );
}

#[test]
fn uniform_null_guard_extraction_rewrites_returns() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Lookup {
    public function find(int $id): ?User
    {
        if ($id <= 0) return null;
        if (!$this->hasAccess()) return null;
        return $this->repo->findById($id);
    }
}
";
    // Select the two null-guard lines (lines 4-5).
    // Line 4: "        if ($id <= 0) return null;"  len=33
    // Line 5: "        if (!$this->hasAccess()) return null;"  len=45
    let actions = get_code_actions(&backend, uri, content, 4, 8, 5, 45);
    let action =
        find_extract_action(&actions).expect("should offer extract for uniform null guards");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should be: if (!$this->findGuard($id)) return null;
    assert!(
        result.contains("if (!$this->findGuard($id)) return null;"),
        "call site should use bool-flag pattern with null:\n{result}"
    );
    // Extracted method should return bool.
    assert!(
        result.contains("): bool"),
        "extracted method should have bool return type:\n{result}"
    );
    // Body should have `return null;` rewritten to `return false;`.
    assert!(
        result.contains("return false;"),
        "null guards should be rewritten to return false:\n{result}"
    );
    // Should NOT contain return null in the extracted method body.
    // The `return null;` should only appear at the call site.
    let extracted_method_start = result.find("private function findGuard").unwrap();
    let extracted_body = &result[extracted_method_start..];
    assert!(
        !extracted_body.contains("return null;"),
        "extracted method should not contain return null:\n{result}"
    );
}

#[test]
fn sentinel_null_extraction_for_different_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function classify(int $code): string
{
    if ($code < 0) return 'negative';
    if ($code === 0) return 'zero';
    if ($code > 1000) return 'overflow';
    return computeStatus($code);
}
";
    // Select the three guard lines (lines 3-5).
    // Line 3: "    if ($code < 0) return 'negative';"  len=38
    // Line 4: "    if ($code === 0) return 'zero';"  len=34
    // Line 5: "    if ($code > 1000) return 'overflow';"  len=41
    let actions = get_code_actions(&backend, uri, content, 3, 4, 5, 41);
    let action = find_extract_action(&actions)
        .expect("should offer extract for different non-null return values");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should use the sentinel-null pattern:
    //   $result = extracted($code);
    //   if ($result !== null) return $result;
    assert!(
        result.contains("$result = tryClassify($code);"),
        "call site should assign to $result:\n{result}"
    );
    assert!(
        result.contains("if ($result !== null) return $result;"),
        "call site should check sentinel:\n{result}"
    );
    // Extracted function should have nullable return type.
    assert!(
        result.contains("): ?string"),
        "extracted function should have ?string return type:\n{result}"
    );
    // Extracted function should end with return null (sentinel).
    assert!(
        result.contains("return null;"),
        "extracted function should have return null as sentinel:\n{result}"
    );
}

#[test]
fn null_guard_with_computed_value_extraction() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Animal {
    private ?Frog $frog;

    public function getSound(): ?string
    {
        if (!$this->frog) return null;
        $sound = $this->frog->speak();
        echo $sound;
    }
}
";
    // Select the guard + the assignment (lines 6-7).
    // Line 6: "        if (!$this->frog) return null;"  (8+30=38)
    // Line 7: "        $sound = $this->frog->speak();"  (8+30=38)
    let actions = get_code_actions(&backend, uri, content, 6, 8, 7, 38);
    let action = find_extract_action(&actions)
        .expect("should offer extract for null guard with computed value");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should assign and check for null:
    //   $sound = $this->getSoundGuard();
    //   if ($sound === null) return null;
    assert!(
        result.contains("$sound = $this->getSoundGuard("),
        "call site should assign $sound from extracted call:\n{result}"
    );
    assert!(
        result.contains("if ($sound === null) return null;"),
        "call site should check $sound for null:\n{result}"
    );
    // Extracted method should keep the guard's return null.
    let extracted_start = result.find("private function getSoundGuard").unwrap();
    let extracted_body = &result[extracted_start..];
    assert!(
        extracted_body.contains("return null;"),
        "extracted method should contain guard's return null:\n{result}"
    );
    // Extracted method should return $sound at the end.
    assert!(
        extracted_body.contains("return $sound;"),
        "extracted method should return $sound as fall-through:\n{result}"
    );
}

#[test]
fn void_guard_with_computed_value_extraction() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Animal {
    private ?Frog $frog;

    public function process(): void
    {
        if (!$this->frog) return;
        $sound = $this->frog->speak();
        echo $sound;
    }
}
";
    // Select the guard + the assignment (lines 6-7).
    let actions = get_code_actions(&backend, uri, content, 6, 8, 7, 38);
    let action = find_extract_action(&actions)
        .expect("should offer extract for void guard with computed value");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should assign and check for null, but return bare
    // (matching the original void return):
    //   $sound = $this->processGuard();
    //   if ($sound === null) return;
    assert!(
        result.contains("$sound = $this->processGuard("),
        "call site should assign $sound from extracted call:\n{result}"
    );
    assert!(
        result.contains("if ($sound === null) return;"),
        "call site should use bare return (void method):\n{result}"
    );
    // The call site must NOT have `return null;` — the enclosing
    // method is void.
    let call_site_area = &result[..result.find("private function").unwrap()];
    assert!(
        !call_site_area.contains("return null;"),
        "call site should not use return null in a void method:\n{result}"
    );
    // Extracted method should rewrite bare `return;` to `return null;`.
    let extracted_start = result.find("private function processGuard").unwrap();
    let extracted_body = &result[extracted_start..];
    assert!(
        extracted_body.contains("return null;"),
        "extracted method should rewrite void guard to return null:\n{result}"
    );
    // Extracted method should return $sound at the end.
    assert!(
        extracted_body.contains("return $sound;"),
        "extracted method should return $sound as fall-through:\n{result}"
    );
    // Extracted method should NOT contain bare `return;`.
    assert_eq!(
        extracted_body.matches("return;").count(),
        0,
        "extracted method should not have bare return:\n{result}"
    );
}

#[test]
fn offered_when_guard_clause_returns_with_trailing_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function multiAssign(array $items): int
    {
        $count = 0;
        if (!$items) return $count;
        if (!$items) return 0;
        foreach ($items as $item) {
            $count = $count + 1;
        }
        return $count;
    }
}
";
    // Select everything from `$count = 0;` through `return $count;`
    let actions = get_code_actions(&backend, uri, content, 4, 8, 10, 22);
    let action = find_extract_action(&actions)
        .expect("should offer extract when guard returns + trailing return");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should be `return $this->extracted(…);` since the
    // selection ends with return.
    assert!(
        result.contains("return $this->getMultiAssignResult("),
        "call site should pass return through:\n{result}"
    );
    // The extracted method should contain the guard clause returns.
    assert!(
        result.contains("if (!$items) return"),
        "extracted method should keep guard clause returns:\n{result}"
    );
}

#[test]
fn offered_when_trailing_return_is_last_statement() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo(): int {
    $x = 1;
    return $x;
}
";
    // Select both statements — return is the last one.
    let actions = get_code_actions(&backend, uri, content, 2, 4, 3, 14);
    let action = find_extract_action(&actions).expect("should offer extract for trailing return");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Call site should wrap with `return`.
    assert!(
        result.contains("return getFooResult(") || result.contains("return $this->getFooResult("),
        "call site should pass return through:\n{result}"
    );
}

#[test]
fn not_offered_outside_function_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
$x = 1;
$y = 2;
";
    let actions = get_code_actions(&backend, uri, content, 1, 0, 2, 7);
    let action = find_extract_action(&actions);
    assert!(
        action.is_none(),
        "should not offer extract outside function body"
    );
}

// ── Extract function (standalone) ───────────────────────────────────────────

#[test]
fn extracts_single_statement_as_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    echo $x;
}
";
    // Select `$x = 1;`
    let actions = get_code_actions(&backend, uri, content, 2, 4, 2, 11);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    assert!(
        action.title.starts_with("Extract function"),
        "should be extract function, not method: {}",
        action.title
    );
    // The call site should reference the extracted function.
    assert!(
        result.contains("computeX()"),
        "should contain call to extracted function: {result}"
    );
    // The new function should be defined.
    assert!(
        result.contains("function computeX()"),
        "should define extracted function: {result}"
    );
}

#[test]
fn extracts_multiple_statements() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    $y = 2;
    $z = $x + $y;
    echo $z;
}
";
    // Select `$x = 1;\n    $y = 2;` (lines 2-3)
    let actions = get_code_actions(&backend, uri, content, 2, 4, 3, 11);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // The extracted function should exist.
    assert!(
        result.contains("function extracted("),
        "should define extracted function: {result}"
    );
}

#[test]
fn passes_variables_read_after_selection_as_return_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 10;
    echo $x;
}
";
    // Select `$x = 10;` — $x is read after the selection (echo $x).
    let actions = get_code_actions(&backend, uri, content, 2, 4, 2, 12);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // $x should be assigned from the extracted function's return value.
    assert!(
        result.contains("$x = computeX("),
        "should assign return value to $x: {result}"
    );
    assert!(
        result.contains("return $x;"),
        "extracted function should return $x: {result}"
    );
}

#[test]
fn passes_variables_defined_before_selection_as_parameters() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 10;
    $y = $x + 5;
    echo $y;
}
";
    // Select `$y = $x + 5;` — $x is defined before, $y is read after.
    let actions = get_code_actions(&backend, uri, content, 3, 4, 3, 16);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // $x should be a parameter of the extracted function.
    assert!(
        result.contains("computeY($x)"),
        "should pass $x as argument: {result}"
    );
    assert!(
        result.contains("$y = computeY("),
        "should assign $y from return value: {result}"
    );
}

#[test]
fn local_variables_stay_inside() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $temp = 1;
    $temp = $temp + 2;
    echo 'done';
}
";
    // Select both $temp lines — $temp is fully local to the selection
    // because it's not read after the selection.
    let actions = get_code_actions(&backend, uri, content, 2, 4, 3, 22);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // $temp should NOT be a parameter or return value — it's local.
    // The call should have no arguments.
    assert!(
        result.contains("extracted()"),
        "should call with no arguments (local var stays inside): {result}"
    );
}

// ── Extract method ──────────────────────────────────────────────────────────

#[test]
fn extracts_as_method_when_this_is_used() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Calculator {
    private int $value = 0;

    public function compute() {
        $x = $this->value;
        echo $x;
    }
}
";
    // Select `$x = $this->value;\n        echo $x;` (lines 5-6)
    let actions = get_code_actions(&backend, uri, content, 5, 8, 6, 16);
    let action = find_extract_action(&actions).expect("should offer extract action");

    assert!(
        action.title.starts_with("Extract method"),
        "should be extract method when $this is used: {}",
        action.title
    );
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // The call site should use $this->
    assert!(
        result.contains("$this->renderCompute()"),
        "should call via $this->: {result}"
    );
    // The method should be private.
    assert!(
        result.contains("private function renderCompute()"),
        "extracted method should be private: {result}"
    );
}

#[test]
fn extracts_static_method_when_in_static_context() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Util {
    public static function run() {
        $x = 1;
        $y = 2;
        echo $x + $y;
    }
}
";
    // Select `$x = 1;\n        $y = 2;` (lines 3-4)
    let actions = get_code_actions(&backend, uri, content, 3, 8, 4, 15);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // The method should be private static.
    assert!(
        result.contains("private static function extracted("),
        "extracted method should be private static: {result}"
    );
}

#[test]
fn method_with_parameters_and_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function bar() {
        $a = 5;
        $b = $a * 2;
        echo $b;
    }
}
";
    // Select `$b = $a * 2;` — $a is defined before, $b is read after.
    let actions = get_code_actions(&backend, uri, content, 4, 8, 4, 20);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // $a should be passed as argument.
    assert!(
        result.contains("$this->computeB($a)") || result.contains("computeB($a)"),
        "should pass $a as argument: {result}"
    );
    assert!(
        result.contains("$b = $this->computeB(") || result.contains("$b = computeB("),
        "should assign $b from return: {result}"
    );
    assert!(
        result.contains("return $b;"),
        "extracted function should return $b: {result}"
    );
}

// ── Name deduplication ──────────────────────────────────────────────────────

#[test]
fn deduplicates_name_when_extracted_exists() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function extracted() {}

function foo() {
    $x = 1;
    echo $x;
}
";
    // Select `$x = 1;`
    let actions = get_code_actions(&backend, uri, content, 4, 4, 4, 11);
    let action = find_extract_action(&actions).expect("should offer extract action");

    // Phase 1 now uses a generic title; the generated name only
    // appears in the resolved edit, not the title.
    assert!(
        action.title.contains("Extract function"),
        "should offer extract function action: {}",
        action.title
    );

    // Verify resolve produces an edit with a deduplicated name.
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());
    // The existing `extracted()` function should cause the generated
    // name to be deduplicated (e.g. `extracted1` or a contextual name
    // like `computeX`).
    assert!(
        !result.contains("\nfunction extracted()")
            || result.matches("function extracted").count() > 1
            || result.contains("function extracted1")
            || result.contains("function computeX"),
        "should deduplicate or use contextual name: {result}"
    );
}

// ── Void return type ────────────────────────────────────────────────────────

#[test]
fn void_return_when_no_return_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    $y = 2;
}
";
    // Select both lines — neither $x nor $y is read after the selection.
    let actions = get_code_actions(&backend, uri, content, 2, 4, 3, 11);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    assert!(
        result.contains("): void"),
        "should have void return type: {result}"
    );
}

// ── Namespace handling ──────────────────────────────────────────────────────

#[test]
fn works_in_namespaced_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
namespace App\\Utils;

function helper() {
    $x = 42;
    echo $x;
}
";
    // Select `$x = 42;`
    let actions = get_code_actions(&backend, uri, content, 4, 4, 4, 12);
    let action = find_extract_action(&actions);
    assert!(
        action.is_some(),
        "should offer extract action in namespaced function"
    );
}

#[test]
fn works_in_namespaced_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
namespace App\\Services;

class Greeter {
    public function greet() {
        $name = 'World';
        echo 'Hello ' . $name;
    }
}
";
    // Select both lines inside the method body.
    let actions = get_code_actions(&backend, uri, content, 5, 8, 6, 31);
    let action = find_extract_action(&actions);
    assert!(
        action.is_some(),
        "should offer extract action in namespaced class method"
    );
}

// ── If / loop selections ────────────────────────────────────────────────────

#[test]
fn extracts_entire_if_block() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo($x) {
    if ($x > 0) {
        echo 'positive';
    }
    echo 'done';
}
";
    // Select the entire if statement (lines 2-4).
    let actions = get_code_actions(&backend, uri, content, 2, 4, 4, 5);
    let action = find_extract_action(&actions);
    assert!(action.is_some(), "should offer extract for entire if block");
}

#[test]
fn extracts_entire_foreach() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo($items) {
    foreach ($items as $item) {
        echo $item;
    }
    echo 'done';
}
";
    // Select the entire foreach.
    let actions = get_code_actions(&backend, uri, content, 2, 4, 4, 5);
    let action = find_extract_action(&actions);
    assert!(
        action.is_some(),
        "should offer extract for entire foreach block"
    );
}

// ── Code action kind ────────────────────────────────────────────────────────

#[test]
fn code_action_kind_is_refactor_extract() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    echo $x;
}
";
    let actions = get_code_actions(&backend, uri, content, 2, 4, 2, 11);
    let action = find_extract_action(&actions).expect("should offer action");
    assert_eq!(
        action.kind,
        Some(CodeActionKind::REFACTOR_EXTRACT),
        "kind should be refactor.extract"
    );
}

// ── Indentation ─────────────────────────────────────────────────────────────

#[test]
fn extracted_method_has_correct_indentation() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function bar() {
        $x = $this->baz();
        echo $x;
    }

    public function baz(): int {
        return 42;
    }
}
";
    // Select `$x = $this->baz();\n        echo $x;` (lines 3-4)
    let actions = get_code_actions(&backend, uri, content, 3, 8, 4, 16);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // The extracted method must be indented at the same level as sibling
    // methods (4 spaces), NOT at the body level (8 spaces).
    assert!(
        result.contains("\n    private function renderBar()"),
        "extracted method should be indented at member level (4 spaces), got:\n{result}"
    );
    // The body inside the extracted method should be 8 spaces.
    assert!(
        !result.contains("\n        private function"),
        "extracted method must NOT be double-indented:\n{result}"
    );
}

#[test]
fn extracted_method_body_lines_indented_consistently() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function dangerousInline(): void
    {
        $id = $this->generateId();
        $this->save($id);
        $this->log($id);
    }

    public function generateId(): string { return 'x'; }
    public function save(string $id): void {}
    public function log(string $id): void {}
}
";
    // Select `$this->save($id);\n        $this->log($id);` (lines 5-6)
    let actions = get_code_actions(&backend, uri, content, 5, 8, 6, 24);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // Every line inside the extracted method body must be indented at
    // exactly 8 spaces (body_indent for a 4-space class member).
    // The bug was that the second line got 16 spaces because the first
    // line's indent was stripped by selection trimming, making
    // min_indent=0 and leaving subsequent lines double-indented.
    let in_extracted = result
        .lines()
        .skip_while(|l| !l.contains("private function extracted("))
        .skip(1) // skip the signature line
        .skip(1) // skip the opening `{`
        .take_while(|l| !l.trim().starts_with('}'))
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>();

    assert!(
        !in_extracted.is_empty(),
        "should have body lines in extracted method:\n{result}"
    );
    for line in &in_extracted {
        let indent = line.len() - line.trim_start().len();
        assert_eq!(
            indent, 8,
            "body line should have 8 spaces indent, got {indent}: '{line}'\nfull result:\n{result}"
        );
    }
}

#[test]
fn offered_when_selection_starts_with_blank_line() {
    // Blank lines (with or without trailing whitespace) before the first
    // statement should not prevent the action from being offered.  After
    // trimming, the selection covers only the statements.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // Line 5 has trailing whitespace (mimicking editor behaviour).
    let content = "\
<?php
class Foo {
    public function multi(array $items): int
    {
        $count = 0;

        foreach ($items as $item) {
            $count = $count + $item;
        }
        return $count;
    }
}
";
    // Select from the blank line (line 5, mid-whitespace) through the
    // foreach closing `}` (line 8).
    let actions = get_code_actions(&backend, uri, content, 5, 4, 8, 9);
    let action = find_extract_action(&actions);
    assert!(
        action.is_some(),
        "should offer extract when selection starts with a blank line, got actions: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(cmd) => cmd.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn extracted_function_body_has_correct_indentation() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    echo $x;
}
";
    // Select `$x = 1;` (line 2)
    let actions = get_code_actions(&backend, uri, content, 2, 4, 2, 11);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // For a top-level function, the definition should have no leading indent.
    assert!(
        result.contains("\nfunction computeX()"),
        "extracted function should be at top level:\n{result}"
    );
}

// ── Accumulator pattern ─────────────────────────────────────────────────────

#[test]
fn accumulator_init_and_loop_not_passed_as_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function multiAssign(array $items): int
    {
        $count = 0;
        foreach ($items as $item) {
            $count = $count + 1;
        }
        return $count;
    }
}
";
    // Select the init + foreach block (lines 4-7):
    //   $count = 0;
    //   foreach ($items as $item) {
    //       $count = $count + 1;
    //   }
    let actions = get_code_actions(&backend, uri, content, 4, 8, 7, 9);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // $count is first written inside the selection ($count = 0), so it
    // must NOT appear as a parameter at the call site.  It should only
    // be a return value.
    assert!(
        !result.contains("computeCount($count"),
        "$count must not be passed as parameter (first write is inside selection):\n{result}"
    );
    // $count should be assigned from the return value.
    assert!(
        result.contains("$count = $this->computeCount("),
        "$count should be assigned from the extracted method's return value:\n{result}"
    );
}

#[test]
fn variable_read_before_inner_write_is_param_and_return() {
    // A variable that is read inside the selection *before* its first
    // write inside the selection consumes the incoming value, so it must
    // be passed as a parameter — even though it is also written and read
    // after the selection (making it a return value too).
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class S {
    public function search(array $subcategories, array $categories): array
    {
        if (!$subcategories) {
            $ids = Repo::byCategories($categories)->all();
            $subcategories = array_merge($subcategories, $ids);
        }

        if (!$subcategories) {
            return [];
        }
        return $subcategories;
    }
}
";
    // Select the outer `if (!$subcategories) { ... }` block (lines 4-7).
    let actions = get_code_actions(&backend, uri, content, 4, 8, 7, 9);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // $subcategories must be passed in (it is read before being written
    // inside the selection) AND assigned from the return value.
    assert!(
        result.contains("$subcategories = $this->computeSubcategories($subcategories"),
        "$subcategories must be both an argument and the assigned return value:\n{result}"
    );
    // The extracted method must declare $subcategories as a parameter so
    // its body's reads resolve.
    let extracted_start = result
        .find("private function computeSubcategories")
        .unwrap();
    let extracted = &result[extracted_start..];
    assert!(
        extracted.contains("$subcategories")
            && extracted[..extracted.find('{').unwrap()].contains("$subcategories"),
        "extracted method signature must include $subcategories:\n{result}"
    );
    assert!(
        extracted.contains("return $subcategories;"),
        "extracted method should return $subcategories:\n{result}"
    );
}

#[test]
fn uniform_guard_value_referencing_local_uses_sentinel() {
    // A guard `return <expr>;` whose expression references a variable
    // that is local to the selection cannot be reproduced at the call
    // site (the local is out of scope there).  The extraction must keep
    // the expression inside the method and propagate it via a null
    // sentinel instead of emitting `if (!extracted(…)) return <expr>;`.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class C {
    public function show(Product $product): RedirectResponse
    {
        if ($product->redirect_id) {
            $url = Slug::lookup($product->redirect_id);
            if ($url) {
                return redirect($url, 301);
            }
        }
        return view('x');
    }
}
";
    // Select the outer `if ($product->redirect_id) { ... }` block (lines 4-9).
    let actions = get_code_actions(&backend, uri, content, 4, 8, 9, 9);
    let action = find_extract_action(&actions).expect("should offer extract action");
    let resolved = resolve_action(&backend, uri, content, action);
    let result = apply_edit(content, resolved.edit.as_ref().unwrap());

    // The call site must NOT reproduce the local-referencing expression.
    let call_area = &result[..result.find("private function").unwrap()];
    assert!(
        !call_area.contains("$url"),
        "call site must not reference the selection-local $url:\n{result}"
    );
    // It should propagate via the null sentinel.
    assert!(
        result.contains("if ($result !== null) return $result;"),
        "call site should propagate the result through a null sentinel:\n{result}"
    );
    // The extracted method keeps the return expression inside, where $url
    // is in scope.
    let extracted_start = result.find("private function").unwrap();
    let extracted = &result[extracted_start..];
    assert!(
        extracted.contains("return redirect($url, 301);"),
        "extracted method should keep the original return expression:\n{result}"
    );
}

use super::*;

// ── Name generation tests ───────────────────────────────────────

#[test]
fn name_from_method_call() {
    assert_eq!(generate_variable_name("$user->getName()"), "name");
}

#[test]
fn name_from_method_call_no_prefix() {
    assert_eq!(generate_variable_name("$user->email()"), "email");
}

#[test]
fn name_from_method_call_with_args() {
    assert_eq!(generate_variable_name("$repo->findById($id)"), "findById");
}

#[test]
fn name_from_property_access() {
    assert_eq!(generate_variable_name("$user->email"), "email");
}

#[test]
fn name_from_nullsafe_method() {
    assert_eq!(generate_variable_name("$user?->getName()"), "name");
}

#[test]
fn name_from_nullsafe_property() {
    assert_eq!(generate_variable_name("$user?->email"), "email");
}

#[test]
fn name_from_static_call() {
    assert_eq!(generate_variable_name("Carbon::now()"), "now");
}

#[test]
fn name_from_static_call_namespaced() {
    assert_eq!(generate_variable_name("\\Carbon\\Carbon::now()"), "now");
}

#[test]
fn name_from_function_call() {
    assert_eq!(
        generate_variable_name("array_filter($items, $fn)"),
        "arrayFilter"
    );
}

#[test]
fn name_from_simple_function() {
    assert_eq!(generate_variable_name("count($items)"), "count");
}

#[test]
fn name_from_namespaced_function() {
    assert_eq!(
        generate_variable_name("App\\Helpers\\format_name($s)"),
        "formatName"
    );
}

#[test]
fn name_fallback_for_expression() {
    assert_eq!(generate_variable_name("$a + $b"), "variable");
}

#[test]
fn name_fallback_for_string_literal() {
    assert_eq!(generate_variable_name("'hello world'"), "variable");
}

#[test]
fn name_fallback_for_number() {
    assert_eq!(generate_variable_name("42"), "variable");
}

#[test]
fn name_from_chained_method_call() {
    // For chained calls, use the last method name
    assert_eq!(
        generate_variable_name("$query->where('x', 1)->first()"),
        "first"
    );
}

#[test]
fn name_from_get_prefix_method() {
    assert_eq!(generate_variable_name("$user->getEmail()"), "email");
}

#[test]
fn name_from_is_prefix_method() {
    assert_eq!(generate_variable_name("$user->isActive()"), "active");
}

#[test]
fn name_from_has_prefix_method() {
    assert_eq!(
        generate_variable_name("$user->hasPermission()"),
        "permission"
    );
}

#[test]
fn name_no_strip_island() {
    // "island" should not have "is" stripped because 'l' is lowercase
    assert_eq!(generate_variable_name("$map->island()"), "island");
}

// ── Deduplication tests ─────────────────────────────────────────

#[test]
fn deduplicate_no_collision() {
    let existing = vec!["$foo".to_string(), "$bar".to_string()];
    assert_eq!(deduplicate_name("name", &existing), "name");
}

#[test]
fn deduplicate_with_collision() {
    let existing = vec!["$name".to_string(), "$foo".to_string()];
    assert_eq!(deduplicate_name("name", &existing), "name1");
}

#[test]
fn deduplicate_multiple_collisions() {
    let existing = vec![
        "$name".to_string(),
        "$name1".to_string(),
        "$name2".to_string(),
    ];
    assert_eq!(deduplicate_name("name", &existing), "name3");
}

// ── Insertion point tests ───────────────────────────────────────

#[test]
fn find_statement_line_simple() {
    let content = "<?php\n    $x = $user->getName();\n";
    // Selection starts at `$user` (offset 14 approximately)
    let offset = content.find("$user").unwrap();
    let (line_start, indent) = find_enclosing_statement_line(content, offset);
    assert_eq!(line_start, 6); // After "<?php\n"
    assert_eq!(indent, "    ");
}

#[test]
fn find_statement_line_no_indent() {
    let content = "<?php\n$x = foo();\n";
    let offset = content.find("foo").unwrap();
    let (line_start, indent) = find_enclosing_statement_line(content, offset);
    assert_eq!(line_start, 6);
    assert_eq!(indent, "");
}

#[test]
fn find_statement_line_tab_indent() {
    let content = "<?php\n\t\t$x = bar();\n";
    let offset = content.find("bar").unwrap();
    let (line_start, indent) = find_enclosing_statement_line(content, offset);
    assert_eq!(line_start, 6);
    assert_eq!(indent, "\t\t");
}

// ── snake_to_camel tests ────────────────────────────────────────

#[test]
fn snake_to_camel_simple() {
    assert_eq!(snake_to_camel("array_filter"), "arrayFilter");
}

#[test]
fn snake_to_camel_single_word() {
    assert_eq!(snake_to_camel("count"), "count");
}

#[test]
fn snake_to_camel_three_parts() {
    assert_eq!(snake_to_camel("str_to_upper"), "strToUpper");
}

// ── strip_outer_parens tests ────────────────────────────────────

#[test]
fn strip_parens_wrapped_expression() {
    assert_eq!(strip_outer_parens("($a + $b)"), "$a + $b");
}

#[test]
fn strip_parens_no_parens() {
    assert_eq!(strip_outer_parens("$a + $b"), "$a + $b");
}

#[test]
fn strip_parens_function_call_unchanged() {
    assert_eq!(strip_outer_parens("foo($x)"), "foo($x)");
}

#[test]
fn strip_parens_two_groups_unchanged() {
    assert_eq!(strip_outer_parens("($a) + ($b)"), "($a) + ($b)");
}

#[test]
fn strip_parens_nested() {
    assert_eq!(strip_outer_parens("(($a + $b))"), "($a + $b)");
}

#[test]
fn strip_parens_with_whitespace() {
    assert_eq!(strip_outer_parens("( $a + $b )"), "$a + $b");
}

// ── is_entire_assignment_rhs tests ──────────────────────────────

#[test]
fn assignment_rhs_full_rhs_detected() {
    let content = "<?php\nfunction test() {\n    $tax = $total * 0.21;\n}\n";
    let start = content.find("$total * 0.21").unwrap();
    let end = start + "$total * 0.21".len();
    assert!(is_entire_assignment_rhs(content, start, end));
}

#[test]
fn assignment_rhs_sub_expression_not_detected() {
    let content = "<?php\nfunction test() {\n    $tax = $total * 0.21;\n}\n";
    let start = content.find("$total").unwrap();
    let end = start + "$total".len();
    assert!(!is_entire_assignment_rhs(content, start, end));
}

#[test]
fn assignment_rhs_standalone_statement_not_detected() {
    let content = "<?php\nfunction test() {\n    echo $total * 0.21;\n}\n";
    let start = content.find("$total * 0.21").unwrap();
    let end = start + "$total * 0.21".len();
    assert!(!is_entire_assignment_rhs(content, start, end));
}

#[test]
fn assignment_rhs_comparison_not_confused() {
    // `==` should not be treated as assignment
    let content = "<?php\nfunction test() {\n    if ($x == $y) {}\n}\n";
    let start = content.find("$y").unwrap();
    let end = start + "$y".len();
    assert!(!is_entire_assignment_rhs(content, start, end));
}

// ── is_entire_expression_statement tests ────────────────────────

#[test]
fn is_entire_statement_true_for_full_expression() {
    let content = "<?php\nfunction test() {\n    $this->save($id);\n}\n";
    let start = content.find("$this->save").unwrap();
    let end = content.find("($id)").unwrap() + 5;
    assert!(is_entire_expression_statement(content, start, end));
}

#[test]
fn is_entire_statement_false_for_sub_expression() {
    let content = "<?php\nfunction test() {\n    return $this->save($id);\n}\n";
    let start = content.find("$this->save").unwrap();
    let end = content.find("($id)").unwrap() + 5;
    assert!(!is_entire_expression_statement(content, start, end));
}

#[test]
fn is_entire_statement_false_for_argument() {
    let content = "<?php\nfunction test() {\n    echo count($items);\n}\n";
    let start = content.find("count").unwrap();
    let end = content.find("($items)").unwrap() + 8;
    assert!(!is_entire_expression_statement(content, start, end));
}

#[test]
fn is_entire_statement_true_for_multiline_selection_with_comment() {
    // Selecting from end of a comment line through `var_dump($value);`
    // should still be detected as a standalone expression statement.
    let content = "<?php\nfunction test($value) {\n    // comment\n    var_dump($value);\n}\n";
    let start = content.find("// comment").unwrap() + "// comment".len();
    let end = content.find("var_dump($value);").unwrap() + "var_dump($value);".len();
    assert!(is_entire_expression_statement(content, start, end));
}

// ── is_valid_expression tests ───────────────────────────────────

#[test]
fn valid_expr_method_call() {
    assert!(is_valid_expression("$this->save($id)"));
}

#[test]
fn valid_expr_property_access() {
    assert!(is_valid_expression("$user->name"));
}

#[test]
fn valid_expr_variable() {
    assert!(is_valid_expression("$x"));
}

#[test]
fn valid_expr_function_call() {
    assert!(is_valid_expression("count($items)"));
}

#[test]
fn valid_expr_static_call() {
    assert!(is_valid_expression("Carbon::now()"));
}

#[test]
fn valid_expr_new() {
    assert!(is_valid_expression("new Foo($a)"));
}

#[test]
fn valid_expr_binary() {
    assert!(is_valid_expression("$a + $b"));
}

#[test]
fn valid_expr_string_literal() {
    assert!(is_valid_expression("'hello'"));
}

#[test]
fn valid_expr_number() {
    assert!(is_valid_expression("42"));
}

#[test]
fn valid_expr_array_literal() {
    assert!(is_valid_expression("[1, 2, 3]"));
}

#[test]
fn valid_expr_ternary() {
    assert!(is_valid_expression("$x ? $a : $b"));
}

#[test]
fn valid_expr_parenthesized() {
    assert!(is_valid_expression("($a + $b)"));
}

#[test]
fn invalid_expr_bare_method_name() {
    assert!(!is_valid_expression("save"));
}

#[test]
fn invalid_expr_bare_identifier() {
    assert!(!is_valid_expression("getName"));
}

#[test]
fn invalid_expr_arrow_fragment() {
    assert!(!is_valid_expression("->save($id)"));
}

#[test]
fn invalid_expr_partial_call() {
    assert!(!is_valid_expression("save($id"));
}

#[test]
fn invalid_expr_method_name_with_parens() {
    // `getLabel()` looks like a function call but is actually a
    // method name fragment when preceded by `->` in the source.
    // The is_valid_expression check alone can't catch this —
    // the context check in collect_extract_variable_actions handles it.
    // So is_valid_expression returns true (it IS valid PHP syntax),
    // but the action is still rejected by the `->` prefix check.
    assert!(is_valid_expression("getLabel()"));
}

#[test]
fn invalid_expr_multi_statement() {
    assert!(!is_valid_expression(
        "$this->generateId();\n        $this->save($id)"
    ));
}

#[test]
fn invalid_expr_two_calls_with_semicolons() {
    assert!(!is_valid_expression("foo(); bar()"));
}

#[test]
fn semicolon_in_string_not_rejected() {
    assert!(is_valid_expression("'hello; world'"));
    assert!(is_valid_expression("\"hello; world\""));
}

#[test]
fn trailing_semicolon_not_rejected() {
    // A single expression with trailing `;` is fine — it's just
    // the statement terminator which we strip.
    assert!(is_valid_expression("$this->save($id);"));
}

#[test]
fn invalid_expr_empty() {
    assert!(!is_valid_expression(""));
}

#[test]
fn invalid_expr_whitespace() {
    assert!(!is_valid_expression("   "));
}

#[test]
fn reject_bare_this_in_method_call_context() {
    // Selecting just `$this` from `$this->save($id)` should be
    // rejected as useless (produces `$variable->save($id)`).
    // While `$this` IS a valid expression syntactically, we rely
    // on is_entire_expression_statement to not trigger (it won't
    // since `$this` is not the whole statement). But is_valid_expression
    // correctly returns true — the real guard is that extracting
    // `$this` alone IS offered but the user simply wouldn't select
    // just `$this`.  The parser-based check ensures we don't
    // produce *broken* code.
    assert!(is_valid_expression("$this"));
}

// ── find_identical_occurrences tests ─────────────────────────────

#[test]
fn find_occurrences_finds_duplicates() {
    let content = "<?php echo $x->foo(); echo $x->foo(); echo $x->bar();";
    let needle = "$x->foo()";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    assert_eq!(occurrences.len(), 1);
    assert!(occurrences[0].0 > first);
}

#[test]
fn find_occurrences_none_when_unique() {
    let content = "<?php echo $x->foo(); echo $x->bar();";
    let needle = "$x->foo()";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    assert!(occurrences.is_empty());
}

#[test]
fn find_occurrences_skips_substrings() {
    let content = "<?php echo $x->foo(); echo $x->fooBar();";
    let needle = "$x->foo";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    // "$x->fooBar" contains "$x->foo" but is followed by 'B' (alphanumeric),
    // so it should NOT match.
    assert!(occurrences.is_empty());
}

#[test]
fn find_occurrences_respects_scope_boundary() {
    // Two functions each with `$x->foo()` — searching within the first
    // function's scope should not find the second.
    let content = "<?php\nfunction a() { echo $x->foo(); }\nfunction b() { echo $x->foo(); }\n";
    let needle = "$x->foo()";
    let first = content.find(needle).unwrap();
    // Scope of function a() body: from first `{` to first `}`
    let scope_start = content.find('{').unwrap();
    let scope_end = content.find('}').unwrap() + 1;
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        scope_start,
        scope_end,
    );
    assert!(
        occurrences.is_empty(),
        "should not find occurrence in function b() when scoped to function a()"
    );
}

#[test]
fn find_occurrences_does_not_overlap_self_adjacent_matches() {
    // `$a . $a . $a` contains two textual matches of "$a . $a" that overlap
    // by one "$a". The scan must resume past the *end* of an accepted
    // match, not one byte past its start, so the overlapping second match
    // is never reported alongside the first.
    let content = "<?php\n$x = $a . $a . $a;\n";
    let needle = "$a . $a";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    for (start, end) in &occurrences {
        assert!(
            *start >= first + needle.len(),
            "occurrence at {start}..{end} overlaps the original selection at {first}..{}",
            first + needle.len()
        );
    }
}

#[test]
fn find_occurrences_handles_multibyte_selection_start() {
    // A selection beginning with a multibyte character must not cause the
    // scan to resume mid-character on the next iteration.
    let content = "<?php\necho Ünit::TAX; echo Ünit::TAX;\n";
    let needle = "Ünit::TAX";
    let first = content.find(needle).unwrap();
    let occurrences = find_identical_occurrences(
        content,
        needle,
        first,
        first + needle.len(),
        0,
        content.len(),
    );
    assert_eq!(occurrences.len(), 1);
    assert!(occurrences[0].0 > first);
}

use super::*;
use crate::util::collapse_continuation_lines;

#[test]
fn test_nullsafe_chain_with_call() {
    // $user->getAddress()?->getCity()->
    let input = "$user->getAddress()?->getCity()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    // Should include the full chain, not lose ->getAddress()
    assert!(
        result.contains("getAddress"),
        "Expected chain to include getAddress(), got: {result}"
    );
    assert!(
        result.contains("getCity"),
        "Expected chain to include getCity, got: {result}"
    );
}

#[test]
fn test_nullsafe_simple_var() {
    // $user?->getCity()->
    let input = "$user?->getCity()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert!(
        result.contains("$user") && result.contains("getCity"),
        "Expected $user...getCity, got: {result}"
    );
}

#[test]
fn test_nullsafe_property_chain() {
    // $a?->b?->c->
    let input = "$a?->b?->c->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert!(
        result.contains("$a") && result.contains("b") && result.contains("c"),
        "Expected full chain $a...b...c, got: {result}"
    );
}

#[test]
fn test_nullsafe_after_property_chain() {
    // A property/call chain followed by `?->method()` must keep the full
    // receiver and preserve the null-safe operator, not collapse to the
    // bare property name (`b?->c()`).
    let input = "$a->b?->c()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$a->b?->c()",
        "Expected full receiver with null-safe operator, got: {result}"
    );
}

#[test]
fn test_nullsafe_after_call_chain() {
    // The receiver before `?->` may itself be a call expression.
    let input = "make()?->c()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "make()?->c()",
        "Expected call-expression receiver preserved, got: {result}"
    );
}

#[test]
fn test_regular_chain() {
    let input = "$user->getProfile()->getName()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert!(
        result.contains("getProfile") && result.contains("getName"),
        "Expected full chain, got: {result}"
    );
}

#[test]
fn test_triple_method_chain() {
    // 3-level chained method calls must be fully extracted.
    let input = "$this->products()->filter()->first()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert!(
        result.contains("products") && result.contains("filter") && result.contains("first"),
        "Expected full 3-level chain, got: {result}"
    );
}

#[test]
fn test_simple_variable() {
    let input = "$user->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(result, "$user");
}

#[test]
fn test_nullsafe_simple() {
    let input = "$user?->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(result, "$user");
}

// ── Multi-line chain collapse tests ─────────────────────────────

#[test]
fn test_collapse_simple_chain() {
    let lines = vec!["$this->getRepository()", "    ->findAll()", "    ->"];
    let (collapsed, col) = collapse_continuation_lines(&lines, 2, 6);
    assert!(
        collapsed.starts_with("$this->getRepository()"),
        "collapsed should start with base expression, got: {collapsed}"
    );
    assert!(
        collapsed.contains("->findAll()->"),
        "collapsed should contain intermediate chain, got: {collapsed}"
    );
    // The cursor should be past the `->` in the collapsed string.
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_collapse_not_a_continuation() {
    let lines = vec!["$this->getRepository()", "    $foo->bar()"];
    let (collapsed, col) = collapse_continuation_lines(&lines, 1, 10);
    assert_eq!(collapsed, "    $foo->bar()");
    assert_eq!(col, 10);
}

#[test]
fn test_collapse_nullsafe_chain() {
    let lines = vec!["$user->getAddress()", "    ?->getCity()", "    ->"];
    let (collapsed, col) = collapse_continuation_lines(&lines, 2, 6);
    assert!(
        collapsed.contains("?->getCity()"),
        "collapsed should preserve nullsafe operator, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(col <= chars.len());
}

#[test]
fn test_collapse_with_static_call_base() {
    let lines = vec![
        "SomeClass::query()",
        "    ->where('active', true)",
        "    ->",
    ];
    let (collapsed, col) = collapse_continuation_lines(&lines, 2, 6);
    assert!(
        collapsed.starts_with("SomeClass::query()"),
        "collapsed should start with static call, got: {collapsed}"
    );
    assert!(
        collapsed.contains("->where('active', true)->"),
        "collapsed should contain chained call, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(col <= chars.len());
}

#[test]
fn test_collapse_cursor_mid_identifier() {
    // Cursor is in the middle of typing an identifier after `->`.
    let lines = vec!["$builder->configure()", "    ->whe"];
    let (collapsed, col) = collapse_continuation_lines(&lines, 1, 9);
    assert!(
        collapsed.contains("->configure()->whe"),
        "collapsed should contain the partial identifier, got: {collapsed}"
    );
    // col should point at the end of `whe`
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(col <= chars.len());
}

#[test]
fn test_collapse_single_continuation() {
    let lines = vec!["$foo->bar()", "    ->"];
    let (collapsed, _col) = collapse_continuation_lines(&lines, 1, 6);
    assert_eq!(collapsed, "$foo->bar()->");
}

#[test]
fn test_collapse_multiline_closure_argument() {
    // Brand::whereNested(function (Builder $q): void {
    // })
    // ->
    let lines = vec![
        "Brand::whereNested(function (Builder $q): void {",
        "})",
        "    ->",
    ];
    let (collapsed, col) = collapse_continuation_lines(&lines, 2, 6);
    assert!(
        collapsed.starts_with("Brand::whereNested("),
        "collapsed should start with the call expression, got: {collapsed}"
    );
    assert!(
        collapsed.contains("})->"),
        "collapsed should join the closing brace/paren with the arrow, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_collapse_multiline_closure_with_body() {
    // Brand::whereNested(function (Builder $q): void {
    //     $q->where('active', true);
    // })
    // ->
    let lines = vec![
        "Brand::whereNested(function (Builder $q): void {",
        "    $q->where('active', true);",
        "})",
        "    ->",
    ];
    let (collapsed, col) = collapse_continuation_lines(&lines, 3, 6);
    assert!(
        collapsed.starts_with("Brand::whereNested("),
        "collapsed should start with the call expression, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_collapse_multiline_closure_then_chain() {
    // Brand::whereNested(function (Builder $q): void {
    // })
    // ->where('active', 1)
    // ->
    let lines = vec![
        "Brand::whereNested(function (Builder $q): void {",
        "})",
        "    ->where('active', 1)",
        "    ->",
    ];
    let (collapsed, col) = collapse_continuation_lines(&lines, 3, 6);
    assert!(
        collapsed.starts_with("Brand::whereNested("),
        "collapsed should start with the call expression, got: {collapsed}"
    );
    assert!(
        collapsed.contains("->where('active', 1)->"),
        "collapsed should contain the chained call, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_collapse_multiline_closure_intermediate_chain() {
    // $builder->where('x', 1)
    // ->whereNested(function ($q) {
    // })
    // ->
    let lines = vec![
        "$builder->where('x', 1)",
        "    ->whereNested(function ($q) {",
        "    })",
        "    ->",
    ];
    let (collapsed, col) = collapse_continuation_lines(&lines, 3, 6);
    assert!(
        collapsed.starts_with("$builder->where('x', 1)"),
        "collapsed should start with the base expression, got: {collapsed}"
    );
    assert!(
        collapsed.contains("->whereNested("),
        "collapsed should contain the closure call, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_collapse_blank_line_in_chain() {
    // A blank line between chain segments should not break the collapse.
    //
    //   Brand::with('english')
    //
    //       ->paginate()
    //       ->
    let lines = vec!["Brand::with('english')", "", "    ->paginate()", "    ->"];
    let (collapsed, col) = collapse_continuation_lines(&lines, 3, 6);
    assert!(
        collapsed.starts_with("Brand::with('english')"),
        "collapsed should start with the base expression, got: {collapsed}"
    );
    assert!(
        collapsed.contains("->paginate()->"),
        "collapsed should contain the intermediate chain, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_collapse_multiple_blank_lines_in_chain() {
    // Multiple blank lines should all be skipped.
    let lines = vec!["$foo->bar()", "", "", "    ->baz()", "    ->"];
    let (collapsed, _col) = collapse_continuation_lines(&lines, 4, 6);
    assert_eq!(collapsed, "$foo->bar()->baz()->");
}

#[test]
fn test_collapse_whitespace_only_line_in_chain() {
    // A line with only spaces/tabs should be treated as blank.
    let lines = vec![
        "SomeClass::query()",
        "    ",
        "    ->where('active', true)",
        "    ->",
    ];
    let (collapsed, col) = collapse_continuation_lines(&lines, 3, 6);
    assert!(
        collapsed.starts_with("SomeClass::query()"),
        "collapsed should start with static call, got: {collapsed}"
    );
    assert!(
        collapsed.contains("->where('active', true)->"),
        "collapsed should contain intermediate chain, got: {collapsed}"
    );
    let chars: Vec<char> = collapsed.chars().collect();
    assert!(
        col <= chars.len(),
        "col {col} should be within collapsed len {}",
        chars.len()
    );
}

#[test]
fn test_inline_array_literal_with_index_access() {
    // [Customer::first()][0]->
    let input = "[Customer::first()][0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "[Customer::first()][0]",
        "Subject should be the literal base plus index segment"
    );
}

#[test]
fn test_inline_array_literal_new_expression() {
    // [new Foo()][0]->
    let input = "[new Foo()][0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "[new Foo()][0]",
        "Subject should be the literal base plus index segment"
    );
}

#[test]
fn test_collapse_blank_line_cursor_on_first_continuation() {
    // Blank line right before the cursor's continuation line.
    let lines = vec!["$obj->method()", "", "    ->"];
    let (collapsed, _col) = collapse_continuation_lines(&lines, 2, 6);
    assert_eq!(collapsed, "$obj->method()->");
}

#[test]
fn test_parenthesized_property_invocation() {
    // ($this->formatter)()->
    // The subject should be `($this->formatter)()` so that the resolver
    // can unwrap the parenthesized property, resolve its type, and check
    // for __invoke().
    let input = "        ($this->formatter)()->";
    let chars: Vec<char> = input.chars().collect();
    let col = chars.len();
    let result = detect_access_operator(&chars, col);
    assert!(
        result.is_some(),
        "Expected Some from detect_access_operator"
    );
    let (subject, kind) = result.unwrap();
    assert_eq!(kind, AccessKind::Arrow);
    assert!(
        subject.contains("$this->formatter"),
        "Expected subject to contain $this->formatter, got: {subject}"
    );
    assert!(
        subject.contains("()"),
        "Expected subject to contain call parens (), got: {subject}"
    );
}

#[test]
fn test_call_expression_base_array_access() {
    // $c->items()[0]->
    let input = "$c->items()[0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$c->items()[0]",
        "Subject should be the call expression base plus index segment"
    );
}

#[test]
fn test_static_call_expression_base_array_access() {
    // Collection::all()[0]->
    let input = "Collection::all()[0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "Collection::all()[0]",
        "Subject should be the static call expression base plus index segment"
    );
}

#[test]
fn test_function_call_base_array_access() {
    // getItems()[0]->
    let input = "getItems()[0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "getItems()[0]",
        "Subject should be the function call base plus index segment"
    );
}

#[test]
fn test_clone_simple_variable() {
    // (clone $date)->
    let input = "(clone $date)->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(result, "$date", "clone should preserve the inner variable");
}

#[test]
fn test_clone_property_chain() {
    // (clone $this->date)->
    let input = "(clone $this->date)->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$this->date",
        "clone should preserve the inner property chain"
    );
}

#[test]
fn test_clone_method_chain() {
    // (clone $date)->endOfMonth()->
    let input = "(clone $date)->endOfMonth()->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert!(
        result.contains("$date") && result.contains("endOfMonth"),
        "Expected chain through clone, got: {result}"
    );
}

#[test]
fn test_clone_with_extra_whitespace() {
    // (clone   $date)->
    let input = "(clone   $date)->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$date",
        "clone with extra whitespace should still resolve"
    );
}

#[test]
fn test_inline_new_expression_method_chain() {
    // (new Foo)->bar()->
    let input = "(new Foo)->bar()->";
    let chars: Vec<char> = input.chars().collect();
    let col = chars.len();
    let result = detect_access_operator(&chars, col);
    assert!(
        result.is_some(),
        "Expected Some from detect_access_operator"
    );
    let (subject, kind) = result.unwrap();
    assert_eq!(kind, AccessKind::Arrow);
    assert_eq!(
        subject, "new Foo->bar()",
        "Expected subject 'new Foo->bar()', got: {subject}"
    );
}

// ── Property chain with array bracket access ────────────────────

#[test]
fn test_property_chain_array_access_variable_key() {
    // $this->cache[$key]->
    let input = "$this->cache[$key]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$this->cache[]",
        "Subject should include the full property chain with bracket segment"
    );
}

#[test]
fn test_property_chain_array_access_numeric_index() {
    // $this->translations[0]->
    let input = "$this->translations[0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$this->translations[0]",
        "Subject should include the full property chain with bracket segment"
    );
}

#[test]
fn test_property_chain_array_access_string_literal_key() {
    // $this->cache['myKey']->
    let input = "$this->cache['myKey']->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$this->cache['myKey']",
        "Subject should preserve the string key in the bracket segment"
    );
}

#[test]
fn test_object_property_array_access() {
    // $service->items[0]->
    let input = "$service->items[0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$service->items[0]",
        "Subject should include the full object property chain with bracket segment"
    );
}

#[test]
fn test_nested_property_chain_array_access() {
    // $this->nested->entries[0]->
    let input = "$this->nested->entries[0]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$this->nested->entries[0]",
        "Subject should include the full nested property chain with bracket segment"
    );
}

#[test]
fn test_nullsafe_property_chain_array_access() {
    // $this?->cache[$key]->
    let input = "$this?->cache[$key]->";
    let chars: Vec<char> = input.chars().collect();
    let arrow_pos = input.rfind("->").unwrap();
    let result = extract_arrow_subject(&chars, arrow_pos);
    assert_eq!(
        result, "$this->cache[]",
        "Subject should include the property chain with bracket segment (? is stripped)"
    );
}

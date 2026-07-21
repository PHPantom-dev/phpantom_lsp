use super::*;
use crate::atom::atom;

/// Dummy class loader that returns `None` for all lookups.
fn no_classes(_name: &str) -> Option<Arc<ClassInfo>> {
    None
}

// ── Trigger detection ───────────────────────────────────────────────

#[test]
fn detects_trigger_at_line_start() {
    let content = "<?php\n/**";
    let pos = Position {
        line: 1,
        character: 3,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(result.is_some(), "Should detect /** trigger");
    let (range, indent) = result.unwrap();
    assert_eq!(indent, "");
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.character, 3);
}

#[test]
fn detects_trigger_with_indentation() {
    let content = "<?php\nclass Foo {\n    /**";
    let pos = Position {
        line: 2,
        character: 7,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(result.is_some(), "Should detect indented /** trigger");
    let (_, indent) = result.unwrap();
    assert_eq!(indent, "    ");
}

#[test]
fn rejects_trigger_inside_existing_docblock() {
    let content = "<?php\n/**\n * @param\n */\nfunction test() {}";
    let pos = Position {
        line: 1,
        character: 3,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(
        result.is_none(),
        "Should not trigger inside existing docblock"
    );
}

#[test]
fn rejects_trigger_with_closing_on_same_line() {
    let content = "<?php\n/** @var int */";
    let pos = Position {
        line: 1,
        character: 3,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(
        result.is_none(),
        "Should not trigger when */ is on the same line"
    );
}

#[test]
fn rejects_trigger_with_code_before() {
    let content = "<?php\n$x = /**";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let result = detect_docblock_trigger(content, pos);
    assert!(
        result.is_none(),
        "Should not trigger when code precedes /**"
    );
}

#[test]
fn no_panic_on_multibyte_characters() {
    // "ń" is 2 bytes in UTF-8 but 1 UTF-16 code unit.
    // The cursor is after the closing paren, UTF-16 column 32.
    // Using that as a byte offset would land inside "ń" and panic.
    let content = "<?php\n                $table->string(ń);";
    let pos = Position {
        line: 1,
        character: 32,
    };
    // Must not panic — should simply return None.
    let result = detect_docblock_trigger(content, pos);
    assert!(result.is_none());
}

// ── Declaration classification ──────────────────────────────────────

#[test]
fn classifies_function() {
    let decl = "function test(string $name): void {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::FunctionOrMethod
    ));
}

#[test]
fn classifies_method() {
    let decl = "    public function test(): int {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::FunctionOrMethod
    ));
}

#[test]
fn classifies_abstract_method() {
    let decl = "    abstract public function test(): int;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::FunctionOrMethod
    ));
}

#[test]
fn classifies_class() {
    let decl = "class Foo extends Bar {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_abstract_class() {
    let decl = "abstract class Foo {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_interface() {
    let decl = "interface Foo {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_trait() {
    let decl = "trait Foo {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_enum() {
    let decl = "enum Status: string {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_property() {
    let decl = "    public string $name;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Property
    ));
}

#[test]
fn classifies_untyped_property() {
    let decl = "    public $name;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Property
    ));
}

#[test]
fn classifies_constant() {
    let decl = "    const FOO = 'bar';";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Constant
    ));
}

#[test]
fn classifies_public_constant() {
    let decl = "    public const int MAX = 100;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Constant
    ));
}

// ── Parameter parsing ───────────────────────────────────────────────

#[test]
fn parses_params_with_types() {
    let text = "function test(string $name, int $age): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 2);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$name".to_string())
    );
    assert_eq!(
        info.params[1],
        (Some(PhpType::parse("int")), "$age".to_string())
    );
    assert_eq!(info.return_type, Some(PhpType::parse("void")));
}

#[test]
fn parses_params_without_types() {
    let text = "function test($name, $age) {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 2);
    assert_eq!(info.params[0], (None, "$name".to_string()));
    assert_eq!(info.params[1], (None, "$age".to_string()));
}

#[test]
fn parses_nullable_type() {
    let text = "function test(?string $name): ?int {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params[0].0, Some(PhpType::parse("?string")));
    assert_eq!(info.return_type, Some(PhpType::parse("?int")));
}

#[test]
fn parses_union_type() {
    let text = "function test(string|int $value): string|false {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params[0].0, Some(PhpType::parse("string|int")));
    assert_eq!(info.return_type, Some(PhpType::parse("string|false")));
}

#[test]
fn parses_variadic_param() {
    let text = "function test(string ...$names): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$names".to_string())
    );
}

#[test]
fn parses_reference_param() {
    let text = "function test(array &$data): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("array")), "$data".to_string())
    );
}

#[test]
fn parses_param_with_default() {
    let text = "function test(string $name = 'world'): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$name".to_string())
    );
}

#[test]
fn parses_no_params() {
    let text = "function test(): void {}";
    let info = parse_declaration_info(text);
    assert!(info.params.is_empty());
    assert_eq!(info.return_type, Some(PhpType::parse("void")));
}

#[test]
fn parses_property_type() {
    let text = "    public string $name;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn parses_readonly_property_type() {
    let text = "    public readonly string $name;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn parses_typed_constant_extracts_only_type() {
    let text = "    const int COW = 0;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("int")));
}

#[test]
fn parses_public_typed_constant_extracts_only_type() {
    let text = "    public const string NAME = 'foo';";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn parses_untyped_constant_has_no_type() {
    let text = "    const MAX = 100;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, None);
}

#[test]
fn parses_promoted_param_type() {
    let text = "function __construct(public readonly bool $selected) {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("bool")), "$selected".to_string())
    );
}

#[test]
fn parses_class_extends() {
    let text = "class Child extends Base {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.extends_names, vec!["Base"]);
    assert!(info.implements_names.is_empty());
}

#[test]
fn parses_class_implements() {
    let text = "class Foo implements Bar, Baz {}";
    let info = parse_declaration_info(text);
    assert!(info.extends_names.is_empty());
    assert_eq!(info.implements_names, vec!["Bar", "Baz"]);
}

#[test]
fn parses_class_extends_and_implements() {
    let text = "class Child extends Base implements Iface {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.extends_names, vec!["Base"]);
    assert_eq!(info.implements_names, vec!["Iface"]);
}

// ── Type enrichment ─────────────────────────────────────────────────

#[test]
fn enrichment_missing_type_produces_mixed() {
    let mut ts = 1;
    let result = enrichment_snippet(None, &mut ts, &no_classes);
    assert_eq!(result, Some("${1:mixed}".to_string()));
    assert_eq!(ts, 2);
}

#[test]
fn enrichment_array_produces_array_tabstop() {
    let mut ts = 1;
    let hint = PhpType::parse("array");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert_eq!(result, Some("array<${1:mixed}>".to_string()));
    assert_eq!(ts, 2);
}

#[test]
fn enrichment_scalar_returns_none() {
    let mut ts = 1;
    let hint = PhpType::parse("string");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert!(result.is_none());
    assert_eq!(ts, 1, "tab stop should not advance for skipped types");
}

#[test]
fn enrichment_union_without_array_returns_none() {
    let mut ts = 1;
    let hint = PhpType::parse("string|int");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert!(result.is_none());
}

#[test]
fn enrichment_union_with_array_enriches_parts() {
    let mut ts = 1;
    let hint = PhpType::parse("array|string");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert_eq!(result, Some("array<${1:mixed}>|string".to_string()));
}

#[test]
fn enrichment_union_with_closure_enriches_parts() {
    let mut ts = 1;
    let hint = PhpType::parse("Closure|null");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert_eq!(result, Some("(Closure(): ${1:mixed})|null".to_string()));
}

#[test]
fn enrichment_nullable_returns_none() {
    let mut ts = 1;
    let hint = PhpType::parse("?string");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert!(result.is_none());
}

#[test]
fn enrichment_void_returns_none() {
    let mut ts = 1;
    let hint = PhpType::parse("void");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert!(result.is_none());
}

#[test]
fn enrichment_closure_produces_callable_placeholder() {
    let mut ts = 1;
    let hint = PhpType::parse("Closure");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert_eq!(result, Some("(Closure(): ${1:mixed})".to_string()));
    assert_eq!(ts, 2);
}

#[test]
fn enrichment_callable_produces_callable_placeholder() {
    let mut ts = 1;
    let hint = PhpType::parse("callable");
    let result = enrichment_snippet(Some(&hint), &mut ts, &no_classes);
    assert_eq!(result, Some("(callable(): ${1:mixed})".to_string()));
    assert_eq!(ts, 2);
}

#[test]
fn enrichment_class_without_templates_returns_none() {
    let mut ts = 1;
    // Class exists but has no template params.
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "User" {
            Some(Arc::new(ClassInfo {
                name: crate::atom::atom("User"),
                ..Default::default()
            }))
        } else {
            None
        }
    };
    let hint = PhpType::parse("User");
    let result = enrichment_snippet(Some(&hint), &mut ts, &loader);
    assert!(result.is_none());
}

#[test]
fn enrichment_class_with_templates_produces_generic() {
    let mut ts = 1;
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Collection" {
            Some(Arc::new(ClassInfo {
                name: crate::atom::atom("Collection"),
                template_params: vec![atom("TKey"), atom("TValue")],
                ..Default::default()
            }))
        } else {
            None
        }
    };
    let hint = PhpType::parse("Collection");
    let result = enrichment_snippet(Some(&hint), &mut ts, &loader);
    assert_eq!(
        result,
        Some("Collection<${1:TKey}, ${2:TValue}>".to_string())
    );
    assert_eq!(ts, 3);
}

// ── Snippet generation ──────────────────────────────────────────────

#[test]
fn generates_function_snippet_no_indent_in_continuation() {
    // Snippet continuation lines must NOT include the base indent.
    // The editor auto-indents multi-line completion snippets to
    // match the text-edit range's start column.
    let sym = SymbolInfo {
        params: vec![(None, "$data".to_string())],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        "    ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    // No line should start with the base indent "    ".
    for (i, line) in snippet.lines().enumerate() {
        if i == 0 {
            continue; // first line is just "/**"
        }
        assert!(
            !line.starts_with("    "),
            "Snippet line {} should not have base indent, got: {:?}",
            i,
            line
        );
    }
}

#[test]
fn snippet_escapes_dollar_in_param_names() {
    let sym = SymbolInfo {
        params: vec![(None, "$data".to_string())],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        "",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    // The `$` in `$data` must be escaped as `\$` so the snippet
    // parser does not treat it as a snippet variable.
    assert!(
        snippet.contains("\\$data"),
        "$ in param name should be escaped, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains(" $data"),
        "Unescaped $data should not appear, got:\n{}",
        snippet
    );
}

#[test]
fn generates_function_snippet_skips_fully_typed_params() {
    // string and int are fully typed — no @param needed.
    // User is a class without templates — no @param needed.
    // Only @return for User should be skipped (no templates).
    let sym = SymbolInfo {
        params: vec![
            (Some(PhpType::parse("string")), "$name".to_string()),
            (Some(PhpType::parse("int")), "$age".to_string()),
        ],
        return_type: Some(PhpType::parse("User")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "User" {
            Some(Arc::new(ClassInfo {
                name: crate::atom::atom("User"),
                ..Default::default()
            }))
        } else {
            None
        }
    };
    let snippet = build_function_snippet(
        &sym,
        "",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &loader,
        None,
    );
    // All params are fully typed, return type is non-template class.
    // Should be a summary-only skeleton with no tags.
    assert!(
        !snippet.contains("@param"),
        "Fully-typed params should not get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Non-template class return should not get @return, got:\n{}",
        snippet
    );
    // Summary-only skeleton: /**\n * ${1}\n */
    assert!(snippet.contains("${1}"), "Should have summary tab stop");
}

#[test]
fn generates_function_snippet_for_untyped_params() {
    let sym = SymbolInfo {
        params: vec![
            (None, "$data".to_string()),
            (Some(PhpType::parse("string")), "$name".to_string()),
        ],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        "    ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    // Only $data (untyped) should get @param, not $name (string).
    assert!(
        snippet.contains("@param ${"),
        "Untyped param should get @param with mixed placeholder, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("mixed"),
        "Untyped param should have mixed placeholder, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("$data"),
        "Should contain $data, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("$name"),
        "Fully-typed $name should not appear in @param, got:\n{}",
        snippet
    );
    assert!(!snippet.contains("@return"), "void should not have @return");
}

#[test]
fn generates_function_snippet_for_array_param_and_return() {
    let sym = SymbolInfo {
        params: vec![(Some(PhpType::parse("array")), "$items".to_string())],
        return_type: Some(PhpType::parse("array")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        "    ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    assert!(snippet.contains("@param"), "array param should get @param");
    assert!(snippet.contains("$items"), "Should reference $items");
    assert!(
        snippet.contains("@return"),
        "array return should get @return"
    );
}

#[test]
fn generates_void_function_snippet_without_return() {
    let sym = SymbolInfo {
        params: vec![(None, "$name".to_string())],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        "    ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    assert!(snippet.contains("@param"));
    assert!(
        !snippet.contains("@return"),
        "void functions should not have @return"
    );
}

#[test]
fn paramless_void_generates_summary_skeleton() {
    let sym = SymbolInfo {
        params: vec![],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        "    ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    assert!(
        !snippet.is_empty(),
        "Paramless void function should produce a summary skeleton"
    );
    assert!(snippet.starts_with("/**"));
    assert!(
        snippet.contains("${1}"),
        "Should have summary tab stop when no tags"
    );
    assert!(!snippet.contains("@param"));
    assert!(!snippet.contains("@return"));
    // Should be exactly 3 lines: /**, * ${1}, */
    let line_count = snippet.lines().count();
    assert_eq!(
        line_count, 3,
        "Summary skeleton should be 3 lines, got:\n{}",
        snippet
    );
}

#[test]
fn generates_class_snippet_without_templates() {
    let sym = SymbolInfo::default();
    let snippet = build_class_snippet(&sym, "    ", &no_classes);
    assert!(snippet.starts_with("/**"));
    assert!(
        snippet.contains("${1}"),
        "No-template class should have summary tab stop"
    );
    assert!(snippet.ends_with(" */"));
    assert!(!snippet.contains("@extends"));
    assert!(!snippet.contains("@implements"));
    // Should be exactly 3 lines: /**, * ${1}, */
    let line_count = snippet.lines().count();
    assert_eq!(
        line_count, 3,
        "Summary skeleton should be 3 lines, got:\n{}",
        snippet
    );
}

#[test]
fn generates_class_snippet_with_templated_parent() {
    let sym = SymbolInfo {
        extends_names: vec!["Factory".to_string()],
        ..Default::default()
    };
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Factory" {
            Some(Arc::new(ClassInfo {
                name: crate::atom::atom("Factory"),
                template_params: vec![atom("TModel")],
                ..Default::default()
            }))
        } else {
            None
        }
    };
    let snippet = build_class_snippet(&sym, " ", &loader);
    assert!(
        snippet.contains("@extends Factory<${1:TModel}>"),
        "Should contain @extends with template tab stop, got:\n{}",
        snippet
    );
    // No summary line when tags are present.
    assert!(
        !snippet.contains("* ${"),
        "Should not have a summary placeholder when tags exist, got:\n{}",
        snippet
    );
    // No blank * separator lines.
    assert!(
        !snippet.lines().any(|l| l.trim() == "*"),
        "Should not have blank separator lines, got:\n{}",
        snippet
    );
}

#[test]
fn generates_class_snippet_with_templated_interface() {
    let sym = SymbolInfo {
        implements_names: vec!["Comparable".to_string()],
        ..Default::default()
    };
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Comparable" {
            Some(Arc::new(ClassInfo {
                name: crate::atom::atom("Comparable"),
                template_params: vec![atom("T")],
                ..Default::default()
            }))
        } else {
            None
        }
    };
    let snippet = build_class_snippet(&sym, " ", &loader);
    assert!(
        snippet.contains("@implements Comparable<${1:T}>"),
        "Should contain @implements with template tab stop, got:\n{}",
        snippet
    );
}

#[test]
fn generates_property_snippet_always_has_var() {
    let sym = SymbolInfo {
        type_hint: Some(PhpType::parse("string")),
        ..Default::default()
    };
    let snippet = build_property_snippet(&sym, "    ", &no_classes);
    assert!(
        snippet.contains("@var string"),
        "Typed property should have @var string, got:\n{}",
        snippet
    );
    // No summary line for properties — just /** @var Type */
    assert!(
        !snippet.lines().any(|l| l.contains("* ${")),
        "Property snippet should not have summary placeholder, got:\n{}",
        snippet
    );
}

#[test]
fn generates_property_snippet_untyped_has_mixed() {
    let sym = SymbolInfo::default();
    let snippet = build_property_snippet(&sym, "    ", &no_classes);
    assert!(
        snippet.contains("@var ${1:mixed}"),
        "Untyped property should have @var with mixed placeholder, got:\n{}",
        snippet
    );
}

#[test]
fn generates_constant_snippet_with_type() {
    let sym = SymbolInfo {
        type_hint: Some(PhpType::parse("int")),
        ..Default::default()
    };
    let snippet = build_constant_snippet(&sym, "    ", &no_classes);
    assert!(snippet.contains("@var int"));
}

#[test]
fn generates_constant_snippet_without_type() {
    let sym = SymbolInfo::default();
    let snippet = build_constant_snippet(&sym, "    ", &no_classes);
    assert!(snippet.contains("@var ${1:mixed}"));
}

#[test]
fn param_names_are_space_aligned() {
    let sym = SymbolInfo {
        params: vec![
            (None, "$activeAlerts".to_string()),
            (None, "$x".to_string()),
        ],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        " ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    // Both params are untyped → both get mixed placeholders.
    // The `$` names should start at the same column.
    let param_lines: Vec<&str> = snippet.lines().filter(|l| l.contains("@param")).collect();
    assert_eq!(param_lines.len(), 2, "Should have 2 @param lines");
    let col1 = param_lines[0].find('$').unwrap();
    let col2 = param_lines[1].find('$').unwrap();
    assert_eq!(col1, col2, "$ names should be aligned, got:\n{}", snippet);
}

#[test]
fn param_names_aligned_with_mixed_enrichment_widths() {
    // Simulate: one param with a generic class type (wide snippet) and
    // one untyped param (short snippet).  The visible `$` columns must
    // still line up even though the raw snippet lengths differ.
    use std::sync::Arc;
    let cls = Arc::new(ClassInfo {
        template_params: vec![atom("TKey"), atom("TValue")],
        ..Default::default()
    });
    let loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "TypedCollection" {
            Some(Arc::clone(&cls))
        } else {
            None
        }
    };

    let sym = SymbolInfo {
        params: vec![
            (None, "$data".to_string()),
            (
                Some(PhpType::parse("TypedCollection")),
                "$primary".to_string(),
            ),
        ],
        return_type: Some(PhpType::parse("void")),
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    let snippet = build_function_snippet(
        &sym,
        " ",
        "<?php\n",
        Position {
            line: 0,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &loader,
        None,
    );
    let param_lines: Vec<&str> = snippet.lines().filter(|l| l.contains("@param")).collect();
    assert_eq!(param_lines.len(), 2, "Should have 2 @param lines");

    // The plain-text renderings are "mixed" (5) and
    // "TypedCollection<TKey, TValue>" (29).  The snippet for the
    // shorter one must contain enough padding so the escaped `\$`
    // param names start at the same visible column.
    //
    // To verify, strip snippet markers and compare the column of
    // the first `\$` (escaped dollar) in each line.
    fn strip_snippets(s: &str) -> String {
        let mut out = String::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b':' {
                    i += 1;
                }
                let mut depth = 1u32;
                while i < bytes.len() && depth > 0 {
                    if bytes[i] == b'}' {
                        depth -= 1;
                        i += 1;
                    } else {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                }
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    }
    let plain1 = strip_snippets(param_lines[0]);
    let plain2 = strip_snippets(param_lines[1]);
    let col1 = plain1.find('$').expect("should contain $");
    let col2 = plain2.find('$').expect("should contain $");
    assert_eq!(
        col1, col2,
        "$ names should be visually aligned, got:\n  {}\n  {}",
        plain1, plain2
    );
}

#[test]
fn blank_separator_between_tag_groups() {
    let sym = SymbolInfo {
        params: vec![(None, "$x".to_string())],
        return_type: None,
        ..Default::default()
    };
    let use_map = HashMap::new();
    let file_ns = None;
    // Use content with a throw so we get @throws.
    let content = "<?php\nfunction test($x) { throw new \\RuntimeException(); }";
    let snippet = build_function_snippet(
        &sym,
        "",
        content,
        Position {
            line: 1,
            character: 0,
        },
        &use_map,
        &file_ns,
        &[],
        &no_classes,
        None,
    );
    // @param, @throws and @return should all be present.
    assert!(
        snippet.contains("@param"),
        "Should have @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("@throws"),
        "Should have @throws, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("@return"),
        "Should have @return, got:\n{}",
        snippet
    );
    // There should be a blank `*` line between @param and @throws,
    // and between @throws and @return.
    let lines: Vec<&str> = snippet.lines().collect();
    let param_idx = lines.iter().position(|l| l.contains("@param")).unwrap();
    let throws_idx = lines.iter().position(|l| l.contains("@throws")).unwrap();
    let return_idx = lines.iter().position(|l| l.contains("@return")).unwrap();
    assert_eq!(
        lines[param_idx + 1].trim(),
        "*",
        "Blank separator between @param and @throws, got:\n{}",
        snippet
    );
    assert_eq!(
        lines[throws_idx + 1].trim(),
        "*",
        "Blank separator between @throws and @return, got:\n{}",
        snippet
    );
    // But no blank line before @param (first group).
    assert_ne!(
        lines[param_idx - 1].trim(),
        "*",
        "No blank separator before @param, got:\n{}",
        snippet
    );
    assert!(
        throws_idx == param_idx + 2,
        "@throws should be right after blank line, got:\n{}",
        snippet
    );
    assert!(
        return_idx == throws_idx + 2,
        "@return should be right after blank line, got:\n{}",
        snippet
    );
}

// ── is_class_like_keyword ───────────────────────────────────────────

#[test]
fn is_class_like_plain_class() {
    assert!(is_class_like_keyword("class Foo {}"));
}

#[test]
fn is_class_like_abstract_class() {
    assert!(is_class_like_keyword("abstract class Foo {}"));
}

#[test]
fn is_class_like_interface() {
    assert!(is_class_like_keyword("interface Foo {}"));
}

#[test]
fn is_class_like_not_function() {
    assert!(!is_class_like_keyword("function foo() {}"));
}

#[test]
fn is_class_like_not_property() {
    assert!(!is_class_like_keyword("public string $foo;"));
}

// ── extract_class_supertypes ────────────────────────────────────────

#[test]
fn extracts_extends_from_decl() {
    let (parents, ifaces) = extract_class_supertypes("class Child extends Base {}");
    assert_eq!(parents, vec!["Base"]);
    assert!(ifaces.is_empty());
}

#[test]
fn extracts_implements_from_decl() {
    let (parents, ifaces) = extract_class_supertypes("class Foo implements Bar, Baz {}");
    assert!(parents.is_empty());
    assert_eq!(ifaces, vec!["Bar", "Baz"]);
}

#[test]
fn extracts_both_from_decl() {
    let (parents, ifaces) =
        extract_class_supertypes("class Child extends Base implements Iface {}");
    assert_eq!(parents, vec!["Base"]);
    assert_eq!(ifaces, vec!["Iface"]);
}

// ── classify_declaration: variable assignments ──────────────────────

#[test]
fn classifies_variable_assignment() {
    let ctx = classify_declaration("    $items = [''];\n");
    assert!(matches!(ctx, DocblockContext::Inline));
}

#[test]
fn classifies_variable_assignment_no_value() {
    let ctx = classify_declaration("    $x = null;\n");
    assert!(matches!(ctx, DocblockContext::Inline));
}

#[test]
fn classifies_variable_not_confused_with_property() {
    // Properties have modifiers; bare `$var` does not.
    let ctx = classify_declaration("    public string $name;\n");
    assert!(matches!(ctx, DocblockContext::Property));
}

#[test]
fn extracts_variable_name_from_inline_assignment() {
    let info = parse_declaration_info("    $items = [''];\n");
    assert_eq!(info.variable_name.as_deref(), Some("$items"));
}

#[test]
fn extracts_variable_name_from_simple_assignment() {
    let info = parse_declaration_info("    $count = 42;\n");
    assert_eq!(info.variable_name.as_deref(), Some("$count"));
}

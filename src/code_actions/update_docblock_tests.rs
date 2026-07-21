use super::*;

/// Helper: parse PHP and check if an update is needed at the given offset.
fn find_info(php: &str, offset: u32) -> Option<FunctionWithDocblock> {
    let arena = LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
    let ctx = find_cursor_context(&program.statements, offset);
    find_function_with_docblock_from_context(
        &ctx,
        &program.statements,
        program.trivia.as_slice(),
        php,
        offset,
    )
}

/// Stub class loader that never resolves anything (for unit tests).
fn no_class_loader() -> impl Fn(&str) -> Option<Arc<ClassInfo>> {
    |_| None
}

/// No function loader (for unit tests).
fn no_function_loader() -> FunctionLoader<'static> {
    None
}

#[test]
fn detects_missing_param() {
    let php = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @param string $a The first param
     */
    public function bar(string $a, int $b): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn detects_extra_param() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     */
    public function bar(string $a): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn detects_reordered_params() {
    let php = r#"<?php
class Foo {
    /**
     * @param int $b
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
    let pos = php.find("@param int").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn no_update_when_params_match() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     */
    public function bar(string $a, int $b): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(!check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn detects_type_contradiction_in_param() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(int $a): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn preserves_refinement_type() {
    let php = r#"<?php
class Foo {
    /**
     * @param non-empty-string $a
     */
    public function bar(string $a): void {}
}
"#;
    let pos = php.find("@param non-empty-string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(!check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn detects_void_return_redundancy() {
    let php = r#"<?php
class Foo {
    /**
     * @return void
     */
    public function bar(): void {}
}
"#;
    let pos = php.find("@return void").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn detects_return_type_contradiction() {
    let php = r#"<?php
class Foo {
    /**
     * @return string
     */
    public function bar(): int {}
}
"#;
    let pos = php.find("@return string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn no_action_without_docblock() {
    let php = r#"<?php
class Foo {
    public function bar(string $a): void {}
}
"#;
    // No docblock at all — cursor on signature should return None.
    let pos = php.find("function bar").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(info.is_none());
}

#[test]
fn works_with_standalone_function() {
    let php = r#"<?php
/**
 * @param string $a
 * @param int $b
 */
function bar(string $a, int $b, bool $c): void {}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn preserves_descriptions() {
    let php = r#"<?php
class Foo {
    /**
     * Summary line.
     *
     * @param string $a The first param
     */
    public function bar(string $a, int $b): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        updated.contains("The first param"),
        "Should preserve description: {}",
        updated
    );
    assert!(
        updated.contains("$b"),
        "Should add missing param: {}",
        updated
    );
    assert!(
        updated.contains("Summary line"),
        "Should preserve summary: {}",
        updated
    );
}

#[test]
fn removes_extra_param_and_adds_missing() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $old
     * @param int $b
     */
    public function bar(int $b, bool $c): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        !updated.contains("$old"),
        "Should remove old param: {}",
        updated
    );
    assert!(updated.contains("$b"), "Should keep $b: {}", updated);
    assert!(updated.contains("$c"), "Should add $c: {}", updated);
}

#[test]
fn updates_contradicted_return_type() {
    let php = r#"<?php
class Foo {
    /**
     * @return string Some description
     */
    public function bar(): int {}
}
"#;
    let pos = php.find("@return string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        updated.contains("@return int Some description"),
        "Should update return type: {}",
        updated
    );
}

#[test]
fn removes_void_return() {
    let php = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @return void
     */
    public function bar(): void {}
}
"#;
    let pos = php.find("@return void").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        !updated.contains("@return"),
        "Should remove @return void: {}",
        updated
    );
}

#[test]
fn handles_variadic_param() {
    let php = r#"<?php
class Foo {
    /**
     * @param string ...$args
     */
    public function bar(string ...$args): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    // Variadic params should match — no update needed.
    assert!(!check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn preserves_generic_refinement() {
    let php = r#"<?php
class Foo {
    /**
     * @param array<int, string> $items
     */
    public function bar(array $items): void {}
}
"#;
    let pos = php.find("@param array").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    // array<int, string> refines array — no contradiction.
    assert!(!check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn preserves_other_tags() {
    let php = r#"<?php
class Foo {
    /**
     * Summary.
     *
     * @template T
     * @param string $a
     * @throws \RuntimeException
     */
    public function bar(string $a, int $b): void {}
}
"#;
    let pos = php.find("@template T").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        updated.contains("@template T"),
        "Should preserve @template: {}",
        updated
    );
    assert!(
        updated.contains("@throws"),
        "Should preserve @throws: {}",
        updated
    );
    assert!(
        updated.contains("$b"),
        "Should add missing param: {}",
        updated
    );
}

#[test]
fn is_contradiction_basic() {
    assert!(is_type_contradiction(
        &PhpType::parse("string"),
        &PhpType::parse("int")
    ));
    assert!(!is_type_contradiction(
        &PhpType::parse("string"),
        &PhpType::parse("string")
    ));
    assert!(!is_type_contradiction(
        &PhpType::parse("non-empty-string"),
        &PhpType::parse("string")
    ));
    assert!(!is_type_contradiction(
        &PhpType::parse("array<int, string>"),
        &PhpType::parse("array")
    ));
}

#[test]
fn is_contradiction_nullable() {
    // ?string and string|null are equivalent.
    assert!(!is_type_contradiction(
        &PhpType::parse("?string"),
        &PhpType::parse("?string")
    ));
    assert!(!is_type_contradiction(
        &PhpType::parse("string|null"),
        &PhpType::parse("?string")
    ));
}

#[test]
fn works_in_namespace() {
    let php = r#"<?php
namespace App;
class Foo {
    /**
     * @param string $a
     */
    public function bar(int $a): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn aligns_param_columns() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b, array $items): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    // All $names should be aligned at the same column.
    assert!(
        updated.contains("@param string       $a"),
        "Should have string padded: {}",
        updated
    );
    assert!(
        updated.contains("@param int          $b"),
        "Should have int padded: {}",
        updated
    );
    assert!(
        updated.contains("@param array<mixed> $items"),
        "Should have array<mixed> padded: {}",
        updated
    );
}

#[test]
fn no_spurious_blank_line_after_open() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     *
     * @return string
     */
    public function bar(string $a, int $b, bool $c): string {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    // Should NOT have a blank line between /** and the first @param.
    let lines: Vec<&str> = updated.lines().collect();
    assert_eq!(
        lines[0].trim(),
        "/**",
        "First line should be opening: {}",
        updated
    );
    assert!(
        lines[1].trim().starts_with("* @param"),
        "Second line should be @param, not blank: {}",
        updated
    );
}

#[test]
fn enriches_callable_types() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, Closure $handler, callable $fallback): void {}
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        updated.contains("(Closure(): mixed)"),
        "Should enrich Closure: {}",
        updated
    );
    assert!(
        updated.contains("(callable(): mixed)"),
        "Should enrich callable: {}",
        updated
    );
}

#[test]
fn adds_missing_throws() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     *
     * @return string
     */
    public function bar(string $a): string {
        throw new \RuntimeException('oops');
    }
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    let updated = build_updated_docblock(
        &info,
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    );
    assert!(
        updated.contains("@throws RuntimeException"),
        "Should add missing @throws: {}",
        updated
    );
}

#[test]
fn does_not_duplicate_existing_throws() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     *
     * @throws RuntimeException
     *
     * @return string
     */
    public function bar(string $a): string {
        throw new \RuntimeException('oops');
    }
}
"#;
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        !check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "Should not need update when throws already documented"
    );
}

#[test]
fn triggers_when_cursor_inside_docblock() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
    // Place the cursor on the @param line inside the docblock.
    let pos = php.find("@param string").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_some(),
        "Should find function info when cursor is inside the docblock"
    );
    let cl = no_class_loader();
    assert!(check_needs_update(
        &info.unwrap(),
        php,
        &[],
        &cl,
        no_function_loader(),
        &HashMap::new(),
        &None,
    ));
}

#[test]
fn triggers_when_cursor_on_docblock_summary() {
    let php = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
    // Place the cursor on the summary line.
    let pos = php.find("Does something").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_some(),
        "Should find function info when cursor is on docblock summary"
    );
}

#[test]
fn triggers_when_cursor_on_opening_docblock() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
    // Place the cursor on the /** line.
    let pos = php.find("/**").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_some(),
        "Should find function info when cursor is on opening /**"
    );
}

// ── @param with no type ─────────────────────────────────────────

/// Helper: parse a docblock string into params via `_from_info`.
fn test_parse_params(docblock: &str) -> Vec<DocParam> {
    match parse_docblock_for_tags(docblock) {
        Some(info) => parse_doc_params_from_info(&info),
        None => Vec::new(),
    }
}

#[test]
fn parse_param_no_type_recognised() {
    let docblock = r#"/**
     * @param $name The user name
     */"#;
    let params = test_parse_params(docblock);
    assert_eq!(params.len(), 1, "should parse one param: {:?}", params);
    assert_eq!(params[0].name, "$name");
    assert_eq!(params[0].type_str_raw, "");
    assert_eq!(params[0].description, "The user name");
}

#[test]
fn parse_param_no_type_variadic() {
    let docblock = r#"/**
     * @param ...$args The arguments
     */"#;
    let params = test_parse_params(docblock);
    assert_eq!(params.len(), 1, "should parse one param: {:?}", params);
    assert_eq!(params[0].name, "...$args");
    assert_eq!(params[0].type_str_raw, "");
    assert_eq!(params[0].description, "The arguments");
}

#[test]
fn parse_param_no_type_no_description() {
    let docblock = r#"/**
     * @param $name
     */"#;
    let params = test_parse_params(docblock);
    assert_eq!(params.len(), 1, "should parse one param: {:?}", params);
    assert_eq!(params[0].name, "$name");
    assert_eq!(params[0].type_str_raw, "");
}

#[test]
fn parse_param_no_type_mixed_with_typed() {
    let docblock = r#"/**
     * @param string $a First
     * @param $b Second
     * @param int $c Third
     */"#;
    let params = test_parse_params(docblock);
    assert_eq!(params.len(), 3, "should parse three params: {:?}", params);
    assert_eq!(params[0].name, "$a");
    assert_eq!(params[0].type_str_raw, "string");
    assert_eq!(params[1].name, "$b");
    assert_eq!(params[1].type_str_raw, "");
    assert_eq!(params[1].description, "Second");
    assert_eq!(params[2].name, "$c");
    assert_eq!(params[2].type_str_raw, "int");
}

#[test]
fn update_needed_when_untyped_param_matches_untyped_sig() {
    // Even when both the docblock and signature omit the type, the
    // update action should fire to add `mixed` as the explicit type.
    let php = r#"<?php
class Foo {
    /**
     * @param $name The user name
     */
    public function bar($name): void {}
}
"#;
    let pos = php.find("@param $name").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "should need update to add `mixed` type to @param $name"
    );
    // The param must still be recognised (not duplicated).
    assert_eq!(info.doc_params.len(), 1);
    assert_eq!(info.doc_params[0].name, "$name");
    assert_eq!(info.doc_params[0].type_str_raw, "");
    assert_eq!(info.doc_params[0].description, "The user name");
}

#[test]
fn detects_missing_param_when_existing_has_no_type() {
    let php = r#"<?php
class Foo {
    /**
     * @param $a First param
     */
    public function bar(string $a, int $b): void {}
}
"#;
    let pos = php.find("@param $a").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "should need update because $b is missing"
    );
    assert_eq!(info.doc_params.len(), 1);
    assert_eq!(info.doc_params[0].name, "$a");
    assert_eq!(info.doc_params[0].description, "First param");
}

#[test]
fn no_update_for_empty_docblock_with_fully_typed_params() {
    // When generate-docblock produces `/** */` (no @param tags) because
    // the native types are sufficient, update-docblock should NOT offer
    // to add redundant @param tags.
    let php = r#"<?php
class Foo {
    /**
     *
     */
    public function stepIntro(CustomerRequest $request): View {}
}
"#;
    let pos = php.find("/**").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        !check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "should not suggest adding @param for a fully-typed non-templated class param"
    );
}

#[test]
fn no_update_for_empty_docblock_with_scalar_params() {
    let php = r#"<?php
class Foo {
    /**
     *
     */
    public function bar(string $a, int $b, bool $c): void {}
}
"#;
    let pos = php.find("/**").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        !check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "should not suggest adding @param for scalar-typed params"
    );
}

#[test]
fn update_for_empty_docblock_with_untyped_param() {
    // When a param has no native type, enrichment produces `mixed`,
    // so the update should be offered.
    let php = r#"<?php
class Foo {
    /**
     *
     */
    public function bar($untyped): void {}
}
"#;
    let pos = php.find("/**").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "should suggest adding @param for an untyped param"
    );
}

#[test]
fn update_for_empty_docblock_with_array_param() {
    // `array` is enrichable (stays `array` but signals it needs a shape
    // or value-type annotation), so the update should be offered.
    let php = r#"<?php
class Foo {
    /**
     *
     */
    public function bar(array $items): void {}
}
"#;
    let pos = php.find("/**").unwrap() as u32;
    let info = find_info(php, pos).unwrap();
    let cl = no_class_loader();
    assert!(
        check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None
        ),
        "should suggest adding @param for an array param"
    );
}

#[test]
fn no_info_inside_method_body() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
    // Place cursor on `$x = 1;` inside the method body.
    let pos = php.find("$x = 1").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_none(),
        "should not offer update docblock inside method body"
    );
}

#[test]
fn no_info_on_method_opening_brace() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
    // Place cursor on the opening brace of the method body.
    let pos = php.find("{\n        $x").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_none(),
        "should not offer update docblock on method body brace"
    );
}

#[test]
fn no_info_on_method_name() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
    let pos = php.find("bar").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_none(),
        "should not offer update docblock when cursor is on method name"
    );
}

#[test]
fn no_info_on_method_return_type() {
    let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
    let pos = php.find("void").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_none(),
        "should not offer update docblock when cursor is on return type hint"
    );
}

#[test]
fn no_info_inside_standalone_function_body() {
    let php = r#"<?php
/**
 * @param string $a
 */
function foo(string $a, int $b): void {
    $x = 1;
}
"#;
    let pos = php.find("$x = 1").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_none(),
        "should not offer update docblock inside standalone function body"
    );
}

#[test]
fn no_info_on_standalone_function_signature() {
    let php = r#"<?php
/**
 * @param string $a
 */
function foo(string $a, int $b): void {
    $x = 1;
}
"#;
    let pos = php.find("function foo").unwrap() as u32;
    let info = find_info(php, pos);
    assert!(
        info.is_none(),
        "should not offer update docblock when cursor is on standalone function signature"
    );
}

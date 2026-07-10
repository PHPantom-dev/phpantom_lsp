//! Integration tests for PHP version-aware stub filtering.
//!
//! Tests that `#[PhpStormStubsElementAvailable]` attributes on functions,
//! methods, and parameters are respected when a target PHP version is set.

use crate::common::create_test_backend;
use phpantom_lsp::Backend;
use phpantom_lsp::types::PhpVersion;
use tower_lsp::lsp_types::{HoverContents, Position};

// ─── PhpVersion parsing ─────────────────────────────────────────────────────

#[test]
fn parse_caret_constraint() {
    let v = PhpVersion::from_composer_constraint("^8.4").unwrap();
    assert_eq!(v, PhpVersion::new(8, 4));
}

#[test]
fn parse_gte_constraint() {
    let v = PhpVersion::from_composer_constraint(">=8.3").unwrap();
    assert_eq!(v, PhpVersion::new(8, 3));
}

#[test]
fn parse_tilde_constraint() {
    let v = PhpVersion::from_composer_constraint("~8.2").unwrap();
    assert_eq!(v, PhpVersion::new(8, 2));
}

#[test]
fn parse_wildcard_constraint() {
    let v = PhpVersion::from_composer_constraint("8.1.*").unwrap();
    assert_eq!(v, PhpVersion::new(8, 1));
}

#[test]
fn parse_exact_version() {
    let v = PhpVersion::from_composer_constraint("8.3.1").unwrap();
    assert_eq!(v, PhpVersion::new(8, 3));
}

#[test]
fn parse_major_only() {
    let v = PhpVersion::from_composer_constraint("^8").unwrap();
    assert_eq!(v, PhpVersion::new(8, 0));
}

#[test]
fn parse_range_takes_first() {
    // ">=8.0 <8.4" → first match wins → 8.0
    let v = PhpVersion::from_composer_constraint(">=8.0 <8.4").unwrap();
    assert_eq!(v, PhpVersion::new(8, 0));
}

#[test]
fn parse_pipe_separated() {
    let v = PhpVersion::from_composer_constraint("^7.4|^8.0").unwrap();
    assert_eq!(v, PhpVersion::new(7, 4));
}

#[test]
fn parse_empty_returns_none() {
    assert!(PhpVersion::from_composer_constraint("").is_none());
}

#[test]
fn parse_garbage_returns_none() {
    assert!(PhpVersion::from_composer_constraint("not-a-version").is_none());
}

#[test]
fn default_version_is_8_5() {
    let v = PhpVersion::default();
    assert_eq!(v, PhpVersion::new(8, 5));
}

// ─── matches_range ──────────────────────────────────────────────────────────

#[test]
fn matches_range_unbounded() {
    let v = PhpVersion::new(8, 4);
    assert!(v.matches_range(None, None));
}

#[test]
fn matches_range_from_only_matches() {
    let v = PhpVersion::new(8, 4);
    assert!(v.matches_range(Some(PhpVersion::new(8, 0)), None));
}

#[test]
fn matches_range_from_only_too_low() {
    let v = PhpVersion::new(7, 4);
    assert!(!v.matches_range(Some(PhpVersion::new(8, 0)), None));
}

#[test]
fn matches_range_to_only_matches() {
    let v = PhpVersion::new(7, 4);
    assert!(v.matches_range(None, Some(PhpVersion::new(7, 4))));
}

#[test]
fn matches_range_to_only_too_high() {
    let v = PhpVersion::new(8, 0);
    assert!(!v.matches_range(None, Some(PhpVersion::new(7, 4))));
}

#[test]
fn matches_range_exact() {
    let v = PhpVersion::new(8, 0);
    assert!(v.matches_range(Some(PhpVersion::new(8, 0)), Some(PhpVersion::new(8, 0))));
}

#[test]
fn matches_range_within() {
    let v = PhpVersion::new(8, 1);
    assert!(v.matches_range(Some(PhpVersion::new(8, 0)), Some(PhpVersion::new(8, 4))));
}

#[test]
fn matches_range_outside_below() {
    let v = PhpVersion::new(7, 4);
    assert!(!v.matches_range(Some(PhpVersion::new(8, 0)), Some(PhpVersion::new(8, 4))));
}

#[test]
fn matches_range_outside_above() {
    let v = PhpVersion::new(8, 5);
    assert!(!v.matches_range(Some(PhpVersion::new(8, 0)), Some(PhpVersion::new(8, 4))));
}

// ─── Composer version detection ─────────────────────────────────────────────

#[test]
fn detect_version_from_require_php() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{ "require": { "php": "^8.4" } }"#,
    )
    .unwrap();
    let v = phpantom_lsp::composer::detect_php_version(dir.path()).unwrap();
    assert_eq!(v, PhpVersion::new(8, 4));
}

#[test]
fn detect_version_from_platform_php() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{ "config": { "platform": { "php": "8.3.1" } }, "require": { "php": "^8.4" } }"#,
    )
    .unwrap();
    // platform.php takes priority over require.php
    let v = phpantom_lsp::composer::detect_php_version(dir.path()).unwrap();
    assert_eq!(v, PhpVersion::new(8, 3));
}

#[test]
fn detect_version_no_composer_json() {
    let dir = tempfile::tempdir().unwrap();
    let v = phpantom_lsp::composer::detect_php_version(dir.path());
    assert!(v.is_none());
}

#[test]
fn detect_version_no_php_constraint() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{ "require": { "laravel/framework": "^11.0" } }"#,
    )
    .unwrap();
    let v = phpantom_lsp::composer::detect_php_version(dir.path());
    assert!(v.is_none());
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Register file content in the backend and return hover result.
fn hover_at(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Option<tower_lsp::lsp_types::Hover> {
    backend.update_ast(uri, content);
    backend.handle_hover(uri, content, Position { line, character })
}

fn hover_text(hover: &tower_lsp::lsp_types::Hover) -> &str {
    match &hover.contents {
        HoverContents::Markup(markup) => &markup.value,
        _ => panic!("Expected MarkupContent"),
    }
}

// ─── Function-level version filtering ───────────────────────────────────────

#[test]
fn function_level_php80_picks_correct_variant() {
    // Two variants of the same function: one for <=7.4, one for >=8.0.
    // With PHP 8.4, only the 8.0+ variant should survive.
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(8, 4));

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

/**
 * @return array|false
 */
#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')]
function array_combine(array $keys, array $values): array|false {}

/**
 * @return array
 */
#[PhpStormStubsElementAvailable(from: '8.0')]
function array_combine(array $keys, array $values): array {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(
        functions.len(),
        1,
        "should have exactly one function variant"
    );
    // The 8.0+ variant has return type `array` (not `array|false`).
    assert_eq!(
        functions[0]
            .native_return_type
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("array"),
        "should pick the PHP 8.0+ variant"
    );
}

#[test]
fn function_level_php74_picks_legacy_variant() {
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')]
function array_combine(array $keys, array $values): array|false {}

#[PhpStormStubsElementAvailable(from: '8.0')]
function array_combine(array $keys, array $values): array {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(7, 4)));
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0]
            .native_return_type
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("array|false"),
        "should pick the PHP 5.3-7.4 variant"
    );
}

#[test]
fn language_level_return_type_wins_over_stale_return_docblock() {
    // phpstorm-stubs keep the legacy `@return resource|false` docblock but
    // declare the version-aware type via `#[LanguageLevelTypeAware]`. On PHP
    // 8.1+ the attribute type (`finfo|false`) must win over the stale docblock
    // (`resource|false`) instead of being widened back to a resource.
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

/**
 * Create a new fileinfo resource
 * @return resource|false a magic database resource on success or FALSE on failure.
 */
#[LanguageLevelTypeAware(['8.1' => 'finfo|false'], default: 'resource|false')]
function finfo_open(int $flags = 0, ?string $magic_database = null) {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0]
            .return_type
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("finfo|false"),
        "8.1+ attribute type should win over the legacy @return docblock"
    );
}

#[test]
fn language_level_return_type_uses_default_on_old_version() {
    // On PHP 7.4 the attribute has no matching entry, so it falls back to its
    // `default` (`resource|false`), which happens to equal the docblock.
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

/**
 * @return resource|false a magic database resource on success or FALSE on failure.
 */
#[LanguageLevelTypeAware(['8.1' => 'finfo|false'], default: 'resource|false')]
function finfo_open(int $flags = 0, ?string $magic_database = null) {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(7, 4)));
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0]
            .return_type
            .as_ref()
            .map(|t| t.to_string())
            .as_deref(),
        Some("resource|false"),
        "pre-8.1 should use the attribute default"
    );
}

#[test]
fn function_without_version_attribute_always_included() {
    let backend = create_test_backend();

    let stub_content = r#"<?php
function always_available(string $arg): string {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "always_available");
}

#[test]
fn function_with_positional_from_argument() {
    // `#[PhpStormStubsElementAvailable('8.1')]` — positional arg = from
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

#[PhpStormStubsElementAvailable('8.1')]
function new_function(): void {}
"#;

    // PHP 8.0 — should be excluded
    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 0)));
    assert_eq!(
        functions.len(),
        0,
        "function should be excluded for PHP 8.0"
    );

    // PHP 8.1 — should be included
    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 1)));
    assert_eq!(
        functions.len(),
        1,
        "function should be included for PHP 8.1"
    );
}

#[test]
fn function_with_to_only() {
    // Available up to 7.4 only
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

#[PhpStormStubsElementAvailable(to: '7.4')]
function legacy_only(): void {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 0)));
    assert_eq!(functions.len(), 0, "should be excluded for PHP 8.0");

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(7, 4)));
    assert_eq!(functions.len(), 1, "should be included for PHP 7.4");
}

// ─── Parameter-level version filtering ──────────────────────────────────────

#[test]
fn parameter_version_filtering_php80() {
    // Like the real array_map stub: one param for 8.0+, another for <=7.4
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

function array_map(
    ?callable $callback,
    #[PhpStormStubsElementAvailable(from: '8.0')] array $array,
    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $arrays,
    array ...$arrays
): array {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions.len(), 1);
    let params = &functions[0].parameters;

    // Should have: $callback, $array (8.0+), ...$arrays
    // Should NOT have the bare $arrays (5.3-7.4)
    let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["$callback", "$array", "$arrays"]);

    // $array should be typed `array`
    assert_eq!(params[1].type_hint_str().as_deref(), Some("array"));
    assert_eq!(params[1].name, "$array");

    // ...$arrays should be variadic
    assert!(params[2].is_variadic);
}

#[test]
fn parameter_version_filtering_php74() {
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

function array_map(
    ?callable $callback,
    #[PhpStormStubsElementAvailable(from: '8.0')] array $array,
    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $arrays,
    array ...$arrays
): array {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(7, 4)));
    assert_eq!(functions.len(), 1);
    let params = &functions[0].parameters;

    // Should have: $callback, $arrays (5.3-7.4 untyped), ...$arrays
    // Should NOT have `array $array` (8.0+)
    let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["$callback", "$arrays", "$arrays"]);

    // The first $arrays (5.3-7.4) has no type hint
    assert_eq!(params[1].type_hint, None);
    assert!(!params[1].is_variadic);

    // The second $arrays is variadic
    assert!(params[2].is_variadic);
}

#[test]
fn parameter_without_version_attribute_always_included() {
    let backend = create_test_backend();

    let stub_content = r#"<?php
function my_func(string $always, int $present): void {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions[0].parameters.len(), 2);
}

#[test]
fn parameter_with_from_only_added_in_later_version() {
    // Parameter added in PHP 7.0
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

function unserialize(string $data, #[PhpStormStubsElementAvailable(from: '7.0')] array $options = []): mixed {}
"#;

    // PHP 7.0+ — should include $options
    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions[0].parameters.len(), 2);

    // PHP 5.6 — should exclude $options
    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(5, 6)));
    assert_eq!(functions[0].parameters.len(), 1);
    assert_eq!(functions[0].parameters[0].name, "$data");
}

// ─── Method-level version filtering ─────────────────────────────────────────

#[test]
fn method_version_filtering() {
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(8, 4));

    let content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

class SplFixedArray {
    #[PhpStormStubsElementAvailable(from: '8.2')]
    public function __serialize(): array {}

    #[PhpStormStubsElementAvailable(from: '8.2')]
    public function __unserialize(array $data): void {}

    #[PhpStormStubsElementAvailable(to: '7.4')]
    public function legacyMethod(): void {}

    public function alwaysAvailable(): void {}
}
"#;

    let classes = Backend::parse_php_versioned(content, Some(PhpVersion::new(8, 4)));
    assert_eq!(classes.len(), 1);
    let method_names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();

    // Should include __serialize, __unserialize, alwaysAvailable
    // Should exclude legacyMethod (to: 7.4)
    assert!(
        method_names.contains(&"__serialize"),
        "should include __serialize"
    );
    assert!(
        method_names.contains(&"__unserialize"),
        "should include __unserialize"
    );
    assert!(
        method_names.contains(&"alwaysAvailable"),
        "should include alwaysAvailable"
    );
    assert!(
        !method_names.contains(&"legacyMethod"),
        "should exclude legacyMethod"
    );
}

#[test]
fn method_version_filtering_picks_legacy() {
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(7, 4));

    let content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

class SplFixedArray {
    #[PhpStormStubsElementAvailable(from: '8.2')]
    public function __serialize(): array {}

    #[PhpStormStubsElementAvailable(to: '7.4')]
    public function legacyMethod(): void {}

    public function alwaysAvailable(): void {}
}
"#;

    let classes = Backend::parse_php_versioned(content, Some(PhpVersion::new(7, 4)));
    assert_eq!(classes.len(), 1);
    let method_names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();

    assert!(
        !method_names.contains(&"__serialize"),
        "should exclude __serialize for 7.4"
    );
    assert!(
        method_names.contains(&"legacyMethod"),
        "should include legacyMethod for 7.4"
    );
    assert!(
        method_names.contains(&"alwaysAvailable"),
        "should include alwaysAvailable"
    );
}

// ─── No filtering without version ───────────────────────────────────────────

#[test]
fn no_version_includes_all_variants() {
    // When no PHP version is set (None), all variants should be included.
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')]
function my_func(): array|false {}

#[PhpStormStubsElementAvailable(from: '8.0')]
function my_func(): array {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, None);
    assert_eq!(
        functions.len(),
        2,
        "without version filtering, both variants should be present"
    );
}

// ─── Method parameter version filtering ─────────────────────────────────────

#[test]
fn method_parameter_version_filtering() {
    let content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

class FilesystemIterator {
    public function setFlags(
        #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $flags = null,
        #[PhpStormStubsElementAvailable(from: '8.0')] int $flags
    ): void {}
}
"#;

    // PHP 8.4 — should get `int $flags`
    let classes = Backend::parse_php_versioned(content, Some(PhpVersion::new(8, 4)));
    let method = &classes[0].methods[0];
    assert_eq!(method.parameters.len(), 1);
    assert_eq!(method.parameters[0].name, "$flags");
    assert_eq!(method.parameters[0].type_hint_str().as_deref(), Some("int"));

    // PHP 7.4 — should get untyped `$flags = null`
    let classes = Backend::parse_php_versioned(content, Some(PhpVersion::new(7, 4)));
    let method = &classes[0].methods[0];
    assert_eq!(method.parameters.len(), 1);
    assert_eq!(method.parameters[0].name, "$flags");
    assert_eq!(method.parameters[0].type_hint, None);
}

// ─── Backend php_version accessor ───────────────────────────────────────────

#[test]
fn backend_default_version() {
    let backend = create_test_backend();
    assert_eq!(backend.php_version(), PhpVersion::new(8, 5));
}

#[test]
fn backend_set_version() {
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(8, 2));
    assert_eq!(backend.php_version(), PhpVersion::new(8, 2));
}

// ─── End-to-end: hover uses version filtering ───────────────────────────────

#[test]
fn hover_shows_version_filtered_function_signature() {
    // Simulate the array_map issue: with PHP 8.4, hover should show
    // `array $array` (not the untyped `$arrays` from 7.4)
    let backend = create_test_backend();
    backend.set_php_version(PhpVersion::new(8, 4));

    // Inject a versioned stub function
    let stub_content: &str = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable;

/**
 * Applies the callback to the elements of the given arrays
 * @link https://php.net/manual/en/function.array-map.php
 * @param callable|null $callback
 * @param array $array
 * @param array ...$arrays
 * @return array
 */
function array_map(
    ?callable $callback,
    #[PhpStormStubsElementAvailable(from: '8.0')] array $array,
    #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $arrays,
    array ...$arrays
): array {}
"#;

    // Manually inject the function into global_functions using versioned parsing
    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    {
        let mut fmap = backend.global_functions().write();
        for func in functions {
            fmap.insert(
                func.name.to_string(),
                ("phpantom-stub-fn://array_map".to_string(), func),
            );
        }
    }

    let content = r#"<?php
array_map(null, []);
"#;
    let uri = "file:///test.php";

    let hover = hover_at(&backend, uri, content, 1, 2);
    if let Some(hover) = hover {
        let text = hover_text(&hover);
        // The signature should NOT contain the untyped `$arrays` parameter
        // that is only for PHP 5.3-7.4.
        assert!(
            !text.contains("$arrays, array ...$arrays"),
            "should not have both $arrays variants in: {}",
            text
        );
        // It should show `array $array` from the 8.0+ variant
        if text.contains("array_map") {
            assert!(
                text.contains("array $array"),
                "should show typed `array $array`: {}",
                text
            );
        }
    }
}

// ─── Aliased Attribute Names ────────────────────────────────────────────────

#[test]
fn element_available_alias_filters_parameters() {
    // intl/intl.php aliases PhpStormStubsElementAvailable as ElementAvailable.
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable as ElementAvailable;

function normalizer_normalize(
    string $string,
    #[ElementAvailable(from: '5.3', to: '5.6')] $form,
    #[ElementAvailable(from: '7.0')] int $form = 16,
    #[ElementAvailable(from: '5.3', to: '5.6')] $arg3
): string|false {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions.len(), 1);
    // On PHP 8.4, only the 7.0+ parameter variant should survive (plus $string).
    let params: Vec<&str> = functions[0]
        .parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        params,
        vec!["$string", "$form"],
        "old params should be filtered out"
    );
}

#[test]
fn element_available_alias_filters_parameters_legacy() {
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable as ElementAvailable;

function normalizer_normalize(
    string $string,
    #[ElementAvailable(from: '5.3', to: '5.6')] $form,
    #[ElementAvailable(from: '7.0')] int $form = 16,
    #[ElementAvailable(from: '5.3', to: '5.6')] $arg3
): string|false {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(5, 4)));
    assert_eq!(functions.len(), 1);
    let params: Vec<&str> = functions[0]
        .parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        params,
        vec!["$string", "$form", "$arg3"],
        "new param should be filtered out"
    );
}

#[test]
fn available_alias_filters_functions() {
    // ldap/ldap.php aliases PhpStormStubsElementAvailable as Available.
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable as Available;

#[Available(from: '8.0')]
function ldap_exop_refresh($ldap, string $dn, int $ttl): int|false {}

#[Available(from: '5.3', to: '7.4')]
function ldap_old_function(): bool {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "ldap_exop_refresh");
}

#[test]
fn available_alias_filters_parameters() {
    let backend = create_test_backend();

    let stub_content = r#"<?php
use JetBrains\PhpStorm\Internal\PhpStormStubsElementAvailable as Available;

function ldap_exop_passwd(
    $ldap,
    #[Available(from: '7.1', to: '7.1')] string $user = "",
    #[Available(from: '7.2', to: '7.2')] string $user,
    #[Available(from: '7.3')] string $user = "",
    #[Available(from: '7.3')] &$controls = null
): string|bool {}
"#;

    let functions = backend.parse_functions_versioned(stub_content, Some(PhpVersion::new(8, 0)));
    assert_eq!(functions.len(), 1);
    let params: Vec<&str> = functions[0]
        .parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        params,
        vec!["$ldap", "$user", "$controls"],
        "only 7.3+ params should survive"
    );
}

// ─── Display ────────────────────────────────────────────────────────────────

#[test]
fn php_version_display() {
    assert_eq!(PhpVersion::new(8, 4).to_string(), "8.4");
    assert_eq!(PhpVersion::new(7, 0).to_string(), "7.0");
}

// ─── LanguageLevelTypeAware — function return types ─────────────────────────

#[test]
fn language_level_function_return_type_selects_matching_version() {
    let backend = create_test_backend();

    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

#[LanguageLevelTypeAware(["8.0" => "int"], default: "int|false")]
function sleep(int $seconds): int|false {}
"#;

    // PHP 8.0+ should get "int"
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 0)));
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("int"),
        "PHP 8.0 should select the 8.0 variant"
    );

    // PHP 7.4 should get the default "int|false"
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(7, 4)));
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("int|false"),
        "PHP 7.4 should fall back to default"
    );
}

#[test]
fn language_level_function_return_type_multi_version() {
    let backend = create_test_backend();

    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

#[LanguageLevelTypeAware(['8.0' => 'int|false', '8.1' => 'int'], default: 'int')]
function bzerror(): int {}
"#;

    // PHP 8.1+ should get "int" (highest matching)
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 4)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("int"),
        "PHP 8.4 should select 8.1 variant (highest <= target)"
    );

    // PHP 8.0 should get "int|false" (exact match for 8.0)
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 0)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("int|false"),
        "PHP 8.0 should select the 8.0 variant"
    );

    // PHP 7.4 should get the default "int"
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(7, 4)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("int"),
        "PHP 7.4 should fall back to default"
    );
}

#[test]
fn language_level_function_return_type_empty_default() {
    let backend = create_test_backend();

    // Empty default means "no type" for older PHP versions.
    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

#[LanguageLevelTypeAware(['8.2' => 'true'], default: '')]
function phpinfo(int $flags = 0): bool {}
"#;

    // PHP 8.2+ should get "true"
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 2)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("true"),
        "PHP 8.2 should select the 8.2 variant"
    );

    // PHP 8.1 should fall back to the native type since default is empty
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 1)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("bool"),
        "PHP 8.1 should fall back to native type when default is empty"
    );
}

#[test]
fn language_level_without_attribute_keeps_native_type() {
    let backend = create_test_backend();

    let stub = r#"<?php
function normal_function(string $arg): string {}
"#;

    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 4)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("string"),
        "Functions without LanguageLevelTypeAware keep native type"
    );
}

// ─── LanguageLevelTypeAware — parameter types ───────────────────────────────

#[test]
fn language_level_param_type_selects_matching_version() {
    let backend = create_test_backend();

    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

function pspell_check(
    #[LanguageLevelTypeAware(['8.1' => 'PSpell\Dictionary'], default: 'int')] $dictionary,
    string $word
): bool {}
"#;

    // PHP 8.1+ should get PSpell\Dictionary
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 4)));
    assert_eq!(functions[0].parameters.len(), 2);
    assert_eq!(
        functions[0].parameters[0].type_hint_str().as_deref(),
        Some("PSpell\\Dictionary"),
        "PHP 8.4 should select 8.1 variant for parameter"
    );

    // PHP 8.0 should get default "int"
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 0)));
    assert_eq!(
        functions[0].parameters[0].type_hint_str().as_deref(),
        Some("int"),
        "PHP 8.0 should fall back to default for parameter"
    );
}

#[test]
fn language_level_param_empty_default_keeps_native_hint() {
    let backend = create_test_backend();

    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

function filter(
    $in,
    $out,
    &$consumed,
    #[LanguageLevelTypeAware(['8.0' => 'bool'], default: '')] $closing
): int {}
"#;

    // PHP 8.0+ should get "bool"
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 0)));
    assert_eq!(
        functions[0].parameters[3].type_hint_str().as_deref(),
        Some("bool"),
        "PHP 8.0 should select the 8.0 variant"
    );

    // PHP 7.4 — empty default means no type override; native hint is None
    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(7, 4)));
    assert_eq!(
        functions[0].parameters[3].type_hint_str().as_deref(),
        None,
        "PHP 7.4 should have no type when default is empty and native is untyped"
    );
}

// ─── LanguageLevelTypeAware — method return types ───────────────────────────

#[test]
fn language_level_method_return_type() {
    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

class SplFileObject {
    #[LanguageLevelTypeAware(['8.0' => 'string|false'], default: 'string')]
    public function fgets(): string {}
}
"#;

    let classes = Backend::parse_php_versioned(stub, Some(PhpVersion::new(8, 4)));
    assert_eq!(classes.len(), 1);
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "fgets")
        .unwrap();
    assert_eq!(
        method.return_type_str().as_deref(),
        Some("string|false"),
        "PHP 8.4 should select the 8.0 variant for method return"
    );

    let classes = Backend::parse_php_versioned(stub, Some(PhpVersion::new(7, 4)));
    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "fgets")
        .unwrap();
    assert_eq!(
        method.return_type_str().as_deref(),
        Some("string"),
        "PHP 7.4 should fall back to default for method return"
    );
}

// ─── LanguageLevelTypeAware — property types ────────────────────────────────

#[test]
fn language_level_property_type() {
    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

class php_user_filter {
    #[LanguageLevelTypeAware(['8.1' => 'string'], default: '')]
    public $filtername;

    #[LanguageLevelTypeAware(['8.1' => 'mixed'], default: '')]
    public $params;

    public $stream;
}
"#;

    // PHP 8.1+ should get the typed properties
    let classes = Backend::parse_php_versioned(stub, Some(PhpVersion::new(8, 4)));
    assert_eq!(classes.len(), 1);

    let filtername = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "filtername")
        .unwrap();
    assert_eq!(
        filtername.type_hint_str().as_deref(),
        Some("string"),
        "PHP 8.4 should select 8.1 type for $filtername"
    );

    let params = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "params")
        .unwrap();
    assert_eq!(
        params.type_hint_str().as_deref(),
        Some("mixed"),
        "PHP 8.4 should select 8.1 type for $params"
    );

    let stream = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "stream")
        .unwrap();
    assert_eq!(
        stream.type_hint_str().as_deref(),
        None,
        "$stream has no LanguageLevelTypeAware and no native type"
    );

    // PHP 7.4 — empty default means no type
    let classes = Backend::parse_php_versioned(stub, Some(PhpVersion::new(7, 4)));
    let filtername = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "filtername")
        .unwrap();
    assert_eq!(
        filtername.type_hint_str().as_deref(),
        None,
        "PHP 7.4 should have no type when default is empty"
    );
}

// ─── LanguageLevelTypeAware — no version (user code) ────────────────────────

#[test]
fn language_level_no_version_keeps_native_type() {
    let backend = create_test_backend();

    // When php_version is None (user code), LanguageLevelTypeAware is ignored
    // and the native type hint is preserved.
    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

#[LanguageLevelTypeAware(["8.0" => "int"], default: "int|false")]
function sleep(int $seconds): int|false {}
"#;

    let functions = backend.parse_functions_versioned(stub, None);
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("int|false"),
        "Without a target version, native type should be kept"
    );
}

// ─── LanguageLevelTypeAware — double-quoted strings ─────────────────────────

#[test]
fn language_level_double_quoted_strings() {
    let backend = create_test_backend();

    // Some stubs use double-quoted strings in the attribute.
    let stub = r#"<?php
use JetBrains\PhpStorm\Internal\LanguageLevelTypeAware;

#[LanguageLevelTypeAware(["8.0" => "string"], default: "string|false")]
function my_func(): string|false {}
"#;

    let functions = backend.parse_functions_versioned(stub, Some(PhpVersion::new(8, 4)));
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("string"),
        "Double-quoted strings in attribute should work"
    );
}

// ─── set_php_version filters stub_constant_index ────────────────────────────

#[test]
fn set_php_version_filters_removed_constants() {
    use std::collections::HashMap;

    let removed_const_stub: &str = Box::leak(
        concat!(
            "<?php\n",
            "/**\n",
            " * @deprecated 7.1\n",
            " * @removed 7.2\n",
            " */\n",
            "define('MCRYPT_ENCRYPT', 0);\n",
        )
        .to_string()
        .into_boxed_str(),
    );

    let normal_const_stub: &str = Box::leak(
        concat!("<?php\n", "define('PHP_EOL', \"\\n\");\n")
            .to_string()
            .into_boxed_str(),
    );

    let mut constant_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    constant_stubs.insert("MCRYPT_ENCRYPT", removed_const_stub);
    constant_stubs.insert("PHP_EOL", normal_const_stub);

    let backend = Backend::new_test_with_all_stubs(HashMap::new(), HashMap::new(), constant_stubs);

    // Default version is 8.5, which is >= 7.2, so the removed constant
    // should already be filtered out after construction.
    assert!(
        !backend.stub_constant_index().contains_key("MCRYPT_ENCRYPT"),
        "MCRYPT_ENCRYPT (@removed 7.2) should be filtered for default PHP 8.5"
    );
    assert!(
        backend.stub_constant_index().contains_key("PHP_EOL"),
        "PHP_EOL should survive version filtering"
    );
    assert_eq!(backend.stub_constant_index().len(), 1);
}

#[test]
fn set_php_version_keeps_removed_constants_for_older_version() {
    // Constant removed in 8.0 — should survive when the default version
    // is overridden to 7.4 before the filtering pass.
    let removed_const_stub: &str = Box::leak(
        concat!(
            "<?php\n",
            "/**\n",
            " * @removed 8.0\n",
            " */\n",
            "define('OLD_CONST', 1);\n",
        )
        .to_string()
        .into_boxed_str(),
    );

    // Construct without the all-stubs helper to avoid the automatic
    // set_php_version(8.5) call that would filter the constant.
    let backend = Backend::new_test();
    backend
        .stub_constant_index_mut()
        .insert("OLD_CONST", removed_const_stub);

    // Target PHP 7.4 — constant removed in 8.0 should survive.
    backend.set_php_version(PhpVersion::new(7, 4));

    assert!(
        backend.stub_constant_index().contains_key("OLD_CONST"),
        "OLD_CONST (@removed 8.0) should survive when targeting PHP 7.4"
    );
}

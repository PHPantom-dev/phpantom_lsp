use crate::common::{completion_response, create_psr4_workspace, create_test_backend, open_php};
use tower_lsp::lsp_types::*;

// ─── Multi-line @return tag tests ───────────────────────────────────────────
//
// These tests verify that a complex multi-line `@return` tag on one method
// (e.g. `groupBy`) does not prevent resolution of other methods (e.g. `map`)
// on the same class.  This reproduces the real-world failure with Laravel's
// `collect([])->map()`.

/// When a class has a method with a multi-line `@return static<…>` docblock
/// (like Laravel Collection's `groupBy`), other methods on the same class
/// must still resolve correctly through a function-call chain.
#[tokio::test]
async fn test_multiline_return_does_not_break_sibling_methods() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Illuminate\\Support\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Collection.php",
                concat!(
                    "<?php\n",
                    "namespace Illuminate\\Support;\n",
                    "\n",
                    "/**\n",
                    " * @template TKey of array-key\n",
                    " * @template-covariant TValue\n",
                    " */\n",
                    "class Collection\n",
                    "{\n",
                    "    /**\n",
                    "     * @template TGroupKey of array-key\n",
                    "     *\n",
                    "     * @param  (callable(TValue, TKey): TGroupKey)|array|string  $groupBy\n",
                    "     * @param  bool  $preserveKeys\n",
                    "     * @return static<\n",
                    "     *  ($groupBy is (array|string)\n",
                    "     *      ? array-key\n",
                    "     *      : (TGroupKey is array-key ? TGroupKey : array-key)),\n",
                    "     *  static<($preserveKeys is true ? TKey : int), TValue>\n",
                    "     * >\n",
                    "     */\n",
                    "    public function groupBy($groupBy, $preserveKeys = false)\n",
                    "    {\n",
                    "    }\n",
                    "\n",
                    "    /**\n",
                    "     * @template TMapValue\n",
                    "     *\n",
                    "     * @param  callable(TValue, TKey): TMapValue  $callback\n",
                    "     * @return static<TKey, TMapValue>\n",
                    "     */\n",
                    "    public function map(callable $callback)\n",
                    "    {\n",
                    "    }\n",
                    "\n",
                    "    /**\n",
                    "     * @return static<TKey, TValue>\n",
                    "     */\n",
                    "    public function values()\n",
                    "    {\n",
                    "    }\n",
                    "}\n",
                ),
            ),
            (
                "src/helpers.php",
                concat!(
                    "<?php\n",
                    "namespace Illuminate\\Support;\n",
                    "\n",
                    "/**\n",
                    " * @param  array  $value\n",
                    " * @return \\Illuminate\\Support\\Collection\n",
                    " */\n",
                    "function collect($value = []) {\n",
                    "    return new Collection($value);\n",
                    "}\n",
                ),
            ),
        ],
    );

    // The "current" file calls collect([])->
    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Illuminate\\Support\\Collection;\n",
        "\n",
        "/**\n",
        " * @return \\Illuminate\\Support\\Collection\n",
        " */\n",
        "function collect($value = []) {\n",
        "    return new Collection($value);\n",
        "}\n",
        "\n",
        "collect([])->\n",
    );
    open_php(&backend, &uri, text).await;

    // Cursor right after `collect([])->`  on line 10
    let result = completion_response(&backend, &uri, 10, 14, None).await;
    assert!(result.is_some(), "Completion should resolve collect([])->");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"map"),
                "Should include 'map' method, got {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"groupBy"),
                "Should include 'groupBy' method, got {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"values"),
                "Should include 'values' method, got {:?}",
                method_names
            );
        }
        CompletionResponse::List(list) => {
            let method_names: Vec<&str> = list
                .items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"map"),
                "Should include 'map' method, got {:?}",
                method_names
            );
        }
    }
}

/// The multi-line `@return` type itself should be collected across lines
/// so that it resolves correctly (e.g. `static<…>` spanning multiple
/// docblock lines should still be treated as a `static` return).
#[tokio::test]
async fn test_multiline_return_type_parsed_correctly() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Builder.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Builder\n",
                "{\n",
                "    /**\n",
                "     * @return array<\n",
                "     *   string,\n",
                "     *   int\n",
                "     * >\n",
                "     */\n",
                "    public function toArray()\n",
                "    {\n",
                "    }\n",
                "\n",
                "    /**\n",
                "     * @return static\n",
                "     */\n",
                "    public function fresh()\n",
                "    {\n",
                "    }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Builder;\n",
        "\n",
        "$b = new Builder();\n",
        "$b->fresh()->\n",
    );
    open_php(&backend, &uri, text).await;

    // Cursor right after `$b->fresh()->`
    let result = completion_response(&backend, &uri, 4, 14, None).await;
    assert!(result.is_some(), "Completion should resolve $b->fresh()->");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"toArray"),
                "Should include 'toArray' method after fresh()->, got {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"fresh"),
                "Should include 'fresh' method after fresh()->, got {:?}",
                method_names
            );
        }
        CompletionResponse::List(list) => {
            let method_names: Vec<&str> = list
                .items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"toArray"),
                "Should include 'toArray' method after fresh()->, got {:?}",
                method_names
            );
        }
    }
}

/// When a `@return` tag cannot be fully parsed and there is no recoverable
/// base type (e.g. `@return <garbage`), the parser should fall back to the
/// native return type rather than producing a broken type string.
#[tokio::test]
async fn test_malformed_docblock_return_falls_back_to_native_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_fallback.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public function getValue(): string { return ''; }\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    /**\n",
        "     * Docblock with completely broken return — no base type.\n",
        "     * @return <garbage\n",
        "     */\n",
        "    public function process(): Result\n",
        "    {\n",
        "    }\n",
        "}\n",
        "\n",
        "$svc = new Service();\n",
        "$svc->process()->\n",
    );
    open_php(&backend, &uri, text).await;

    // Cursor right after `$svc->process()->` on line 16
    let result = completion_response(&backend, &uri, 16, 18, None).await;
    assert!(
        result.is_some(),
        "Completion should fall back to native Result type when docblock is broken"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"getValue"),
                "Should fall back to native type Result and show getValue(), got {:?}",
                method_names
            );
        }
        CompletionResponse::List(list) => {
            let method_names: Vec<&str> = list
                .items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"getValue"),
                "Should fall back to native type Result and show getValue(), got {:?}",
                method_names
            );
        }
    }
}

/// When a `@return` tag has an unclosed generic like `static<broken`,
/// the parser should recover the base `static` portion and resolve it
/// back to the owning class.
#[tokio::test]
async fn test_broken_static_generic_recovers_to_self() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Builder.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Builder\n",
                "{\n",
                "    /**\n",
                "     * Broken multi-line return — recovers to static.\n",
                "     * @return static<\n",
                "     */\n",
                "    public function broken()\n",
                "    {\n",
                "    }\n",
                "\n",
                "    public function build(): string\n",
                "    {\n",
                "    }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test_static_recovery.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Builder;\n",
        "\n",
        "$b = new Builder();\n",
        "$b->broken()->\n",
    );
    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 4, 15, None).await;
    assert!(
        result.is_some(),
        "Completion should recover static from static< and resolve to Builder"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"build"),
                "Should recover static and show Builder methods including build(), got {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"broken"),
                "Should recover static and show Builder methods including broken(), got {:?}",
                method_names
            );
        }
        CompletionResponse::List(list) => {
            let method_names: Vec<&str> = list
                .items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"build"),
                "Should recover static and show Builder methods including build(), got {:?}",
                method_names
            );
        }
    }
}

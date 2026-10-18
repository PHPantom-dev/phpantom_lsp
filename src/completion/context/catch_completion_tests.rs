use super::*;
use crate::completion::source::throws_analysis;

#[test]
fn test_find_method_throws_tags_with_private() {
    let content = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /** @throws ValidationException */\n",
        "    private function riskyOperation(): void {}\n",
        "}\n",
    );
    let result = throws_analysis::find_method_throws_tags(content, "riskyOperation");
    let names: Vec<String> = result.iter().map(|t| t.to_string()).collect();
    assert_eq!(
        names,
        vec!["ValidationException"],
        "Should find @throws through 'private' modifier"
    );
}

#[test]
fn test_find_method_throws_tags_with_protected_static() {
    let content = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /** @throws RuntimeException */\n",
        "    protected static function dangerousCall(): void {}\n",
        "}\n",
    );
    let result = throws_analysis::find_method_throws_tags(content, "dangerousCall");
    let names: Vec<String> = result.iter().map(|t| t.to_string()).collect();
    assert_eq!(
        names,
        vec!["RuntimeException"],
        "Should find @throws through 'protected static' modifiers"
    );
}

#[test]
fn test_find_method_throws_tags_without_modifier() {
    let content = concat!(
        "<?php\n",
        "/** @throws LogicException */\n",
        "function standalone(): void {}\n",
    );
    let result = throws_analysis::find_method_throws_tags(content, "standalone");
    let names: Vec<String> = result.iter().map(|t| t.to_string()).collect();
    assert_eq!(
        names,
        vec!["LogicException"],
        "Should find @throws on a standalone function (no modifier)"
    );
}

#[test]
fn test_propagated_throws_with_visibility_in_catch() {
    // Full file content — cursor will be inside catch()
    //                                                    v cursor (line 5, char 17)
    // Line 0: <?php
    // Line 1: class Foo {
    // Line 2:     public function doStuff(): void {
    // Line 3:         try {
    // Line 4:             $this->riskyOperation();
    // Line 5:         } catch () {}
    // Line 6:     }
    // Line 7:
    // Line 8:     /** @throws ValidationException */
    // Line 9:     private function riskyOperation(): void {}
    // Line 10: }
    let full_content = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function doStuff(): void {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch () {}\n",
        "    }\n",
        "\n",
        "    /** @throws ValidationException */\n",
        "    private function riskyOperation(): void {}\n",
        "}\n",
    );

    // Character 17 is between `(` (char 16) and `)` (char 17) on line 5
    let pos = Position {
        line: 5,
        character: 17,
    };
    let ctx = detect_catch_context(full_content, pos);
    assert!(ctx.is_some(), "Should detect catch context");
    let ctx = ctx.unwrap();
    assert!(
        ctx.suggested_types
            .contains(&"ValidationException".to_string()),
        "Should suggest ValidationException from propagated @throws on private method, got: {:?}",
        ctx.suggested_types
    );
}

#[test]
fn test_propagated_throws_with_protected_static_in_catch() {
    let full_content = concat!(
        "<?php\n",
        "class Bar {\n",
        "    public function handle(): void {\n",
        "        try {\n",
        "            $this->dangerousCall();\n",
        "        } catch () {}\n",
        "    }\n",
        "\n",
        "    /** @throws RuntimeException */\n",
        "    protected static function dangerousCall(): void {}\n",
        "}\n",
    );

    let pos = Position {
        line: 5,
        character: 17,
    };
    let ctx = detect_catch_context(full_content, pos);
    assert!(ctx.is_some(), "Should detect catch context");
    let ctx = ctx.unwrap();
    assert!(
        ctx.suggested_types
            .contains(&"RuntimeException".to_string()),
        "Should suggest RuntimeException through protected static modifier, got: {:?}",
        ctx.suggested_types
    );
}

#[test]
fn test_find_inline_throws_annotations_in_catch() {
    let body = r#"
        /** @throws ModelNotFoundException */
        $model = SomeService::find($id);
        /** @throws \App\Exceptions\AuthException */
        $auth = doSomething();
    "#;
    let result = throws_analysis::find_inline_throws_annotations(body);
    // Raw names are returned; short-name extraction happens in detect_catch_context
    let names: Vec<String> = result.iter().map(|t| t.to_string()).collect();
    assert_eq!(
        names,
        vec!["ModelNotFoundException", "App\\Exceptions\\AuthException"]
    );
}

#[test]
fn test_find_inline_throws_multiline_docblock_in_catch() {
    let body = r#"
        /**
         * @throws RuntimeException
         */
        doStuff();
    "#;
    let result = throws_analysis::find_inline_throws_annotations(body);
    let names: Vec<String> = result.iter().map(|t| t.to_string()).collect();
    assert_eq!(names, vec!["RuntimeException"]);
}

#[test]
fn test_parse_catch_paren_content_empty() {
    let (partial, already) = parse_catch_paren_content("");
    assert_eq!(partial, "");
    assert!(already.is_empty());
}

#[test]
fn test_parse_catch_paren_content_partial() {
    let (partial, already) = parse_catch_paren_content("IOEx");
    assert_eq!(partial, "IOEx");
    assert!(already.is_empty());
}

#[test]
fn test_parse_catch_paren_content_multi_catch() {
    let (partial, already) = parse_catch_paren_content("IOException | ");
    assert_eq!(partial, "");
    assert_eq!(already, vec!["IOException"]);
}

#[test]
fn test_parse_catch_paren_content_multi_catch_with_partial() {
    let (partial, already) = parse_catch_paren_content("IOException | Time");
    assert_eq!(partial, "Time");
    assert_eq!(already, vec!["IOException"]);
}

#[test]
fn test_parse_catch_paren_content_three_types() {
    let (partial, already) = parse_catch_paren_content("IOException | TimeoutException | ");
    assert_eq!(partial, "");
    assert_eq!(already, vec!["IOException", "TimeoutException"]);
}

#[test]
fn test_detect_catch_context_always_includes_throwable() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    throw new RuntimeException('error');\n",
        "} catch (",
    );
    let pos = Position {
        line: 3,
        character: 10,
    };
    let ctx = detect_catch_context(content, pos).unwrap();
    assert!(
        ctx.suggested_types.contains(&"\\Throwable".to_string()),
        "Should always include \\Throwable, got: {:?}",
        ctx.suggested_types
    );
    assert!(ctx.has_specific_types);
}

#[test]
fn test_detect_catch_context_no_specific_types_sets_flag() {
    let content = concat!("<?php\n", "try {\n", "    doSomething();\n", "} catch (",);
    let pos = Position {
        line: 3,
        character: 10,
    };
    let ctx = detect_catch_context(content, pos).unwrap();
    assert!(
        !ctx.has_specific_types,
        "Should have no specific types when try block has no throws"
    );
    // Throwable is still offered
    assert!(ctx.suggested_types.contains(&"\\Throwable".to_string()));
}

#[test]
fn test_detect_catch_context_simple() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    throw new RuntimeException('error');\n",
        "} catch (",
    );
    let pos = Position {
        line: 3,
        character: 10,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_some(), "Should detect catch context");
    let ctx = ctx.unwrap();
    assert!(
        ctx.suggested_types
            .contains(&"RuntimeException".to_string()),
        "Should suggest RuntimeException, got: {:?}",
        ctx.suggested_types
    );
}

#[test]
fn test_detect_catch_context_with_inline_throws() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    /** @throws ModelNotFoundException */\n",
        "    $model = SomeService::find($id);\n",
        "} catch (",
    );
    let pos = Position {
        line: 4,
        character: 10,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_some(), "Should detect catch context");
    let ctx = ctx.unwrap();
    assert!(
        ctx.suggested_types
            .contains(&"ModelNotFoundException".to_string()),
        "Should suggest ModelNotFoundException from inline @throws, got: {:?}",
        ctx.suggested_types
    );
}

#[test]
fn test_detect_catch_context_multi_throw() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    throw new IOException('io');\n",
        "    throw new TimeoutException('timeout');\n",
        "} catch (",
    );
    let pos = Position {
        line: 4,
        character: 10,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert!(ctx.suggested_types.contains(&"IOException".to_string()));
    assert!(
        ctx.suggested_types
            .contains(&"TimeoutException".to_string())
    );
}

#[test]
fn test_detect_catch_context_second_catch() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    throw new IOException('io');\n",
        "    throw new TimeoutException('timeout');\n",
        "} catch (IOException $e) {\n",
        "    // handled\n",
        "} catch (",
    );
    let pos = Position {
        line: 6,
        character: 10,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_some(), "Should detect second catch context");
    let ctx = ctx.unwrap();
    // Both types are in the try block
    assert!(ctx.suggested_types.contains(&"IOException".to_string()));
    assert!(
        ctx.suggested_types
            .contains(&"TimeoutException".to_string())
    );
}

#[test]
fn test_detect_catch_context_partial_typed() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    throw new RuntimeException('error');\n",
        "    throw new InvalidArgumentException('bad');\n",
        "} catch (Run",
    );
    let pos = Position {
        line: 4,
        character: 13,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.partial, "Run");
    // Both are suggested (filtering happens in build_catch_completions)
    assert!(
        ctx.suggested_types
            .contains(&"RuntimeException".to_string())
    );
    assert!(
        ctx.suggested_types
            .contains(&"InvalidArgumentException".to_string())
    );
}

#[test]
fn test_detect_catch_context_not_catch() {
    let content = concat!("<?php\n", "function foo(",);
    let pos = Position {
        line: 1,
        character: 14,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_none(), "Should not detect catch context in function");
}

#[test]
fn test_build_catch_completions_filters_by_partial() {
    let ctx = CatchContext {
        partial: "Run".to_string(),
        suggested_types: vec![
            "RuntimeException".to_string(),
            "InvalidArgumentException".to_string(),
        ],
        has_specific_types: true,
    };
    let empty_use_map = std::collections::HashMap::new();
    let no_namespace = None;
    let items = build_catch_completions(&ctx, &empty_use_map, &no_namespace);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "RuntimeException");
}

#[test]
fn test_build_catch_completions_empty_partial_shows_all() {
    let ctx = CatchContext {
        partial: String::new(),
        suggested_types: vec![
            "RuntimeException".to_string(),
            "InvalidArgumentException".to_string(),
        ],
        has_specific_types: true,
    };
    let empty_use_map = std::collections::HashMap::new();
    let no_namespace = None;
    let items = build_catch_completions(&ctx, &empty_use_map, &no_namespace);
    assert_eq!(items.len(), 2);
}

#[test]
fn test_detect_catch_context_multi_catch_pipe() {
    let content = concat!(
        "<?php\n",
        "try {\n",
        "    throw new IOException('io');\n",
        "    throw new TimeoutException('timeout');\n",
        "    throw new RuntimeException('rt');\n",
        "} catch (IOException | ",
    );
    let pos = Position {
        line: 5,
        character: 23,
    };
    let ctx = detect_catch_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    // IOException should be filtered out since it's already listed
    assert!(
        !ctx.suggested_types.contains(&"IOException".to_string()),
        "IOException should be filtered out"
    );
    assert!(
        ctx.suggested_types
            .contains(&"TimeoutException".to_string())
    );
    assert!(
        ctx.suggested_types
            .contains(&"RuntimeException".to_string())
    );
}

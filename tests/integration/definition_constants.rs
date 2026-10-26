use crate::common::{create_psr4_workspace, create_test_backend, goto_definition_at, open_php};
use tower_lsp_server::gen_lsp_types::*;

// ─── DefineInfo value extraction correctness ────────────────────────────────

/// The parser should extract exactly the value expression text, with no
/// trailing `)` or `;` from the surrounding `define(...)` call.
#[tokio::test]
async fn test_define_value_no_trailing_paren() {
    let backend = create_test_backend();
    let uri = "file:///define_value.php";
    let content = "<?php\ndefine('REPORT_ALL', 255);\n";
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();
    let info = dmap
        .get("REPORT_ALL")
        .expect("REPORT_ALL should be in global_defines");
    assert_eq!(
        info.value.as_deref(),
        Some("255"),
        "value should be '255', not '255)' or '255);'"
    );
}

/// `const` declarations should also extract clean values.
#[tokio::test]
async fn test_const_value_no_trailing_semicolon() {
    let backend = create_test_backend();
    let uri = "file:///const_value.php";
    let content = "<?php\nconst LIMIT = 100;\n";
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();
    let info = dmap
        .get("LIMIT")
        .expect("LIMIT should be in global_defines");
    assert_eq!(
        info.value.as_deref(),
        Some("100"),
        "value should be '100', not '100;'"
    );
}

/// String values in define() should be extracted with their quotes intact.
#[tokio::test]
async fn test_define_string_value() {
    let backend = create_test_backend();
    let uri = "file:///define_str.php";
    let content = "<?php\ndefine('APP_NAME', 'PHPantom');\n";
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();
    let info = dmap
        .get("APP_NAME")
        .expect("APP_NAME should be in global_defines");
    assert_eq!(
        info.value.as_deref(),
        Some("'PHPantom'"),
        "string value should include quotes but no trailing paren"
    );
}

/// define() with no second argument should have value = None.
#[tokio::test]
async fn test_define_no_value() {
    let backend = create_test_backend();
    let uri = "file:///define_noval.php";
    // This is technically invalid PHP but stubs sometimes have it.
    let content = "<?php\ndefine('NO_VAL');\n";
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();
    // It may or may not appear in global_defines; if it does, value should be None.
    if let Some(info) = dmap.get("NO_VAL") {
        assert_eq!(
            info.value, None,
            "define with no second arg should have no value"
        );
    }
}

// ─── DefineInfo value extraction from multi-constant stub files ─────────────

/// Reproduces the scenario where a stub file contains multiple constants
/// defined via `define()` back-to-back, as in the mysqli stubs.
/// The value for `MYSQLI_REPORT_ALL` should be `255`, not `255)`.
#[tokio::test]
async fn test_define_value_multi_constant_stub_file() {
    let backend = create_test_backend();
    let uri = "file:///mysqli_constants.php";
    let content = concat!(
        "<?php\n",
        "define('MYSQLI_REPORT_ALL', 255);\n",
        "/**\n",
        " * Turns reporting off.\n",
        " */\n",
        "define('MYSQLI_REPORT_OFF', 0);\n",
    );
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();

    let all = dmap
        .get("MYSQLI_REPORT_ALL")
        .expect("MYSQLI_REPORT_ALL should exist");
    assert_eq!(
        all.value.as_deref(),
        Some("255"),
        "value should be '255', not '255)'"
    );

    let off = dmap
        .get("MYSQLI_REPORT_OFF")
        .expect("MYSQLI_REPORT_OFF should exist");
    assert_eq!(off.value.as_deref(), Some("0"), "value should be '0'");
}

/// Verify that `const` declarations in stubs also extract clean values
/// without trailing semicolons or parens.
#[tokio::test]
async fn test_const_declaration_value_clean() {
    let backend = create_test_backend();
    let uri = "file:///consts.php";
    let content = "<?php\nconst LIMIT = 100;\nconst NAME = 'hello';\n";
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();

    let limit = dmap.get("LIMIT").expect("LIMIT should exist");
    assert_eq!(limit.value.as_deref(), Some("100"));

    let name = dmap.get("NAME").expect("NAME should exist");
    assert_eq!(name.value.as_deref(), Some("'hello'"));
}

// ─── Same-File Constant Go-to-Definition ────────────────────────────────────

/// Clicking on a constant name used in an expression should jump to the
/// `define('CONSTANT_NAME', ...)` call in the same file.
#[tokio::test]
async fn test_goto_definition_constant_same_file() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///constants.php").unwrap();
    let text = concat!(
        "<?php\n",                           // 0
        "define('APP_VERSION', '1.0.0');\n", // 1
        "define('APP_NAME', 'PHPantom');\n", // 2
        "\n",                                // 3
        "echo APP_VERSION;\n",               // 4
    );

    open_php(&backend, &uri, text).await;

    // Click on "APP_VERSION" on line 4
    let result = goto_definition_at(&backend, &uri, 4, 7).await;
    assert!(
        result.is_some(),
        "Should resolve constant APP_VERSION to its define() call"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "APP_VERSION is defined on line 1"
            );
            assert_eq!(
                location.range.start.character, 8,
                "the target is the name inside the quotes, past `define('`"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on a different constant in the same file should jump to the
/// correct `define()` call.
#[tokio::test]
async fn test_goto_definition_constant_same_file_second_constant() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///constants2.php").unwrap();
    let text = concat!(
        "<?php\n",                           // 0
        "define('APP_VERSION', '1.0.0');\n", // 1
        "define('APP_NAME', 'PHPantom');\n", // 2
        "\n",                                // 3
        "echo APP_NAME;\n",                  // 4
    );

    open_php(&backend, &uri, text).await;

    // Click on "APP_NAME" on line 4
    let result = goto_definition_at(&backend, &uri, 4, 7).await;
    assert!(
        result.is_some(),
        "Should resolve constant APP_NAME to its define() call"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "APP_NAME is defined on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Constants defined with double-quoted strings should also be resolved.
#[tokio::test]
async fn test_goto_definition_constant_double_quoted() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_dq.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "define(\"DB_HOST\", 'localhost');\n", // 1
        "\n",                                  // 2
        "echo DB_HOST;\n",                     // 3
    );

    open_php(&backend, &uri, text).await;

    // Click on "DB_HOST" on line 3
    let result = goto_definition_at(&backend, &uri, 3, 7).await;
    assert!(
        result.is_some(),
        "Should resolve double-quoted define constant"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 1, "DB_HOST is defined on line 1");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When `define()` is indented (e.g. inside a class method or if block),
/// the position should point at the correct column.
#[tokio::test]
async fn test_goto_definition_constant_indented_define() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_indent.php").unwrap();
    let text = concat!(
        "<?php\n",                           // 0
        "if (!defined('DEBUG_MODE')) {\n",   // 1
        "    define('DEBUG_MODE', true);\n", // 2
        "}\n",                               // 3
        "\n",                                // 4
        "echo DEBUG_MODE;\n",                // 5
    );

    open_php(&backend, &uri, text).await;

    // Click on "DEBUG_MODE" on line 5
    let result = goto_definition_at(&backend, &uri, 5, 7).await;
    assert!(result.is_some(), "Should resolve indented define constant");

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "DEBUG_MODE is defined on line 2"
            );
            assert_eq!(
                location.range.start.character, 12,
                "the target is the name inside the quotes of a define() indented by 4 spaces"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Cross-File Constant Go-to-Definition ───────────────────────────────────

/// Clicking on a constant defined in another file (via define()) should
/// jump to that file's define() call.
#[tokio::test]
async fn test_goto_definition_constant_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/constants.php",
                concat!(
                    "<?php\n",                          // 0
                    "define('MAX_RETRIES', 3);\n",      // 1
                    "define('DEFAULT_TIMEOUT', 30);\n", // 2
                ),
            ),
            (
                "src/Service.php",
                concat!(
                    "<?php\n",                                   // 0
                    "namespace App;\n",                          // 1
                    "class Service {\n",                         // 2
                    "    public function getTimeout(): int {\n", // 3
                    "        return DEFAULT_TIMEOUT;\n",         // 4
                    "    }\n",                                   // 5
                    "}\n",                                       // 6
                ),
            ),
        ],
    );

    // Open the constants file first so define() calls are registered
    let constants_path = _dir.path().join("src/constants.php");
    let constants_uri = Uri::from_file_path(&constants_path).unwrap();
    let constants_text = std::fs::read_to_string(&constants_path).unwrap();

    open_php(&backend, &constants_uri, &constants_text).await;

    // Open the service file
    let service_path = _dir.path().join("src/Service.php");
    let service_uri = Uri::from_file_path(&service_path).unwrap();
    let service_text = std::fs::read_to_string(&service_path).unwrap();

    open_php(&backend, &service_uri, &service_text).await;

    // Click on "DEFAULT_TIMEOUT" on line 4 in Service.php
    let result = goto_definition_at(&backend, &service_uri, 4, 18).await;
    assert!(
        result.is_some(),
        "Should resolve DEFAULT_TIMEOUT to its define() in constants.php"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, constants_uri, "Should jump to constants.php");
            assert_eq!(
                location.range.start.line, 2,
                "DEFAULT_TIMEOUT is defined on line 2 of constants.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cross-file: jumping to the first constant in a multi-constant file.
#[tokio::test]
async fn test_goto_definition_constant_cross_file_first_constant() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/constants.php",
                concat!(
                    "<?php\n",                          // 0
                    "define('MAX_RETRIES', 3);\n",      // 1
                    "define('DEFAULT_TIMEOUT', 30);\n", // 2
                ),
            ),
            (
                "src/Worker.php",
                concat!(
                    "<?php\n",                                           // 0
                    "namespace App;\n",                                  // 1
                    "class Worker {\n",                                  // 2
                    "    public function run(): void {\n",               // 3
                    "        for ($i = 0; $i < MAX_RETRIES; $i++) {}\n", // 4
                    "    }\n",                                           // 5
                    "}\n",                                               // 6
                ),
            ),
        ],
    );

    // Open the constants file first
    let constants_path = _dir.path().join("src/constants.php");
    let constants_uri = Uri::from_file_path(&constants_path).unwrap();
    let constants_text = std::fs::read_to_string(&constants_path).unwrap();

    open_php(&backend, &constants_uri, &constants_text).await;

    // Open the worker file
    let worker_path = _dir.path().join("src/Worker.php");
    let worker_uri = Uri::from_file_path(&worker_path).unwrap();
    let worker_text = std::fs::read_to_string(&worker_path).unwrap();

    open_php(&backend, &worker_uri, &worker_text).await;

    // Click on "MAX_RETRIES" on line 4 in Worker.php
    let result = goto_definition_at(&backend, &worker_uri, 4, 35).await;
    assert!(
        result.is_some(),
        "Should resolve MAX_RETRIES to its define() in constants.php"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, constants_uri, "Should jump to constants.php");
            assert_eq!(
                location.range.start.line, 1,
                "MAX_RETRIES is defined on line 1 of constants.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Constants used in various contexts ─────────────────────────────────────

/// Constants used in function arguments should be resolvable.
#[tokio::test]
async fn test_goto_definition_constant_in_function_arg() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_arg.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "define('LOG_LEVEL', 'info');\n",              // 1
        "\n",                                          // 2
        "function setLevel(string $level): void {}\n", // 3
        "setLevel(LOG_LEVEL);\n",                      // 4
    );

    open_php(&backend, &uri, text).await;

    // Click on "LOG_LEVEL" on line 4, inside the function argument
    let result = goto_definition_at(&backend, &uri, 4, 12).await;
    assert!(
        result.is_some(),
        "Should resolve LOG_LEVEL used as function argument"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "LOG_LEVEL is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Constants used in class method bodies should be resolvable.
#[tokio::test]
async fn test_goto_definition_constant_inside_class() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_class.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "define('BASE_URL', 'https://example.com');\n", // 1
        "\n",                                           // 2
        "class ApiClient {\n",                          // 3
        "    public function getUrl(): string {\n",     // 4
        "        return BASE_URL . '/api';\n",          // 5
        "    }\n",                                      // 6
        "}\n",                                          // 7
    );

    open_php(&backend, &uri, text).await;

    // Click on "BASE_URL" on line 5
    let result = goto_definition_at(&backend, &uri, 5, 18).await;
    assert!(
        result.is_some(),
        "Should resolve BASE_URL used inside a class method"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "BASE_URL is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// An unknown constant (not in global_defines or stubs) should return None.
#[tokio::test]
async fn test_goto_definition_constant_unknown_returns_none() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_unknown.php").unwrap();
    let text = concat!(
        "<?php\n",                  // 0
        "echo UNKNOWN_CONSTANT;\n", // 1
    );

    open_php(&backend, &uri, text).await;

    // Click on "UNKNOWN_CONSTANT" on line 1
    let result = goto_definition_at(&backend, &uri, 1, 10).await;
    assert!(
        result.is_none(),
        "Unknown constant should not resolve to any definition"
    );
}

/// Constants used in array index positions should be resolvable.
#[tokio::test]
async fn test_goto_definition_constant_in_array_index() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_array.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "define('KEY_NAME', 'name');\n",  // 1
        "\n",                             // 2
        "$data = ['name' => 'Alice'];\n", // 3
        "echo $data[KEY_NAME];\n",        // 4
    );

    open_php(&backend, &uri, text).await;

    // Click on "KEY_NAME" on line 4, inside the array brackets
    let result = goto_definition_at(&backend, &uri, 4, 14).await;
    assert!(
        result.is_some(),
        "Should resolve KEY_NAME used as array index"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "KEY_NAME is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Multiple define() calls in a file — each constant should resolve to
/// its own define() call, not the first one.
#[tokio::test]
async fn test_goto_definition_constant_multiple_defines() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///multi_const.php").unwrap();
    let text = concat!(
        "<?php\n",                // 0
        "define('FIRST', 1);\n",  // 1
        "define('SECOND', 2);\n", // 2
        "define('THIRD', 3);\n",  // 3
        "\n",                     // 4
        "echo THIRD;\n",          // 5
    );

    open_php(&backend, &uri, text).await;

    // Click on "THIRD" on line 5
    let result = goto_definition_at(&backend, &uri, 5, 7).await;
    assert!(
        result.is_some(),
        "Should resolve THIRD to its specific define() call"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "THIRD is defined on line 3, not line 1 or 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Constants with spaces after the opening paren: `define( 'NAME', value )`
/// should still be found.
#[tokio::test]
async fn test_goto_definition_constant_spaces_in_define() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_spaces.php").unwrap();
    let text = concat!(
        "<?php\n",                         // 0
        "define( 'SPACED_CONST', 42 );\n", // 1
        "\n",                              // 2
        "echo SPACED_CONST;\n",            // 3
    );

    open_php(&backend, &uri, text).await;

    // Click on "SPACED_CONST" on line 3
    let result = goto_definition_at(&backend, &uri, 3, 7).await;
    assert!(
        result.is_some(),
        "Should resolve define() with spaces after paren"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "SPACED_CONST is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Constant used in comparison / switch ───────────────────────────────────

/// Constant used on the right side of a comparison should be resolvable.
#[tokio::test]
async fn test_goto_definition_constant_in_comparison() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_cmp.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "define('STATUS_ACTIVE', 1);\n",            // 1
        "define('STATUS_INACTIVE', 0);\n",          // 2
        "\n",                                       // 3
        "function isActive(int $status): bool {\n", // 4
        "    return $status === STATUS_ACTIVE;\n",  // 5
        "}\n",                                      // 6
    );

    open_php(&backend, &uri, text).await;

    // Click on "STATUS_ACTIVE" on line 5
    let result = goto_definition_at(&backend, &uri, 5, 27).await;
    assert!(
        result.is_some(),
        "Should resolve STATUS_ACTIVE used in comparison"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "STATUS_ACTIVE is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── define() constant inside class method ──────────────────────────────────

/// When `define('CONST', ...)` and `echo CONST` are both inside a class
/// method, GTD on the usage should still jump to the `define()` call,
/// and GTD on the constant name inside the `define()` call should NOT
/// jump to itself.
#[tokio::test]
async fn test_goto_definition_constant_inside_class_method() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_in_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // 0
        "class Demo {\n",                            // 1
        "    public function run(): void {\n",       // 2
        "        define('APP_VERSION', '1.0.0');\n", // 3
        "        echo APP_VERSION;\n",               // 4
        "    }\n",                                   // 5
        "}\n",                                       // 6
    );

    open_php(&backend, &uri, text).await;

    // Click on "APP_VERSION" in `echo APP_VERSION;` on line 4
    let result = goto_definition_at(&backend, &uri, 4, 15).await;
    assert!(
        result.is_some(),
        "GTD on APP_VERSION usage inside class method should jump to define() call"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "define('APP_VERSION', ...) is on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Same as above but inside a namespace block, matching the example.php
/// scenario where `Demo\APP_VERSION` could confuse namespace-qualified
/// resolution.
#[tokio::test]
async fn test_goto_definition_constant_inside_namespace_class_method() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///const_ns_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "namespace Demo {\n",                            // 1
        "    class GtdDemo {\n",                         // 2
        "        public function demo(): void {\n",      // 3
        "            define('APP_VERSION', '1.0.0');\n", // 4
        "            echo APP_VERSION;\n",               // 5
        "        }\n",                                   // 6
        "    }\n",                                       // 7
        "}\n",                                           // 8
    );

    open_php(&backend, &uri, text).await;

    // Click on "APP_VERSION" in `echo APP_VERSION;` on line 5
    let result = goto_definition_at(&backend, &uri, 5, 20).await;
    assert!(
        result.is_some(),
        "GTD on APP_VERSION usage in namespace class method should jump to define() call"
    );

    match result.unwrap() {
        DefinitionResponse::Definition(Definition::Location(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "define('APP_VERSION', ...) is on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// A namespaced `const` is indexed under its fully-qualified name, and
/// every spelling of a reference to it navigates to the declaration.
#[tokio::test]
async fn test_goto_definition_namespaced_constant_every_spelling() {
    let text = concat!(
        "<?php\n",                          // 0
        "namespace App\\Config;\n",         // 1
        "\n",                               // 2
        "const GRADES = ['a'];\n",          // 3
        "\n",                               // 4
        "namespace App;\n",                 // 5
        "\n",                               // 6
        "use App\\Config;\n",               // 7
        "use const App\\Config\\GRADES;\n", // 8
        "\n",                               // 9
        "$a = GRADES;\n",                   // 10
        "$b = Config\\GRADES;\n",           // 11
        "$c = \\App\\Config\\GRADES;\n",    // 12
    );

    for (line, character, spelling) in [
        (10, 6, "GRADES"),
        (11, 14, "Config\\GRADES"),
        (12, 19, "\\App\\Config\\GRADES"),
    ] {
        let backend = create_test_backend();
        let uri = Uri::parse("file:///ns_const.php").unwrap();
        open_php(&backend, &uri, text).await;

        let result = goto_definition_at(&backend, &uri, line, character).await;

        match result {
            Some(DefinitionResponse::Definition(Definition::Location(location))) => assert_eq!(
                location.range.start.line, 3,
                "`{spelling}` should land on the declaration"
            ),
            other => panic!("`{spelling}` did not resolve: {other:?}"),
        }
    }
}

/// The declaration is indexed fully-qualified, so a bare name that no
/// namespace declares still finds the global constant.
#[tokio::test]
async fn test_namespaced_const_is_indexed_fully_qualified() {
    let backend = create_test_backend();
    let uri = "file:///ns_const_index.php";
    let content =
        "<?php\nnamespace App\\Config;\n\nconst GRADES = ['a'];\ndefine('APP_KEY', 'k');\n";
    backend.update_ast(uri, content);

    let dmap = backend.global_defines().read();
    assert!(
        dmap.contains_key("App\\Config\\GRADES"),
        "a namespaced const is indexed fully-qualified; got {:?}",
        dmap.keys().collect::<Vec<_>>()
    );
    assert!(
        !dmap.contains_key("GRADES"),
        "and not additionally under its short name"
    );
    assert!(
        dmap.contains_key("APP_KEY"),
        "a define() names a global constant wherever it sits; got {:?}",
        dmap.keys().collect::<Vec<_>>()
    );
}

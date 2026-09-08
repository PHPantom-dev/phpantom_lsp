#![allow(deprecated)] // tests for text-search helpers that are now deprecated

//! Go-to-definition on type hints in declarations.
//!
//! Tests that clicking on a class/interface/enum name used as a type hint
//! in parameter declarations, return types, property types, catch blocks,
//! and docblock annotations navigates to that type's definition.
//!
//! This covers todo item #22 (type hints in code) and #23 (docblock types).

use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

/// Send a go-to-definition request and return the result.
async fn goto_definition(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<GotoDefinitionResponse> {
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    backend.goto_definition(params).await.unwrap()
}

/// Assert that a definition response points to a given URI and line.
fn assert_location(response: GotoDefinitionResponse, expected_uri: &Url, expected_line: u32) {
    match response {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                &location.uri, expected_uri,
                "Expected URI {:?}, got {:?}",
                expected_uri, location.uri
            );
            assert_eq!(
                location.range.start.line, expected_line,
                "Expected line {}, got {}",
                expected_line, location.range.start.line
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Assert that a definition response points to a file path ending with the given suffix.
fn assert_location_path_ends_with(
    response: GotoDefinitionResponse,
    path_suffix: &str,
    expected_line: u32,
) {
    match response {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with(path_suffix),
                "Expected path ending with {:?}, got {:?}",
                path_suffix,
                path
            );
            assert_eq!(
                location.range.start.line, expected_line,
                "Expected line {}, got {}",
                expected_line, location.range.start.line
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  Parameter type hints — clicking on the TYPE NAME (not $variable)
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on `Request` in `function handle(Request $req)` should jump
/// to the `Request` class definition in the same file.
#[tokio::test]
async fn test_goto_definition_parameter_type_hint_same_file() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class Request {\n",                            // 1
        "    public function input(): string {}\n",     // 2
        "}\n",                                          // 3
        "class Controller {\n",                         // 4
        "    public function handle(Request $req) {\n", // 5
        "    }\n",                                      // 6
        "}\n",                                          // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Request" on line 5 (the type hint, not $req)
    let result = goto_definition(&backend, &uri, 5, 31).await;
    assert!(result.is_some(), "Should resolve parameter type hint");
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on `Request` in a nullable parameter `?Request $req`.
#[tokio::test]
async fn test_goto_definition_nullable_parameter_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "class Request {\n",                             // 1
        "    public function input(): string {}\n",      // 2
        "}\n",                                           // 3
        "class Controller {\n",                          // 4
        "    public function handle(?Request $req) {\n", // 5
        "    }\n",                                       // 6
        "}\n",                                           // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Request" in "?Request" on line 5 (after the ?)
    let result = goto_definition(&backend, &uri, 5, 33).await;
    assert!(
        result.is_some(),
        "Should resolve nullable parameter type hint"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on the first type in a union parameter `Reader|Stream $input`.
#[tokio::test]
async fn test_goto_definition_union_parameter_first_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                     // 0
        "class Reader { public function read(): void {} }\n",          // 1
        "class Stream { public function consume(): void {} }\n",       // 2
        "class App {\n",                                               // 3
        "    public function process(Reader|Stream $input): void {\n", // 4
        "    }\n",                                                     // 5
        "}\n",                                                         // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Reader" in "Reader|Stream" on line 4
    let result = goto_definition(&backend, &uri, 4, 32).await;
    assert!(
        result.is_some(),
        "Should resolve first type in union parameter"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on the second type in a union parameter `Reader|Stream $input`.
#[tokio::test]
async fn test_goto_definition_union_parameter_second_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                     // 0
        "class Reader { public function read(): void {} }\n",          // 1
        "class Stream { public function consume(): void {} }\n",       // 2
        "class App {\n",                                               // 3
        "    public function process(Reader|Stream $input): void {\n", // 4
        "    }\n",                                                     // 5
        "}\n",                                                         // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Stream" in "Reader|Stream" on line 4
    let result = goto_definition(&backend, &uri, 4, 40).await;
    assert!(
        result.is_some(),
        "Should resolve second type in union parameter"
    );
    assert_location(result.unwrap(), &uri, 2);
}

/// When a comment contains `class AdminUser`, clicking on `AdminUser` in a
/// type hint should jump to the real class definition, not the comment.
#[tokio::test]
async fn test_goto_definition_type_hint_skips_class_in_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // 0
        "class User { public function getEmail(): string {} }\n",           // 1
        "// class AdminUser extends Model implements Renderable\n",         // 2
        "class AdminUser { public function grantPermission(): void {} }\n", // 3
        "function handle(User|AdminUser $u): void {}\n",                    // 4
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "AdminUser" in "User|AdminUser" on line 4
    // "function handle(User|AdminUser $u): void {}"
    //  f=0..e=7 ' '=8 h=9..e=14 '('=15 U=16..r=19 '|'=20 A=21
    let result = goto_definition(&backend, &uri, 4, 21).await;
    assert!(
        result.is_some(),
        "Should resolve AdminUser from union type hint"
    );
    assert_location(
        result.unwrap(),
        &uri,
        3, // real class on line 3, NOT the comment on line 2
    );
}

/// Clicking on a type in an intersection parameter `(Foo&Bar) $x`.
#[tokio::test]
async fn test_goto_definition_intersection_parameter_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // 0
        "interface Countable { public function count(): int; }\n",          // 1
        "interface Stringable { public function __toString(): string; }\n", // 2
        "class App {\n",                                                    // 3
        "    public function handle(Countable&Stringable $item): void {\n", // 4
        "    }\n",                                                          // 5
        "}\n",                                                              // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Countable" in "Countable&Stringable" on line 4
    let result = goto_definition(&backend, &uri, 4, 31).await;
    assert!(
        result.is_some(),
        "Should resolve first type in intersection parameter"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Stringable" in "Countable&Stringable" on line 4
    let result2 = goto_definition(&backend, &uri, 4, 42).await;
    assert!(
        result2.is_some(),
        "Should resolve second type in intersection parameter"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Return type hints
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on the return type `Response` in `: Response`.
#[tokio::test]
async fn test_goto_definition_return_type_hint_same_file() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Response { public function send(): void {} }\n", // 1
        "class Controller {\n",                                 // 2
        "    public function handle(): Response {\n",           // 3
        "        return new Response();\n",                     // 4
        "    }\n",                                              // 5
        "}\n",                                                  // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Response" in the return type on line 3
    let result = goto_definition(&backend, &uri, 3, 35).await;
    assert!(result.is_some(), "Should resolve return type hint");
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a nullable return type `?Response`.
#[tokio::test]
async fn test_goto_definition_nullable_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Response { public function send(): void {} }\n", // 1
        "class Controller {\n",                                 // 2
        "    public function handle(): ?Response {\n",          // 3
        "        return new Response();\n",                     // 4
        "    }\n",                                              // 5
        "}\n",                                                  // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Response" in "?Response" on line 3
    let result = goto_definition(&backend, &uri, 3, 36).await;
    assert!(result.is_some(), "Should resolve nullable return type hint");
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a union return type `Response|Error`.
#[tokio::test]
async fn test_goto_definition_union_return_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class Response { public function send(): void {} }\n",      // 1
        "class Error { public function getMessage(): string {} }\n", // 2
        "class Controller {\n",                                      // 3
        "    public function handle(): Response|Error {\n",          // 4
        "        return new Response();\n",                          // 5
        "    }\n",                                                   // 6
        "}\n",                                                       // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Response" in "Response|Error" on line 4
    let result = goto_definition(&backend, &uri, 4, 35).await;
    assert!(
        result.is_some(),
        "Should resolve first type in union return type"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Error" in "Response|Error" on line 4
    let result2 = goto_definition(&backend, &uri, 4, 44).await;
    assert!(
        result2.is_some(),
        "Should resolve second type in union return type"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Property type hints
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a property type hint `private UserRepository $repo`.
#[tokio::test]
async fn test_goto_definition_property_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class UserRepository {\n",                     // 1
        "    public function find(int $id): void {}\n", // 2
        "}\n",                                          // 3
        "class Service {\n",                            // 4
        "    private UserRepository $repo;\n",          // 5
        "}\n",                                          // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "UserRepository" on line 5
    let result = goto_definition(&backend, &uri, 5, 16).await;
    assert!(result.is_some(), "Should resolve property type hint");
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a nullable property type hint `private ?Logger $logger`.
#[tokio::test]
async fn test_goto_definition_nullable_property_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "class Logger {\n",                     // 1
        "    public function log(): void {}\n", // 2
        "}\n",                                  // 3
        "class Service {\n",                    // 4
        "    private ?Logger $logger;\n",       // 5
        "}\n",                                  // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Logger" in "?Logger" on line 5
    let result = goto_definition(&backend, &uri, 5, 15).await;
    assert!(
        result.is_some(),
        "Should resolve nullable property type hint"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a union property type hint `HtmlString|string $content`.
#[tokio::test]
async fn test_goto_definition_union_property_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // 0
        "class HtmlString {\n",                      // 1
        "    public function toHtml(): string {}\n", // 2
        "}\n",                                       // 3
        "class Widget {\n",                          // 4
        "    public HtmlString|string $content;\n",  // 5
        "}\n",                                       // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "HtmlString" in "HtmlString|string" on line 5
    let result = goto_definition(&backend, &uri, 5, 16).await;
    assert!(
        result.is_some(),
        "Should resolve first type in union property"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Promoted constructor parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a promoted constructor parameter type hint.
#[tokio::test]
async fn test_goto_definition_promoted_parameter_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "class Config { public function get(): mixed {} }\n", // 1
        "class Service {\n",                                  // 2
        "    public function __construct(\n",                 // 3
        "        private readonly Config $config,\n",         // 4
        "    ) {}\n",                                         // 5
        "}\n",                                                // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Config" on line 4
    let result = goto_definition(&backend, &uri, 4, 29).await;
    assert!(
        result.is_some(),
        "Should resolve promoted constructor parameter type hint"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Standalone function type hints (not inside a class)
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a parameter type in a standalone function.
#[tokio::test]
async fn test_goto_definition_standalone_function_parameter_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "class User {\n",                       // 1
        "    public string $name;\n",           // 2
        "}\n",                                  // 3
        "function greet(User $user): void {\n", // 4
        "}\n",                                  // 5
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" on line 4
    let result = goto_definition(&backend, &uri, 4, 17).await;
    assert!(
        result.is_some(),
        "Should resolve standalone function parameter type hint"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a return type in a standalone function.
#[tokio::test]
async fn test_goto_definition_standalone_function_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                         // 0
        "class User {\n",                  // 1
        "    public string $name;\n",      // 2
        "}\n",                             // 3
        "function createUser(): User {\n", // 4
        "    return new User();\n",        // 5
        "}\n",                             // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in return type on line 4
    let result = goto_definition(&backend, &uri, 4, 25).await;
    assert!(
        result.is_some(),
        "Should resolve standalone function return type hint"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Cross-file resolution (PSR-4)
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a parameter type hint that resolves via PSR-4.
#[tokio::test]
async fn test_goto_definition_type_hint_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "\n",
                "class User {\n",
                "    public string $name;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "namespace App\\Http;\n",                         // 1
        "\n",                                             // 2
        "use App\\Models\\User;\n",                       // 3
        "\n",                                             // 4
        "class UserController {\n",                       // 5
        "    public function show(User $user): void {\n", // 6
        "    }\n",                                        // 7
        "}\n",                                            // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" on line 6 (parameter type hint)
    // "    public function show(User $user): void {"
    //  0   4      11       19  23 '('=24 U=25
    let result = goto_definition(&backend, &uri, 6, 26).await;
    assert!(
        result.is_some(),
        "Should resolve parameter type hint via PSR-4"
    );
    assert_location_path_ends_with(result.unwrap(), "src/Models/User.php", 3);
}

/// Clicking on a return type hint that resolves via PSR-4.
#[tokio::test]
async fn test_goto_definition_return_type_hint_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "\n",
                "class User {\n",
                "    public string $name;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "namespace App\\Http;\n",                  // 1
        "\n",                                      // 2
        "use App\\Models\\User;\n",                // 3
        "\n",                                      // 4
        "class UserController {\n",                // 5
        "    public function current(): User {\n", // 6
        "    }\n",                                 // 7
        "}\n",                                     // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in return type on line 6
    let result = goto_definition(&backend, &uri, 6, 35).await;
    assert!(
        result.is_some(),
        "Should resolve return type hint via PSR-4"
    );
    assert_location_path_ends_with(result.unwrap(), "src/Models/User.php", 3);
}

/// Clicking on a property type hint that resolves via PSR-4.
#[tokio::test]
async fn test_goto_definition_property_type_hint_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Repositories/UserRepository.php",
            concat!(
                "<?php\n",
                "namespace App\\Repositories;\n",
                "\n",
                "class UserRepository {\n",
                "    public function find(int $id): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "namespace App\\Services;\n",               // 1
        "\n",                                       // 2
        "use App\\Repositories\\UserRepository;\n", // 3
        "\n",                                       // 4
        "class UserService {\n",                    // 5
        "    private UserRepository $repo;\n",      // 6
        "}\n",                                      // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "UserRepository" on line 6
    let result = goto_definition(&backend, &uri, 6, 18).await;
    assert!(
        result.is_some(),
        "Should resolve property type hint via PSR-4"
    );
    assert_location_path_ends_with(result.unwrap(), "src/Repositories/UserRepository.php", 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Catch block type hints
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on the exception class name in a catch block.
#[tokio::test]
async fn test_goto_definition_catch_exception_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "class CustomException extends \\Exception {}\n", // 1
        "class App {\n",                                  // 2
        "    public function run(): void {\n",            // 3
        "        try {\n",                                // 4
        "        } catch (CustomException $e) {\n",       // 5
        "        }\n",                                    // 6
        "    }\n",                                        // 7
        "}\n",                                            // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "CustomException" on line 5
    let result = goto_definition(&backend, &uri, 5, 22).await;
    assert!(
        result.is_some(),
        "Should resolve exception type in catch block"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a multi-catch exception type.
#[tokio::test]
async fn test_goto_definition_multi_catch_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class NotFoundException extends \\Exception {}\n",               // 1
        "class ValidationException extends \\Exception {}\n",             // 2
        "class App {\n",                                                  // 3
        "    public function run(): void {\n",                            // 4
        "        try {\n",                                                // 5
        "        } catch (NotFoundException|ValidationException $e) {\n", // 6
        "        }\n",                                                    // 7
        "    }\n",                                                        // 8
        "}\n",                                                            // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "NotFoundException" on line 6
    let result = goto_definition(&backend, &uri, 6, 22).await;
    assert!(
        result.is_some(),
        "Should resolve first exception type in multi-catch"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "ValidationException" on line 6
    let result2 = goto_definition(&backend, &uri, 6, 40).await;
    assert!(
        result2.is_some(),
        "Should resolve second exception type in multi-catch"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Extends / Implements clauses
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on the parent class name in an `extends` clause.
#[tokio::test]
async fn test_goto_definition_extends_clause() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class BaseModel {\n",                   // 1
        "    public function save(): void {}\n", // 2
        "}\n",                                   // 3
        "class User extends BaseModel {\n",      // 4
        "}\n",                                   // 5
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "BaseModel" in "extends BaseModel" on line 4
    let result = goto_definition(&backend, &uri, 4, 22).await;
    assert!(
        result.is_some(),
        "Should resolve parent class in extends clause"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on an interface name in an `implements` clause.
#[tokio::test]
async fn test_goto_definition_implements_clause() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "interface Serializable {\n",                               // 1
        "    public function serialize(): string;\n",               // 2
        "}\n",                                                      // 3
        "class User implements Serializable {\n",                   // 4
        "    public function serialize(): string { return ''; }\n", // 5
        "}\n",                                                      // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Serializable" in "implements Serializable" on line 4
    let result = goto_definition(&backend, &uri, 4, 26).await;
    assert!(
        result.is_some(),
        "Should resolve interface in implements clause"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  `new` expression
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a class name after `new`.
#[tokio::test]
async fn test_goto_definition_new_expression() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "class User {\n",                      // 1
        "    public string $name;\n",          // 2
        "}\n",                                 // 3
        "class App {\n",                       // 4
        "    public function run(): void {\n", // 5
        "        $u = new User();\n",          // 6
        "    }\n",                             // 7
        "}\n",                                 // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "new User()" on line 6
    let result = goto_definition(&backend, &uri, 6, 20).await;
    assert!(result.is_some(), "Should resolve class name after new");
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Docblock type references (todo #23)
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a class name in a `@param` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Request {\n",                        // 1
        "    public function input(): string {}\n", // 2
        "}\n",                                      // 3
        "class Controller {\n",                     // 4
        "    /**\n",                                // 5
        "     * @param Request $req\n",             // 6
        "     */\n",                                // 7
        "    public function handle($req) {\n",     // 8
        "    }\n",                                  // 9
        "}\n",                                      // 10
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Request" in "@param Request" on line 6
    let result = goto_definition(&backend, &uri, 6, 16).await;
    assert!(
        result.is_some(),
        "Should resolve class name in @param docblock"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a class name in a `@return` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "class User {\n",                 // 1
        "    public string $name;\n",     // 2
        "}\n",                            // 3
        "class Repository {\n",           // 4
        "    /**\n",                      // 5
        "     * @return User\n",          // 6
        "     */\n",                      // 7
        "    public function find() {\n", // 8
        "    }\n",                        // 9
        "}\n",                            // 10
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "@return User" on line 6
    let result = goto_definition(&backend, &uri, 6, 16).await;
    assert!(
        result.is_some(),
        "Should resolve class name in @return docblock"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a class name inside generic brackets in a docblock `Collection<User>`.
#[tokio::test]
async fn test_goto_definition_docblock_generic_inner_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class User { public string $name; }\n",             // 1
        "class Collection { public function first() {} }\n", // 2
        "class Repository {\n",                              // 3
        "    /**\n",                                         // 4
        "     * @return Collection<User>\n",                 // 5
        "     */\n",                                         // 6
        "    public function all() {\n",                     // 7
        "    }\n",                                           // 8
        "}\n",                                               // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Collection" in "@return Collection<User>" on line 5
    let result = goto_definition(&backend, &uri, 5, 18).await;
    assert!(
        result.is_some(),
        "Should resolve outer generic class in docblock"
    );
    assert_location(result.unwrap(), &uri, 2);

    // Cursor on "User" inside angle brackets on line 5
    let result2 = goto_definition(&backend, &uri, 5, 30).await;
    assert!(
        result2.is_some(),
        "Should resolve inner generic type in docblock"
    );
    assert_location(result2.unwrap(), &uri, 1);
}

/// Clicking on a class name that follows both a non-ASCII class name and a
/// PHPStan `*` wildcard in the same generic argument list.
#[tokio::test]
async fn test_goto_definition_docblock_generic_wildcard_after_multibyte() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "class Café {}\n",                     // 1
        "class User {}\n",                     // 2
        "class Map {}\n",                      // 3
        "class Repository {\n",                // 4
        "    /**\n",                           // 5
        "     * @return Map<Café, *, User>\n", // 6
        "     */\n",                           // 7
        "    public function all() {\n",       // 8
        "    }\n",                             // 9
        "}\n",                                 // 10
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User", which sits after both the multibyte `é` and the
    // `*` wildcard on line 6.
    let result = goto_definition(&backend, &uri, 6, 29).await;
    assert!(
        result.is_some(),
        "Should resolve a generic argument that follows a multibyte name and a `*` wildcard"
    );
    assert_location(result.unwrap(), &uri, 2);
}

/// Clicking on a class name in a `@throws` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_throws_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class NotFoundException extends \\Exception {}\n", // 1
        "class Repository {\n",                             // 2
        "    /**\n",                                        // 3
        "     * @throws NotFoundException\n",               // 4
        "     */\n",                                        // 5
        "    public function find(int $id) {\n",            // 6
        "    }\n",                                          // 7
        "}\n",                                              // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "NotFoundException" in "@throws NotFoundException" on line 4
    let result = goto_definition(&backend, &uri, 4, 20).await;
    assert!(
        result.is_some(),
        "Should resolve exception class in @throws docblock"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a class name in a `@var` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_var_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "class Logger {\n",                     // 1
        "    public function log(): void {}\n", // 2
        "}\n",                                  // 3
        "class Service {\n",                    // 4
        "    /** @var Logger */\n",             // 5
        "    private $logger;\n",               // 6
        "}\n",                                  // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Logger" in "@var Logger" on line 5
    let result = goto_definition(&backend, &uri, 5, 15).await;
    assert!(
        result.is_some(),
        "Should resolve class name in @var docblock"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a class name in a union type within a docblock.
#[tokio::test]
async fn test_goto_definition_docblock_union_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "class User { public string $name; }\n",   // 1
        "class Admin { public string $role; }\n",  // 2
        "class Service {\n",                       // 3
        "    /**\n",                               // 4
        "     * @param User|Admin $person\n",      // 5
        "     */\n",                               // 6
        "    public function handle($person) {\n", // 7
        "    }\n",                                 // 8
        "}\n",                                     // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "@param User|Admin" on line 5
    let result = goto_definition(&backend, &uri, 5, 16).await;
    assert!(
        result.is_some(),
        "Should resolve first type in docblock union"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Admin" in "@param User|Admin" on line 5
    let result2 = goto_definition(&backend, &uri, 5, 22).await;
    assert!(
        result2.is_some(),
        "Should resolve second type in docblock union"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

/// Clicking on a class name in a docblock with cross-file PSR-4 resolution.
#[tokio::test]
async fn test_goto_definition_docblock_type_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "\n",
                "class User {\n",
                "    public string $name;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "namespace App\\Services;\n",     // 1
        "\n",                             // 2
        "use App\\Models\\User;\n",       // 3
        "\n",                             // 4
        "class UserService {\n",          // 5
        "    /**\n",                      // 6
        "     * @return User\n",          // 7
        "     */\n",                      // 8
        "    public function find() {\n", // 9
        "    }\n",                        // 10
        "}\n",                            // 11
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "@return User" on line 7
    let result = goto_definition(&backend, &uri, 7, 16).await;
    assert!(
        result.is_some(),
        "Should resolve docblock type via PSR-4 cross-file"
    );
    assert_location_path_ends_with(result.unwrap(), "src/Models/User.php", 3);
}

/// Clicking on a class name in an `@extends` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_extends_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Collection {}\n",                       // 1
        "class User {}\n",                             // 2
        "/**\n",                                       // 3
        " * @extends Collection<User>\n",              // 4
        " */\n",                                       // 5
        "class UserCollection extends Collection {\n", // 6
        "}\n",                                         // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Collection" in "@extends Collection<User>" on line 4
    let result = goto_definition(&backend, &uri, 4, 14).await;
    assert!(
        result.is_some(),
        "Should resolve class in @extends docblock"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "User" inside angle brackets on line 4
    let result2 = goto_definition(&backend, &uri, 4, 26).await;
    assert!(
        result2.is_some(),
        "Should resolve generic param type in @extends docblock"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §11  Inline @var annotation
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a class name in an inline `/** @var User $user */` annotation.
#[tokio::test]
async fn test_goto_definition_inline_var_annotation_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "class User {\n",                      // 1
        "    public string $name;\n",          // 2
        "}\n",                                 // 3
        "class App {\n",                       // 4
        "    public function run(): void {\n", // 5
        "        /** @var User $user */\n",    // 6
        "        $user = getUser();\n",        // 7
        "    }\n",                             // 8
        "}\n",                                 // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "/** @var User $user */" on line 6
    let result = goto_definition(&backend, &uri, 6, 20).await;
    assert!(
        result.is_some(),
        "Should resolve class name in inline @var annotation"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §12  Edge cases: scalars return None
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a scalar type hint like `string` should return None
/// (no class to navigate to).
#[tokio::test]
async fn test_goto_definition_scalar_type_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class App {\n",                                     // 1
        "    public function greet(string $name): void {\n", // 2
        "    }\n",                                           // 3
        "}\n",                                               // 4
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "string" on line 2
    let result = goto_definition(&backend, &uri, 2, 30).await;
    assert!(
        result.is_none(),
        "Scalar type hints should not resolve to any definition"
    );
}

/// Clicking on `void` return type should return None.
#[tokio::test]
async fn test_goto_definition_void_return_type_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "class App {\n",                       // 1
        "    public function run(): void {\n", // 2
        "    }\n",                             // 3
        "}\n",                                 // 4
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "void" on line 2
    let result = goto_definition(&backend, &uri, 2, 30).await;
    assert!(
        result.is_none(),
        "void type hint should not resolve to any definition"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// §13  `use` trait statement inside class body
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a trait name in a `use TraitName;` statement inside a class.
#[tokio::test]
async fn test_goto_definition_use_trait_statement() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "trait HasTimestamps {\n",                // 1
        "    public function touch(): void {}\n", // 2
        "}\n",                                    // 3
        "class User {\n",                         // 4
        "    use HasTimestamps;\n",               // 5
        "}\n",                                    // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "HasTimestamps" in "use HasTimestamps;" on line 5
    let result = goto_definition(&backend, &uri, 5, 10).await;
    assert!(
        result.is_some(),
        "Should resolve trait name in use statement"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §14  DNF (Disjunctive Normal Form) types
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a type inside DNF notation `(Foo&Bar)|null`.
#[tokio::test]
async fn test_goto_definition_dnf_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // 0
        "interface Countable { public function count(): int; }\n",              // 1
        "interface Stringable { public function __toString(): string; }\n",     // 2
        "class App {\n",                                                        // 3
        "    public function handle((Countable&Stringable)|null $x): void {\n", // 4
        "    }\n",                                                              // 5
        "}\n",                                                                  // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Countable" inside "(Countable&Stringable)|null" on line 4
    let result = goto_definition(&backend, &uri, 4, 33).await;
    assert!(
        result.is_some(),
        "Should resolve first type in DNF expression"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Stringable" inside "(Countable&Stringable)|null" on line 4
    let result2 = goto_definition(&backend, &uri, 4, 44).await;
    assert!(
        result2.is_some(),
        "Should resolve second type in DNF expression"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §15  Docblock @mixin and @property types
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a class name in a `@mixin` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_mixin_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class QueryBuilder {\n",                 // 1
        "    public function where(): void {}\n", // 2
        "}\n",                                    // 3
        "/**\n",                                  // 4
        " * @mixin QueryBuilder\n",               // 5
        " */\n",                                  // 6
        "class Model {\n",                        // 7
        "}\n",                                    // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "QueryBuilder" in "@mixin QueryBuilder" on line 5
    let result = goto_definition(&backend, &uri, 5, 14).await;
    assert!(result.is_some(), "Should resolve class in @mixin docblock");
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a class name in a `@property` docblock tag.
#[tokio::test]
async fn test_goto_definition_docblock_property_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                         // 0
        "class Address {\n",               // 1
        "    public string $street;\n",    // 2
        "}\n",                             // 3
        "/**\n",                           // 4
        " * @property Address $address\n", // 5
        " */\n",                           // 6
        "class User {\n",                  // 7
        "}\n",                             // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Address" in "@property Address $address" on line 5
    let result = goto_definition(&backend, &uri, 5, 17).await;
    assert!(
        result.is_some(),
        "Should resolve class in @property docblock"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §16  Docblock with nullable and generic combinations
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a class name in a nullable docblock type `?User`.
#[tokio::test]
async fn test_goto_definition_docblock_nullable_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class User { public string $name; }\n", // 1
        "class Repository {\n",                  // 2
        "    /**\n",                             // 3
        "     * @return ?User\n",                // 4
        "     */\n",                             // 5
        "    public function find() {\n",        // 6
        "    }\n",                               // 7
        "}\n",                                   // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "@return ?User" on line 4 (after the ?)
    let result = goto_definition(&backend, &uri, 4, 17).await;
    assert!(result.is_some(), "Should resolve class after ? in docblock");
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a class inside `array<int, User>` in a docblock.
#[tokio::test]
async fn test_goto_definition_docblock_array_generic_value_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class User { public string $name; }\n", // 1
        "class Repository {\n",                  // 2
        "    /**\n",                             // 3
        "     * @return array<int, User>\n",     // 4
        "     */\n",                             // 5
        "    public function all() {\n",         // 6
        "    }\n",                               // 7
        "}\n",                                   // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "array<int, User>" on line 4
    // "     * @return array<int, User>"
    //  0123456789012345678901234567890
    let result = goto_definition(&backend, &uri, 4, 27).await;
    assert!(
        result.is_some(),
        "Should resolve value type in array generic docblock"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §17  Multiple interface implements
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on the second interface in `implements A, B`.
#[tokio::test]
async fn test_goto_definition_second_implements_interface() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "interface Loggable { public function log(): void; }\n",    // 1
        "interface Cacheable { public function cache(): void; }\n", // 2
        "class Service implements Loggable, Cacheable {\n",         // 3
        "    public function log(): void {}\n",                     // 4
        "    public function cache(): void {}\n",                   // 5
        "}\n",                                                      // 6
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Loggable" in "implements Loggable, Cacheable" on line 3
    let result = goto_definition(&backend, &uri, 3, 27).await;
    assert!(
        result.is_some(),
        "Should resolve first interface in implements list"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Cacheable" in "implements Loggable, Cacheable" on line 3
    let result2 = goto_definition(&backend, &uri, 3, 39).await;
    assert!(
        result2.is_some(),
        "Should resolve second interface in implements list"
    );
    assert_location(result2.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §18  Closure / arrow function type hints
// ═══════════════════════════════════════════════════════════════════════════

/// Clicking on a type hint in a closure parameter.
#[tokio::test]
async fn test_goto_definition_closure_parameter_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "class User { public string $name; }\n",   // 1
        "class App {\n",                           // 2
        "    public function run(): void {\n",     // 3
        "        $fn = function (User $user) {\n", // 4
        "        };\n",                            // 5
        "    }\n",                                 // 6
        "}\n",                                     // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "function (User $user)" on line 4
    let result = goto_definition(&backend, &uri, 4, 26).await;
    assert!(
        result.is_some(),
        "Should resolve type hint in closure parameter"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// Clicking on a return type hint in a closure.
#[tokio::test]
async fn test_goto_definition_closure_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class User { public string $name; }\n", // 1
        "class App {\n",                         // 2
        "    public function run(): void {\n",   // 3
        "        $fn = function (): User {\n",   // 4
        "            return new User();\n",      // 5
        "        };\n",                          // 6
        "    }\n",                               // 7
        "}\n",                                   // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in "function (): User" on line 4
    let result = goto_definition(&backend, &uri, 4, 30).await;
    assert!(
        result.is_some(),
        "Should resolve return type hint in closure"
    );
    assert_location(result.unwrap(), &uri, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// §12  Callable types inside docblock annotations
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on the return type of a callable annotation: `\Closure(): Pencil`.
#[tokio::test]
async fn test_goto_definition_docblock_callable_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Pencil {\n",                                // 1
        "    public string $color;\n",                     // 2
        "}\n",                                             // 3
        "class Factory {\n",                               // 4
        "    /** @var \\Closure(): Pencil $supplier */\n", // 5
        "    private $supplier;\n",                        // 6
        "}\n",                                             // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Pencil" in `\Closure(): Pencil` on line 5
    let result = goto_definition(&backend, &uri, 5, 29).await;
    assert!(
        result.is_some(),
        "Should resolve Pencil in callable return type"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// GTD on a parameter type inside a callable annotation:
/// `callable(Request): Response`.
#[tokio::test]
async fn test_goto_definition_docblock_callable_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class Request {\n",                                      // 1
        "    public string $body;\n",                             // 2
        "}\n",                                                    // 3
        "class Response {\n",                                     // 4
        "    public int $status;\n",                              // 5
        "}\n",                                                    // 6
        "class Handler {\n",                                      // 7
        "    /** @var callable(Request): Response $handler */\n", // 8
        "    private $handler;\n",                                // 9
        "}\n",                                                    // 10
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Request" inside `callable(Request)` on line 8
    let result = goto_definition(&backend, &uri, 8, 24).await;
    assert!(
        result.is_some(),
        "Should resolve Request in callable param type"
    );
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Response" in the callable return type on line 8
    let result = goto_definition(&backend, &uri, 8, 34).await;
    assert!(
        result.is_some(),
        "Should resolve Response in callable return type"
    );
    assert_location(result.unwrap(), &uri, 4);
}

/// GTD on `\Closure` itself inside a callable annotation.
#[tokio::test]
async fn test_goto_definition_docblock_callable_base_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "class Result {\n",                               // 1
        "    public string $value;\n",                    // 2
        "}\n",                                            // 3
        "class Worker {\n",                               // 4
        "    /** @param \\Closure(int): Result $cb */\n", // 5
        "    public function run($cb) {}\n",              // 6
        "}\n",                                            // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Result" in callable return type on line 5
    let result = goto_definition(&backend, &uri, 5, 35).await;
    assert!(
        result.is_some(),
        "Should resolve Result in callable return type"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// GTD on a callable with multiple parameter types.
#[tokio::test]
async fn test_goto_definition_docblock_callable_multiple_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "class Config {}\n",                                        // 1
        "class Logger {}\n",                                        // 2
        "class Output {}\n",                                        // 3
        "class App {\n",                                            // 4
        "    /** @var callable(Config, Logger): Output $boot */\n", // 5
        "    private $boot;\n",                                     // 6
        "}\n",                                                      // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Config" on line 5
    let result = goto_definition(&backend, &uri, 5, 24).await;
    assert!(result.is_some(), "Should resolve Config in callable param");
    assert_location(result.unwrap(), &uri, 1);

    // Cursor on "Logger" on line 5
    let result = goto_definition(&backend, &uri, 5, 32).await;
    assert!(result.is_some(), "Should resolve Logger in callable param");
    assert_location(result.unwrap(), &uri, 2);

    // Cursor on "Output" on line 5
    let result = goto_definition(&backend, &uri, 5, 41).await;
    assert!(
        result.is_some(),
        "Should resolve Output in callable return type"
    );
    assert_location(result.unwrap(), &uri, 3);
}

/// GTD on callable return type in @return tag.
#[tokio::test]
async fn test_goto_definition_docblock_return_callable_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // 0
        "class User {}\n",                           // 1
        "class Repository {\n",                      // 2
        "    /**\n",                                 // 3
        "     * @return callable(): User\n",         // 4
        "     */\n",                                 // 5
        "    public function getUserFactory() {}\n", // 6
        "}\n",                                       // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "User" in `callable(): User` on line 4
    let result = goto_definition(&backend, &uri, 4, 27).await;
    assert!(
        result.is_some(),
        "Should resolve User in callable return type of @return"
    );
    assert_location(result.unwrap(), &uri, 1);
}

/// GTD on callable with no return type (bare `callable(Type)`).
#[tokio::test]
async fn test_goto_definition_docblock_callable_no_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "class Event {}\n",                              // 1
        "class Dispatcher {\n",                          // 2
        "    /** @param callable(Event) $listener */\n", // 3
        "    public function listen($listener) {}\n",    // 4
        "}\n",                                           // 5
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "Event" in `callable(Event)` on line 3
    let result = goto_definition(&backend, &uri, 3, 28).await;
    assert!(
        result.is_some(),
        "Should resolve Event in callable param type without return type"
    );
    assert_location(result.unwrap(), &uri, 1);
}

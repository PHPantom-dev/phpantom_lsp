use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::request::{GotoTypeDefinitionParams, GotoTypeDefinitionResponse};
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

async fn goto_type_definition(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<GotoTypeDefinitionResponse> {
    let params = GotoTypeDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    backend.goto_type_definition(params).await.unwrap()
}

fn assert_single_location(response: GotoTypeDefinitionResponse, expected_line: u32) -> Location {
    match response {
        GotoTypeDefinitionResponse::Scalar(loc) => {
            assert_eq!(
                loc.range.start.line, expected_line,
                "Expected type definition on line {}, got line {}",
                expected_line, loc.range.start.line
            );
            loc
        }
        GotoTypeDefinitionResponse::Array(locs) => {
            assert_eq!(
                locs.len(),
                1,
                "Expected exactly one location, got {}",
                locs.len()
            );
            assert_eq!(
                locs[0].range.start.line, expected_line,
                "Expected type definition on line {}, got line {}",
                expected_line, locs[0].range.start.line
            );
            locs[0].clone()
        }
        GotoTypeDefinitionResponse::Link(_) => {
            panic!("Expected Scalar or Array response, got Link");
        }
    }
}

fn assert_multiple_locations(
    response: GotoTypeDefinitionResponse,
    expected_lines: &[u32],
) -> Vec<Location> {
    match response {
        GotoTypeDefinitionResponse::Array(locs) => {
            assert_eq!(
                locs.len(),
                expected_lines.len(),
                "Expected {} locations, got {}",
                expected_lines.len(),
                locs.len()
            );
            let mut actual_lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
            actual_lines.sort();
            let mut expected_sorted = expected_lines.to_vec();
            expected_sorted.sort();
            assert_eq!(actual_lines, expected_sorted, "Location lines don't match");
            locs
        }
        GotoTypeDefinitionResponse::Scalar(loc) => {
            assert_eq!(
                expected_lines.len(),
                1,
                "Got single location but expected {}",
                expected_lines.len()
            );
            assert_eq!(loc.range.start.line, expected_lines[0]);
            vec![loc]
        }
        GotoTypeDefinitionResponse::Link(_) => {
            panic!("Expected Array response, got Link");
        }
    }
}

// ─── Variable Type Definition ───────────────────────────────────────────────

#[tokio::test]
async fn test_variable_type_definition_from_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Logger {\n",                                  // 1
        "    public function info(): void {}\n",             // 2
        "}\n",                                               // 3
        "class Service {\n",                                 // 4
        "    public function run(Logger $logger): void {\n", // 5
        "        $logger->info();\n",                        // 6
        "    }\n",                                           // 7
        "}\n",                                               // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $logger in method body (line 6, char 9)
    let result = goto_type_definition(&backend, &uri, 6, 9).await;
    assert!(
        result.is_some(),
        "Should resolve type definition for $logger"
    );
    assert_single_location(result.unwrap(), 1); // Logger class on line 1
}

#[tokio::test]
async fn test_variable_type_definition_from_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "class User {\n",             // 1
        "    public string $name;\n", // 2
        "}\n",                        // 3
        "function test() {\n",        // 4
        "    $user = new User();\n",  // 5
        "    $user->name;\n",         // 6
        "}\n",                        // 7
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $user on line 6
    let result = goto_type_definition(&backend, &uri, 6, 5).await;
    assert!(result.is_some(), "Should resolve type definition for $user");
    assert_single_location(result.unwrap(), 1); // User class on line 1
}

#[tokio::test]
async fn test_this_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "class MyClass {\n",                    // 1
        "    public function test(): void {\n", // 2
        "        $this->test();\n",             // 3
        "    }\n",                              // 4
        "}\n",                                  // 5
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $this (line 3, char 9)
    let result = goto_type_definition(&backend, &uri, 3, 9).await;
    assert!(result.is_some(), "Should resolve type definition for $this");
    assert_single_location(result.unwrap(), 1); // MyClass on line 1
}

// ─── Member Access Type Definition ──────────────────────────────────────────

#[tokio::test]
async fn test_method_return_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Result {\n",                                     // 1
        "    public function value(): string { return ''; }\n", // 2
        "}\n",                                                  // 3
        "class Service {\n",                                    // 4
        "    public function getResult(): Result {\n",          // 5
        "        return new Result();\n",                       // 6
        "    }\n",                                              // 7
        "}\n",                                                  // 8
        "function test(Service $svc) {\n",                      // 9
        "    $svc->getResult();\n",                             // 10
        "}\n",                                                  // 11
    );
    open_php(&backend, &uri, text).await;

    // Cursor on getResult method call (line 10)
    let result = goto_type_definition(&backend, &uri, 10, 11).await;
    assert!(
        result.is_some(),
        "Should resolve type definition for getResult()"
    );
    assert_single_location(result.unwrap(), 1); // Result class on line 1
}

#[tokio::test]
async fn test_property_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "class Address {\n",              // 1
        "    public string $city;\n",     // 2
        "}\n",                            // 3
        "class User {\n",                 // 4
        "    public Address $address;\n", // 5
        "}\n",                            // 6
        "function test(User $user) {\n",  // 7
        "    $user->address;\n",          // 8
        "}\n",                            // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on address property access (line 8)
    let result = goto_type_definition(&backend, &uri, 8, 12).await;
    assert!(
        result.is_some(),
        "Should resolve type definition for ->address"
    );
    assert_single_location(result.unwrap(), 1); // Address class on line 1
}

// ─── self / static / parent ─────────────────────────────────────────────────

#[tokio::test]
async fn test_self_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "class Widget {\n",                              // 1
        "    public static function create(): self {\n", // 2
        "        return new self();\n",                  // 3
        "    }\n",                                       // 4
        "}\n",                                           // 5
    );
    open_php(&backend, &uri, text).await;

    // Cursor on `self` in `new self()` (line 3)
    let result = goto_type_definition(&backend, &uri, 3, 20).await;
    assert!(result.is_some(), "Should resolve type definition for self");
    assert_single_location(result.unwrap(), 1); // Widget on line 1
}

#[tokio::test]
async fn test_parent_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class Base {\n",                         // 1
        "    public function hello(): void {}\n", // 2
        "}\n",                                    // 3
        "class Child extends Base {\n",           // 4
        "    public function test(): void {\n",   // 5
        "        parent::hello();\n",             // 6
        "    }\n",                                // 7
        "}\n",                                    // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on `parent` (line 6)
    let result = goto_type_definition(&backend, &uri, 6, 10).await;
    assert!(
        result.is_some(),
        "Should resolve type definition for parent"
    );
    assert_single_location(result.unwrap(), 1); // Base on line 1
}

// ─── Class Reference ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_class_reference_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "class Foo {\n",                        // 1
        "    public function bar(): void {}\n", // 2
        "}\n",                                  // 3
        "function test(Foo $f) {\n",            // 4
        "}\n",                                  // 5
    );
    open_php(&backend, &uri, text).await;

    // Cursor on Foo type hint in function parameter (line 4, char 15)
    let result = goto_type_definition(&backend, &uri, 4, 15).await;
    assert!(
        result.is_some(),
        "Should resolve type definition for class reference Foo"
    );
    assert_single_location(result.unwrap(), 1); // Foo on line 1
}

// ─── Function Call Return Type ──────────────────────────────────────────────

#[tokio::test]
async fn test_function_call_return_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "class Config {\n",                 // 1
        "    public string $value;\n",      // 2
        "}\n",                              // 3
        "/** @return Config */\n",          // 4
        "function getConfig(): Config {\n", // 5
        "    return new Config();\n",       // 6
        "}\n",                              // 7
        "function test() {\n",              // 8
        "    getConfig();\n",               // 9
        "}\n",                              // 10
    );
    open_php(&backend, &uri, text).await;

    // Cursor on getConfig() call (line 9)
    let result = goto_type_definition(&backend, &uri, 9, 7).await;
    assert!(
        result.is_some(),
        "Should resolve type definition for getConfig()"
    );
    assert_single_location(result.unwrap(), 1); // Config class on line 1
}

// ─── Scalars return None ────────────────────────────────────────────────────

#[tokio::test]
async fn test_scalar_type_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                // 0
        "function test() {\n",    // 1
        "    $name = 'hello';\n", // 2
        "    $name;\n",           // 3
        "}\n",                    // 4
    );
    open_php(&backend, &uri, text).await;

    // $name is a string — no class to jump to.
    let result = goto_type_definition(&backend, &uri, 3, 5).await;
    assert!(
        result.is_none(),
        "Scalar type should not produce a type definition"
    );
}

// ─── Nullable type ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_nullable_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Token {\n",                             // 1
        "    public string $value;\n",                 // 2
        "}\n",                                         // 3
        "class Parser {\n",                            // 4
        "    public function nextToken(): ?Token {\n", // 5
        "        return null;\n",                      // 6
        "    }\n",                                     // 7
        "}\n",                                         // 8
        "function test(Parser $p) {\n",                // 9
        "    $p->nextToken();\n",                      // 10
        "}\n",                                         // 11
    );
    open_php(&backend, &uri, text).await;

    // Cursor on nextToken() — return type is ?Token, should resolve to Token
    let result = goto_type_definition(&backend, &uri, 10, 9).await;
    assert!(result.is_some(), "Should resolve nullable type definition");
    assert_single_location(result.unwrap(), 1); // Token class on line 1
}

// ─── Union type → multiple locations ────────────────────────────────────────

#[tokio::test]
async fn test_union_type_multiple_locations() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Cat {\n",                            // 1
        "    public function meow(): void {}\n",    // 2
        "}\n",                                      // 3
        "class Dog {\n",                            // 4
        "    public function bark(): void {}\n",    // 5
        "}\n",                                      // 6
        "class Shelter {\n",                        // 7
        "    /** @return Cat|Dog */\n",             // 8
        "    public function adopt(): Cat|Dog {\n", // 9
        "        return new Cat();\n",              // 10
        "    }\n",                                  // 11
        "}\n",                                      // 12
        "function test(Shelter $s) {\n",            // 13
        "    $s->adopt();\n",                       // 14
        "}\n",                                      // 15
    );
    open_php(&backend, &uri, text).await;

    // Cursor on adopt() — return type is Cat|Dog
    let result = goto_type_definition(&backend, &uri, 14, 9).await;
    assert!(result.is_some(), "Should resolve union type definition");
    assert_multiple_locations(result.unwrap(), &[1, 4]); // Cat on line 1, Dog on line 4
}

// ─── Cross-file type definition via PSR-4 ───────────────────────────────────

#[tokio::test]
async fn test_cross_file_type_definition() {
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
                "    public string $email;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test_consumer.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "namespace App;\n",                               // 1
        "use App\\Models\\User;\n",                       // 2
        "\n",                                             // 3
        "class UserService {\n",                          // 4
        "    public function find(User $user): void {\n", // 5
        "        $user;\n",                               // 6
        "    }\n",                                        // 7
        "}\n",                                            // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $user in method body (line 6) — type is User from cross-file
    let result = goto_type_definition(&backend, &uri, 6, 9).await;
    assert!(
        result.is_some(),
        "Should resolve cross-file type definition for User"
    );

    match result.unwrap() {
        GotoTypeDefinitionResponse::Scalar(loc) => {
            let path = loc.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Models/User.php"),
                "Should point to src/Models/User.php, got: {:?}",
                path
            );
        }
        other => {
            panic!("Expected Scalar response, got {:?}", other);
        }
    }
}

// ─── Docblock @var type definition ──────────────────────────────────────────

#[tokio::test]
async fn test_docblock_var_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                        // 0
        "class Order {\n",                // 1
        "    public int $total;\n",       // 2
        "}\n",                            // 3
        "function test() {\n",            // 4
        "    /** @var Order $order */\n", // 5
        "    $order = getOrder();\n",     // 6
        "    $order;\n",                  // 7
        "}\n",                            // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $order (line 7)
    let result = goto_type_definition(&backend, &uri, 7, 5).await;
    assert!(result.is_some(), "Should resolve @var type definition");
    assert_single_location(result.unwrap(), 1); // Order on line 1
}

// ─── Foreach variable type definition ───────────────────────────────────────

#[tokio::test]
async fn test_foreach_variable_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class Item {\n",                                      // 1
        "    public string $name;\n",                          // 2
        "}\n",                                                 // 3
        "class Cart {\n",                                      // 4
        "    /** @param Item[] $items */\n",                   // 5
        "    public function process(array $items): void {\n", // 6
        "        foreach ($items as $item) {\n",               // 7
        "            $item;\n",                                // 8
        "        }\n",                                         // 9
        "    }\n",                                             // 10
        "}\n",                                                 // 11
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $item in the foreach body (line 8)
    let result = goto_type_definition(&backend, &uri, 8, 13).await;
    assert!(
        result.is_some(),
        "Should resolve foreach variable type definition"
    );
    assert_single_location(result.unwrap(), 1); // Item on line 1
}

// ─── Static method return type ──────────────────────────────────────────────

#[tokio::test]
async fn test_static_method_return_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Connection {\n",                                 // 1
        "    public function query(): void {}\n",               // 2
        "}\n",                                                  // 3
        "class DB {\n",                                         // 4
        "    public static function connect(): Connection {\n", // 5
        "        return new Connection();\n",                   // 6
        "    }\n",                                              // 7
        "}\n",                                                  // 8
        "function test() {\n",                                  // 9
        "    DB::connect();\n",                                 // 10
        "}\n",                                                  // 11
    );
    open_php(&backend, &uri, text).await;

    // Cursor on connect() static call (line 10)
    let result = goto_type_definition(&backend, &uri, 10, 8).await;
    assert!(
        result.is_some(),
        "Should resolve static method return type definition"
    );
    assert_single_location(result.unwrap(), 1); // Connection on line 1
}

// ─── No type definition for class/member declarations ───────────────────────

#[tokio::test]
async fn test_class_declaration_returns_none() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",       // 0
        "class Foo {\n", // 1
        "}\n",           // 2
    );
    open_php(&backend, &uri, text).await;

    // Cursor on the `Foo` class declaration name (line 1, char 7)
    let result = goto_type_definition(&backend, &uri, 1, 7).await;
    assert!(
        result.is_none(),
        "Class declaration should not produce a type definition"
    );
}

// ─── Chained method call type definition ────────────────────────────────────

#[tokio::test]
async fn test_chained_method_return_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Builder {\n",                        // 1
        "    public function where(): Builder {\n", // 2
        "        return $this;\n",                  // 3
        "    }\n",                                  // 4
        "    public function first(): Builder {\n", // 5
        "        return $this;\n",                  // 6
        "    }\n",                                  // 7
        "}\n",                                      // 8
        "function test(Builder $b) {\n",            // 9
        "    $b->where();\n",                       // 10
        "}\n",                                      // 11
    );
    open_php(&backend, &uri, text).await;

    // Cursor on where() — return type is Builder
    let result = goto_type_definition(&backend, &uri, 10, 9).await;
    assert!(
        result.is_some(),
        "Should resolve chained method return type definition"
    );
    assert_single_location(result.unwrap(), 1); // Builder on line 1
}

// ─── Method returning self/static ───────────────────────────────────────────

#[tokio::test]
async fn test_method_returning_self_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "class Fluent {\n",                    // 1
        "    public function set(): self {\n", // 2
        "        return $this;\n",             // 3
        "    }\n",                             // 4
        "}\n",                                 // 5
        "function test(Fluent $f) {\n",        // 6
        "    $f->set();\n",                    // 7
        "}\n",                                 // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on set() — return type is self, should resolve to Fluent
    let result = goto_type_definition(&backend, &uri, 7, 9).await;
    assert!(
        result.is_some(),
        "Should resolve self return type to Fluent"
    );
    assert_single_location(result.unwrap(), 1); // Fluent on line 1
}

// ─── Union type with scalars filtered ───────────────────────────────────────

#[tokio::test]
async fn test_union_with_scalar_filters_scalars() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class ErrorResult {\n",                                // 1
        "    public string $message;\n",                        // 2
        "}\n",                                                  // 3
        "class Handler {\n",                                    // 4
        "    /** @return string|ErrorResult */\n",              // 5
        "    public function handle(): string|ErrorResult {\n", // 6
        "        return '';\n",                                 // 7
        "    }\n",                                              // 8
        "}\n",                                                  // 9
        "function test(Handler $h) {\n",                        // 10
        "    $h->handle();\n",                                  // 11
        "}\n",                                                  // 12
    );
    open_php(&backend, &uri, text).await;

    // Cursor on handle() — return type is string|ErrorResult, only ErrorResult should remain
    let result = goto_type_definition(&backend, &uri, 11, 9).await;
    assert!(
        result.is_some(),
        "Should resolve union type with scalar filtered"
    );
    assert_single_location(result.unwrap(), 1); // ErrorResult on line 1
}

// ─── Catch variable type definition ─────────────────────────────────────────

#[tokio::test]
async fn test_catch_variable_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "class AppException extends Exception {\n", // 1 (note: Exception may not be defined in test)
        "    public function getContext(): array { return []; }\n", // 2
        "}\n",                                      // 3
        "function test() {\n",                      // 4
        "    try {\n",                              // 5
        "        throw new AppException();\n",      // 6
        "    } catch (AppException $e) {\n",        // 7
        "        $e;\n",                            // 8
        "    }\n",                                  // 9
        "}\n",                                      // 10
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $e in catch body (line 8)
    let result = goto_type_definition(&backend, &uri, 8, 9).await;
    assert!(
        result.is_some(),
        "Should resolve catch variable type definition"
    );
    assert_single_location(result.unwrap(), 1); // AppException on line 1
}

// ─── Parameter type definition ──────────────────────────────────────────────

#[tokio::test]
async fn test_parameter_type_definition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class Request {\n",                                      // 1
        "    public function input(): string { return ''; }\n",   // 2
        "}\n",                                                    // 3
        "class Controller {\n",                                   // 4
        "    public function handle(Request $request): void {\n", // 5
        "        $request;\n",                                    // 6
        "    }\n",                                                // 7
        "}\n",                                                    // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on $request in the method body (line 6)
    let result = goto_type_definition(&backend, &uri, 6, 9).await;
    assert!(result.is_some(), "Should resolve parameter type definition");
    assert_single_location(result.unwrap(), 1); // Request on line 1
}

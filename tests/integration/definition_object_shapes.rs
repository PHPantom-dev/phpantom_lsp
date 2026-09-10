//! Go-to-definition tests for synthetic object shape properties.
//!
//! When a variable's type is an object shape (`object{name: string, age: int}`),
//! clicking on a property access like `$profile->name` should jump to the
//! property key inside the docblock annotation that defines the shape.

use crate::common::{create_test_backend, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

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

// ═══════════════════════════════════════════════════════════════════════════
// §1  Object shape from @return annotation
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on `$data->name` where the type comes from
/// `@return object{name: string, age: int}`.
#[tokio::test]
async fn test_gtd_object_shape_return_type_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    //                                                          line
    let text = concat!(
        "<?php\n",                                               // 0
        "class Service {\n",                                     // 1
        "    /**\n",                                             // 2
        "     * @return object{name: string, age: int}\n",       // 3
        "     */\n",                                             // 4
        "    public function getData(): object {\n",             // 5
        "        return (object)['name' => 'a', 'age' => 1];\n", // 6
        "    }\n",                                               // 7
        "}\n",                                                   // 8
        "class Demo {\n",                                        // 9
        "    public function run(): void {\n",                   // 10
        "        $svc = new Service();\n",                       // 11
        "        $data = $svc->getData();\n",                    // 12
        "        $data->name;\n",                                // 13
        "        $data->age;\n",                                 // 14
        "    }\n",                                               // 15
        "}\n",                                                   // 16
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "name" in `$data->name` on line 13
    let result = goto_definition(&backend, &uri, 13, 15).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'name'"
    );
    // Should jump to line 3 where `name:` is defined in the docblock
    assert_location(result.unwrap(), &uri, 3);

    // Cursor on "age" in `$data->age` on line 14
    let result = goto_definition(&backend, &uri, 14, 15).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'age'"
    );
    assert_location(result.unwrap(), &uri, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Object shape from inline @var annotation
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on `$item->title` where the type comes from
/// `/** @var object{title: string, score: float} $item */`.
#[tokio::test]
async fn test_gtd_object_shape_inline_var_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                         // 0
        "class Demo {\n",                                                  // 1
        "    public function run(): void {\n",                             // 2
        "        /** @var object{title: string, score: float} $item */\n", // 3
        "        $item = getUnknownValue();\n",                            // 4
        "        $item->title;\n",                                         // 5
        "        $item->score;\n",                                         // 6
        "    }\n",                                                         // 7
        "}\n",                                                             // 8
        "function getUnknownValue(): mixed { return null; }\n",            // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "title" in `$item->title` on line 5
    let result = goto_definition(&backend, &uri, 5, 15).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'title' from inline @var"
    );
    assert_location(result.unwrap(), &uri, 3);

    // Cursor on "score" in `$item->score` on line 6
    let result = goto_definition(&backend, &uri, 6, 15).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'score' from inline @var"
    );
    assert_location(result.unwrap(), &uri, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Object shape from @param annotation
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on `$config->host` where the type comes from a @param annotation.
#[tokio::test]
async fn test_gtd_object_shape_param_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "class Server {\n",                                        // 1
        "    /**\n",                                               // 2
        "     * @param object{host: string, port: int} $config\n", // 3
        "     */\n",                                               // 4
        "    public function connect($config): void {\n",          // 5
        "        $config->host;\n",                                // 6
        "        $config->port;\n",                                // 7
        "    }\n",                                                 // 8
        "}\n",                                                     // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "host" in `$config->host` on line 6
    let result = goto_definition(&backend, &uri, 6, 18).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'host' from @param"
    );
    assert_location(result.unwrap(), &uri, 3);

    // Cursor on "port" in `$config->port` on line 7
    let result = goto_definition(&backend, &uri, 7, 18).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'port' from @param"
    );
    assert_location(result.unwrap(), &uri, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Object shape with class-typed property values
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on `$result->tool` where the value type is a class (`Pen`).
/// Completion resolves the class, but GTD on the property name itself
/// should jump to the docblock key.
#[tokio::test]
async fn test_gtd_object_shape_property_with_class_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                       // 0
        "class Pen { public function write(): void {} }\n",              // 1
        "class Workshop {\n",                                            // 2
        "    /**\n",                                                     // 3
        "     * @return object{tool: Pen, label: string}\n",             // 4
        "     */\n",                                                     // 5
        "    public function getKit(): object { return (object)[]; }\n", // 6
        "}\n",                                                           // 7
        "class Demo {\n",                                                // 8
        "    public function run(): void {\n",                           // 9
        "        $kit = (new Workshop())->getKit();\n",                  // 10
        "        $kit->tool;\n",                                         // 11
        "        $kit->label;\n",                                        // 12
        "    }\n",                                                       // 13
        "}\n",                                                           // 14
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "tool" in `$kit->tool` on line 11
    let result = goto_definition(&backend, &uri, 11, 14).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'tool'"
    );
    assert_location(result.unwrap(), &uri, 4);

    // Cursor on "label" in `$kit->label` on line 12
    let result = goto_definition(&backend, &uri, 12, 14).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'label'"
    );
    assert_location(result.unwrap(), &uri, 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Nullable object shape
// ═══════════════════════════════════════════════════════════════════════════

/// GTD works on a nullable object shape (`?object{…}`).
#[tokio::test]
async fn test_gtd_nullable_object_shape() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "class Fetcher {\n",                                       // 1
        "    /**\n",                                               // 2
        "     * @return ?object{id: int, name: string}\n",         // 3
        "     */\n",                                               // 4
        "    public function fetch(): ?object { return null; }\n", // 5
        "}\n",                                                     // 6
        "class Demo {\n",                                          // 7
        "    public function run(): void {\n",                     // 8
        "        $result = (new Fetcher())->fetch();\n",           // 9
        "        $result->id;\n",                                  // 10
        "    }\n",                                                 // 11
        "}\n",                                                     // 12
    );
    open_php(&backend, &uri, text).await;

    let result = goto_definition(&backend, &uri, 10, 18).await;
    assert!(
        result.is_some(),
        "Should resolve property 'id' on nullable object shape"
    );
    assert_location(result.unwrap(), &uri, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Optional property keys
// ═══════════════════════════════════════════════════════════════════════════

/// GTD works on optional property keys (`key?: type`).
#[tokio::test]
async fn test_gtd_object_shape_optional_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                      // 0
        "class Builder {\n",                                            // 1
        "    /**\n",                                                    // 2
        "     * @return object{required: int, optional?: string}\n",    // 3
        "     */\n",                                                    // 4
        "    public function build(): object { return (object)[]; }\n", // 5
        "}\n",                                                          // 6
        "class Demo {\n",                                               // 7
        "    public function run(): void {\n",                          // 8
        "        $obj = (new Builder())->build();\n",                   // 9
        "        $obj->required;\n",                                    // 10
        "        $obj->optional;\n",                                    // 11
        "    }\n",                                                      // 12
        "}\n",                                                          // 13
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "required" on line 10
    let result = goto_definition(&backend, &uri, 10, 14).await;
    assert!(
        result.is_some(),
        "Should resolve required object shape property"
    );
    assert_location(result.unwrap(), &uri, 3);

    // Cursor on "optional" on line 11
    let result = goto_definition(&backend, &uri, 11, 14).await;
    assert!(
        result.is_some(),
        "Should resolve optional object shape property"
    );
    assert_location(result.unwrap(), &uri, 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Object shape from @var on a class property
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on `$this->config->host` where `$this->config` has
/// `@var object{host: string, port: int}`.
#[tokio::test]
async fn test_gtd_object_shape_class_property_var() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class App {\n",                                     // 1
        "    /** @var object{host: string, port: int} */\n", // 2
        "    public $config;\n",                             // 3
        "    public function run(): void {\n",               // 4
        "        $this->config->host;\n",                    // 5
        "        $this->config->port;\n",                    // 6
        "    }\n",                                           // 7
        "}\n",                                               // 8
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "host" in `$this->config->host` on line 5
    let result = goto_definition(&backend, &uri, 5, 23).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'host' from class property @var"
    );
    assert_location(result.unwrap(), &uri, 2);

    // Cursor on "port" in `$this->config->port` on line 6
    let result = goto_definition(&backend, &uri, 6, 23).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'port' from class property @var"
    );
    assert_location(result.unwrap(), &uri, 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Multiple object shapes in the same file — closest match wins
// ═══════════════════════════════════════════════════════════════════════════

/// When the same property key appears in multiple object shapes, GTD
/// should jump to the one closest to (but before) the cursor.
#[tokio::test]
async fn test_gtd_object_shape_closest_match() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                    // 0
        "class First {\n",                                            // 1
        "    /** @return object{name: string} */\n",                  // 2
        "    public function get(): object { return (object)[]; }\n", // 3
        "}\n",                                                        // 4
        "class Second {\n",                                           // 5
        "    /** @return object{name: string, extra: int} */\n",      // 6
        "    public function get(): object { return (object)[]; }\n", // 7
        "}\n",                                                        // 8
        "class Demo {\n",                                             // 9
        "    public function run(): void {\n",                        // 10
        "        $b = (new Second())->get();\n",                      // 11
        "        $b->name;\n",                                        // 12
        "    }\n",                                                    // 13
        "}\n",                                                        // 14
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "name" in `$b->name` on line 12 — should prefer line 6
    // (the closest `object{name: …}` before the cursor).
    let result = goto_definition(&backend, &uri, 12, 13).await;
    assert!(
        result.is_some(),
        "Should resolve 'name' to closest object shape"
    );
    assert_location(result.unwrap(), &uri, 6);
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  Nested object shape (first level)
// ═══════════════════════════════════════════════════════════════════════════

/// GTD on `$result->meta` from `@return object{data: string, meta: object{page: int}}`.
#[tokio::test]
async fn test_gtd_nested_object_shape_first_level() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                    // 0
        "class Api {\n",                                                              // 1
        "    /**\n",                                                                  // 2
        "     * @return object{data: string, meta: object{page: int, total: int}}\n", // 3
        "     */\n",                                                                  // 4
        "    public function fetch(): object { return (object)[]; }\n",               // 5
        "}\n",                                                                        // 6
        "class Demo {\n",                                                             // 7
        "    public function run(): void {\n",                                        // 8
        "        $result = (new Api())->fetch();\n",                                  // 9
        "        $result->data;\n",                                                   // 10
        "        $result->meta;\n",                                                   // 11
        "    }\n",                                                                    // 12
        "}\n",                                                                        // 13
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "data" in `$result->data` on line 10
    let result = goto_definition(&backend, &uri, 10, 18).await;
    assert!(
        result.is_some(),
        "Should resolve 'data' in nested object shape"
    );
    assert_location(result.unwrap(), &uri, 3);

    // Cursor on "meta" in `$result->meta` on line 11
    let result = goto_definition(&backend, &uri, 11, 18).await;
    assert!(
        result.is_some(),
        "Should resolve 'meta' in nested object shape"
    );
    assert_location(result.unwrap(), &uri, 3);
}

/// When the object-shape-returning method is on `$this`, GTD on the
/// property should still jump to the docblock key.  This matches the
/// `example.php` ShapeMethodDemo pattern:
///   $profile = $this->getProfile();
///   $profile->name;   // Ctrl+Click → @return docblock
#[tokio::test]
async fn test_gtd_object_shape_this_method_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                            // 0
        "class ShapeDemo {\n",                                                // 1
        "    public function demo(): void {\n",                               // 2
        "        $profile = $this->getProfile();\n",                          // 3
        "        $profile->name;\n",                                          // 4
        "        $profile->age;\n",                                           // 5
        "    }\n",                                                            // 6
        "    /** @return object{name: string, age: int, active: bool} */\n",  // 7
        "    public function getProfile(): object { return (object) []; }\n", // 8
        "}\n",                                                                // 9
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "name" in `$profile->name` on line 4
    let result = goto_definition(&backend, &uri, 4, 19).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'name' from $this->getProfile()"
    );
    assert_location(result.unwrap(), &uri, 7);

    // Cursor on "age" in `$profile->age` on line 5
    let result = goto_definition(&backend, &uri, 5, 19).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'age' from $this->getProfile()"
    );
    assert_location(result.unwrap(), &uri, 7);
}

/// Chaining through an object shape property that itself is a class:
///   $result = $this->getResult();
///   $result->tool->write();   // Ctrl+Click `tool` → @return docblock
#[tokio::test]
async fn test_gtd_object_shape_chain_first_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                     // 0
        "class Pen {\n",                                                               // 1
        "    public function write(): string { return ''; }\n",                        // 2
        "}\n",                                                                         // 3
        "class ChainDemo {\n",                                                         // 4
        "    public function demo(): void {\n",                                        // 5
        "        $result = $this->getResult();\n",                                     // 6
        "        $result->tool->write();\n",                                           // 7
        "        $result->meta->page;\n",                                              // 8
        "    }\n",                                                                     // 9
        "    /** @return object{tool: Pen, meta: object{page: int, total: int}} */\n", // 10
        "    public function getResult(): object { return (object) []; }\n",           // 11
        "}\n",                                                                         // 12
    );
    open_php(&backend, &uri, text).await;

    // Cursor on "tool" in `$result->tool` on line 7
    let result = goto_definition(&backend, &uri, 7, 18).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'tool' from $this->getResult()"
    );
    assert_location(result.unwrap(), &uri, 10);

    // Cursor on "meta" in `$result->meta` on line 8
    let result = goto_definition(&backend, &uri, 8, 18).await;
    assert!(
        result.is_some(),
        "Should resolve object shape property 'meta' from $this->getResult()"
    );
    assert_location(result.unwrap(), &uri, 10);
}

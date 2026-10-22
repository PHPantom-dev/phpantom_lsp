use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp::lsp_types::*;

// ─── Object Shape Parsing Unit Tests ────────────────────────────────────────

#[test]
fn test_parse_object_shape_basic() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries =
        parse_object_shape_typed(&PhpType::parse("object{foo: int, bar: string}")).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key.as_deref(), Some("foo"));
    assert_eq!(entries[0].value_type.to_string(), "int");
    assert!(!entries[0].optional);
    assert_eq!(entries[1].key.as_deref(), Some("bar"));
    assert_eq!(entries[1].value_type.to_string(), "string");
    assert!(!entries[1].optional);
}

#[test]
fn test_parse_object_shape_optional_property() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries =
        parse_object_shape_typed(&PhpType::parse("object{foo: int, bar?: string}")).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key.as_deref(), Some("foo"));
    assert!(!entries[0].optional);
    assert_eq!(entries[1].key.as_deref(), Some("bar"));
    assert_eq!(entries[1].value_type.to_string(), "string");
    assert!(entries[1].optional);
}

#[test]
fn test_parse_object_shape_quoted_keys() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries =
        parse_object_shape_typed(&PhpType::parse(r#"object{'foo': int, "bar": string}"#)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key.as_deref(), Some("foo"));
    assert_eq!(entries[0].value_type.to_string(), "int");
    assert_eq!(entries[1].key.as_deref(), Some("bar"));
    assert_eq!(entries[1].value_type.to_string(), "string");
}

#[test]
fn test_parse_object_shape_quoted_optional() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries =
        parse_object_shape_typed(&PhpType::parse(r#"object{'foo': int, "bar"?: string}"#)).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(!entries[0].optional);
    assert!(entries[1].optional);
}

#[test]
fn test_parse_object_shape_empty() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries = parse_object_shape_typed(&PhpType::parse("object{}")).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn test_parse_object_shape_nullable() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries = parse_object_shape_typed(&PhpType::parse("?object{foo: int}")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key.as_deref(), Some("foo"));
    assert_eq!(entries[0].value_type.to_string(), "int");
}

#[test]
fn test_parse_object_shape_canonical_form() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    // After resolution, `object` never has a leading `\` — verify canonical input works.
    let entries = parse_object_shape_typed(&PhpType::parse("object{foo: int}")).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key.as_deref(), Some("foo"));
}

#[test]
fn test_parse_object_shape_complex_value_types() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    let entries = parse_object_shape_typed(&PhpType::parse(
        "object{user: User, items: list<Item>, meta: array{page: int}}",
    ))
    .unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].key.as_deref(), Some("user"));
    assert_eq!(entries[0].value_type.to_string(), "User");
    assert_eq!(entries[1].key.as_deref(), Some("items"));
    assert_eq!(entries[1].value_type.to_string(), "list<Item>");
    assert_eq!(entries[2].key.as_deref(), Some("meta"));
    assert_eq!(entries[2].value_type.to_string(), "array{page: int}");
}

#[test]
fn test_parse_object_shape_not_an_object_shape() {
    use phpantom_lsp::docblock::parse_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    assert!(parse_object_shape_typed(&PhpType::parse("array{foo: int}")).is_none());
    assert!(parse_object_shape_typed(&PhpType::parse("string")).is_none());
    assert!(parse_object_shape_typed(&PhpType::parse("object")).is_none());
    assert!(parse_object_shape_typed(&PhpType::parse("User")).is_none());
}

#[test]
fn test_is_object_shape() {
    use phpantom_lsp::docblock::is_object_shape_typed;
    use phpantom_lsp::php_type::PhpType;

    assert!(is_object_shape_typed(&PhpType::parse("object{foo: int}")));
    assert!(is_object_shape_typed(&PhpType::parse("?object{foo: int}")));

    assert!(is_object_shape_typed(&PhpType::parse("object{}")));
    assert!(!is_object_shape_typed(&PhpType::parse("object")));
    assert!(!is_object_shape_typed(&PhpType::parse("array{foo: int}")));
    assert!(!is_object_shape_typed(&PhpType::parse("string")));
}

#[test]
fn test_extract_object_shape_property_type() {
    use phpantom_lsp::docblock::extract_object_shape_property_type_typed;
    use phpantom_lsp::php_type::PhpType;

    assert_eq!(
        extract_object_shape_property_type_typed(
            &PhpType::parse("object{name: string, user: User}"),
            "user"
        )
        .map(|t| t.to_string()),
        Some("User".to_string())
    );
    assert_eq!(
        extract_object_shape_property_type_typed(
            &PhpType::parse("object{name: string, user: User}"),
            "name"
        )
        .map(|t| t.to_string()),
        Some("string".to_string())
    );
    assert_eq!(
        extract_object_shape_property_type_typed(
            &PhpType::parse("object{name: string, user: User}"),
            "missing"
        )
        .map(|t| t.to_string()),
        None
    );
    assert_eq!(
        extract_object_shape_property_type_typed(&PhpType::parse("array{name: string}"), "name")
            .map(|t| t.to_string()),
        None
    );
}

#[test]
fn test_extract_object_shape_property_type_quoted_key() {
    use phpantom_lsp::docblock::extract_object_shape_property_type_typed;
    use phpantom_lsp::php_type::PhpType;

    let parsed = PhpType::parse(r#"object{"host": string, 'port': int, ssl: bool}"#);
    assert_eq!(
        extract_object_shape_property_type_typed(&parsed, "host").map(|t| t.to_string()),
        Some("string".to_string())
    );
    assert_eq!(
        extract_object_shape_property_type_typed(&parsed, "port").map(|t| t.to_string()),
        Some("int".to_string())
    );
    assert_eq!(
        extract_object_shape_property_type_typed(&parsed, "ssl").map(|t| t.to_string()),
        Some("bool".to_string())
    );
}

// ─── Object Shape Completion Integration Tests ──────────────────────────────

/// Basic object shape: `@return object{foo: int, bar: string}` should
/// suggest properties `foo` and `bar` when accessed via `->`.
#[tokio::test]
async fn test_object_shape_basic_property_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    /**\n",
        "     * @return object{name: string, age: int, active: bool}\n",
        "     */\n",
        "    public function getData(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "}\n",
        "$svc = new Service();\n",
        "$result = $svc->getData();\n",
        "$result->\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor right after `$result->` (line 11, char 9)
    let result = completion_response(&backend, &uri, 11, 9, None).await;
    assert!(
        result.is_some(),
        "Should return completions for object shape properties"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"name"),
                "Should suggest 'name', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"age"),
                "Should suggest 'age', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"active"),
                "Should suggest 'active', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape with optional property: `object{foo: int, bar?: string}`
/// should suggest both properties.
#[tokio::test]
async fn test_object_shape_optional_property_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_optional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Maker {\n",
        "    /**\n",
        "     * @return object{required: int, optional?: string}\n",
        "     */\n",
        "    public function make(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "}\n",
        "$m = new Maker();\n",
        "$obj = $m->make();\n",
        "$obj->\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 11, 6, None).await;
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"required"),
                "Should suggest 'required', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"optional"),
                "Should suggest 'optional', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape with class value type: `object{user: User}` should
/// chain-resolve `$obj->user->` to User members.
#[tokio::test]
async fn test_object_shape_value_type_chain_resolution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Handler {\n",
        "    /**\n",
        "     * @return object{user: User, count: int}\n",
        "     */\n",
        "    public function process(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "}\n",
        "$h = new Handler();\n",
        "$data = $h->process();\n",
        "$data->user->\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor right after `$data->user->` (line 15, char 14)
    let result = completion_response(&backend, &uri, 15, 14, None).await;
    assert!(
        result.is_some(),
        "Should chain-resolve object shape property type to User"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                method_names.contains(&"getEmail"),
                "Should suggest User::getEmail(), got {:?}",
                method_names
            );
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"name"),
                "Should suggest User::$name, got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape via inline @var annotation on a variable.
#[tokio::test]
async fn test_object_shape_var_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/** @var object{title: string, score: float} $item */\n",
        "$item = getUnknown();\n",
        "$item->\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 3, 7, None).await;
    assert!(
        result.is_some(),
        "Should return completions from @var object shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"title"),
                "Should suggest 'title', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"score"),
                "Should suggest 'score', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape via @param annotation in a method.
#[tokio::test]
async fn test_object_shape_param_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Handler {\n",
        "    /**\n",
        "     * @param object{host: string, port: int, ssl: bool} $config\n",
        "     */\n",
        "    public function connect(object $config): void {\n",
        "        $config->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor right after `$config->` (line 6, char 17)
    let result = completion_response(&backend, &uri, 6, 17, None).await;
    assert!(
        result.is_some(),
        "Should return completions from @param object shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"host"),
                "Should suggest 'host', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"port"),
                "Should suggest 'port', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"ssl"),
                "Should suggest 'ssl', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape intersected with \stdClass:
/// `object{foo: int, bar: string}&\stdClass`
/// should offer properties from the object shape AND any members from stdClass.
#[tokio::test]
async fn test_object_shape_intersection_with_stdclass() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_intersection.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    /**\n",
        "     * @return object{name: string, value: int}&\\stdClass\n",
        "     */\n",
        "    public function create(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "}\n",
        "$f = new Factory();\n",
        "$obj = $f->create();\n",
        "$obj->\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 11, 6, None).await;
    assert!(
        result.is_some(),
        "Should return completions for object shape intersection"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // At minimum, the object shape properties should be present
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"name"),
                "Should suggest 'name' from object shape, got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"value"),
                "Should suggest 'value' from object shape, got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape in a union type: `object{foo: int}|null` should still
/// offer property completions.
#[tokio::test]
async fn test_object_shape_in_union_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Provider {\n",
        "    /**\n",
        "     * @return object{status: string, code: int}|null\n",
        "     */\n",
        "    public function fetch(): ?object {\n",
        "        return null;\n",
        "    }\n",
        "}\n",
        "$p = new Provider();\n",
        "$result = $p->fetch();\n",
        "$result->\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 11, 9, None).await;
    assert!(
        result.is_some(),
        "Should return completions for nullable object shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"status"),
                "Should suggest 'status', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"code"),
                "Should suggest 'code', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Bare `object` return type (without `{…}`) should NOT produce property
/// completions.
#[tokio::test]
async fn test_bare_object_no_completions() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function get(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "}\n",
        "$s = new Service();\n",
        "$obj = $s->get();\n",
        "$obj->\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 8, 6, None).await;
    // Bare `object` has no properties — result should be None.
    if let Some(resp) = result {
        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let class_members: Vec<&str> = items
            .iter()
            .filter(|i| {
                i.kind == Some(CompletionItemKind::METHOD)
                    || i.kind == Some(CompletionItemKind::PROPERTY)
            })
            .map(|i| i.label.as_str())
            .collect();
        assert!(
            class_members.is_empty(),
            "Bare 'object' should not produce member completions, got {:?}",
            class_members
        );
    }
}

/// Object shape inside a class method using $this->method().
#[tokio::test]
async fn test_object_shape_from_this_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Api {\n",
        "    /**\n",
        "     * @return object{id: int, title: string}\n",
        "     */\n",
        "    public function getItem(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "\n",
        "    public function test(): void {\n",
        "        $item = $this->getItem();\n",
        "        $item->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor right after `$item->` (line 11, char 15)
    let result = completion_response(&backend, &uri, 11, 15, None).await;
    assert!(
        result.is_some(),
        "Should return completions from $this->getItem() object shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"id"),
                "Should suggest 'id', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"title"),
                "Should suggest 'title', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape with nested object shape value type:
/// `object{meta: object{page: int, total: int}}` should chain-resolve.
#[tokio::test]
async fn test_object_shape_nested_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Paginator {\n",
        "    /**\n",
        "     * @return object{items: array, meta: object{page: int, total: int}}\n",
        "     */\n",
        "    public function paginate(): object {\n",
        "        return (object) [];\n",
        "    }\n",
        "}\n",
        "$p = new Paginator();\n",
        "$result = $p->paginate();\n",
        "$result->meta->\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor right after `$result->meta->` (line 11, char 16)
    let result = completion_response(&backend, &uri, 11, 16, None).await;
    assert!(result.is_some(), "Should chain-resolve nested object shape");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"page"),
                "Should suggest 'page' from inner object shape, got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"total"),
                "Should suggest 'total' from inner object shape, got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape from a standalone function return type.
#[tokio::test]
async fn test_object_shape_from_function_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @return object{success: bool, message: string}\n",
        " */\n",
        "function apiCall(): object {\n",
        "    return (object) [];\n",
        "}\n",
        "$response = apiCall();\n",
        "$response->\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor right after `$response->` (line 8, char 12)
    let result = completion_response(&backend, &uri, 8, 12, None).await;
    assert!(
        result.is_some(),
        "Should return completions from function return object shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                prop_labels.contains(&"success"),
                "Should suggest 'success', got {:?}",
                prop_labels
            );
            assert!(
                prop_labels.contains(&"message"),
                "Should suggest 'message', got {:?}",
                prop_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape does not interfere with array shapes — a `@var` annotation
/// with `array{…}` should still offer key completions as before.
#[tokio::test]
async fn test_object_shape_does_not_break_array_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///object_shape_no_break.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/** @var array{user: string, count: int} $data */\n",
        "$data = getStats();\n",
        "$data['\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 3, 7, None).await;
    assert!(
        result.is_some(),
        "Array shape key completion should still work"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.contains(&"user"),
                "Should suggest 'user' array key, got {:?}",
                labels
            );
            assert!(
                labels.contains(&"count"),
                "Should suggest 'count' array key, got {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

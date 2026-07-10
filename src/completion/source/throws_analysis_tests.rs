use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ClassLikeKind, FunctionInfo, MethodInfo, Visibility};

// ── Low-level scanning tests ────────────────────────────────────────

#[test]
fn test_find_throw_statements_basic() {
    let body = r#"
        throw new InvalidArgumentException("bad");
        throw new \RuntimeException("oops");
    "#;
    let throws = find_throw_statements(body);
    assert_eq!(throws.len(), 2);
    assert_eq!(
        throws[0].type_name,
        PhpType::Named("InvalidArgumentException".to_string())
    );
    assert_eq!(
        throws[1].type_name,
        PhpType::Named("\\RuntimeException".to_string())
    );
}

#[test]
fn test_find_throw_statements_skips_strings() {
    let body = r#"
        $msg = "throw new FakeException()";
        throw new RealException("msg");
    "#;
    let throws = find_throw_statements(body);
    assert_eq!(throws.len(), 1);
    assert_eq!(
        throws[0].type_name,
        PhpType::Named("RealException".to_string())
    );
}

#[test]
fn test_find_throw_statements_skips_comments() {
    let body = r#"
        // throw new CommentException();
        /* throw new BlockException(); */
        throw new RealException("msg");
    "#;
    let throws = find_throw_statements(body);
    assert_eq!(throws.len(), 1);
    assert_eq!(
        throws[0].type_name,
        PhpType::Named("RealException".to_string())
    );
}

#[test]
fn test_find_method_throws_tags_basic() {
    let content = r#"
/**
 * @throws InvalidArgumentException
 * @throws \RuntimeException
 */
public function doSomething(): void {
}
    "#;
    let tags = find_method_throws_tags(content, "doSomething");
    assert_eq!(
        tags,
        vec![
            PhpType::Named("InvalidArgumentException".to_string()),
            PhpType::Named("RuntimeException".to_string())
        ]
    );
}

#[test]
fn test_find_method_throws_tags_with_modifiers() {
    let content = r#"
/**
 * @throws InvalidArgumentException
 */
private static function doSomething(): void {
}
    "#;
    let tags = find_method_throws_tags(content, "doSomething");
    assert_eq!(
        tags,
        vec![PhpType::Named("InvalidArgumentException".to_string())]
    );
}

#[test]
fn test_find_method_return_type_native() {
    let content = r#"
public function createException(): RuntimeException {
}
    "#;
    let ret = find_method_return_type(content, "createException");
    assert_eq!(ret, Some(PhpType::Named("RuntimeException".to_string())));
}

#[test]
fn test_find_method_return_type_docblock() {
    let content = r#"
/**
 * @return RuntimeException
 */
public function createException() {
}
    "#;
    let ret = find_method_return_type(content, "createException");
    assert_eq!(ret, Some(PhpType::Named("RuntimeException".to_string())));
}

#[test]
fn test_find_method_return_type_skips_void() {
    let content = r#"
/**
 * @return void
 */
public function doNothing() {
}
    "#;
    let ret = find_method_return_type(content, "doNothing");
    assert_eq!(ret, None);
}

#[test]
fn test_find_inline_throws_annotations() {
    let body = r#"
        /** @throws InvalidArgumentException */
        $client->request();
        /** @throws RuntimeException when things go wrong */
        $db->query();
    "#;
    let annotations = find_inline_throws_annotations(body);
    let names: Vec<String> = annotations
        .iter()
        .map(|t| t.type_name.to_string())
        .collect();
    assert_eq!(names, vec!["InvalidArgumentException", "RuntimeException"]);
}

#[test]
fn test_find_propagated_throws() {
    let file_content = r#"
/**
 * @throws IOException
 * @throws NetworkException
 */
public function riskyMethod(): void {
    // ...
}

public function caller(): void {
    $this->riskyMethod();
}
    "#;
    // Scan the body of `caller`
    let body = "$this->riskyMethod();";
    let propagated = find_propagated_throws(body, file_content);
    let names: Vec<String> = propagated.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(names, vec!["IOException", "NetworkException"]);
}

#[test]
fn test_find_throw_expression_types() {
    let file_content = r#"
public function createException(): RuntimeException {
    return new RuntimeException("oops");
}

public function caller(): void {
    throw $this->createException();
}
    "#;
    let body = "throw $this->createException();";
    let types = find_throw_expression_types(body, file_content);
    assert_eq!(types.len(), 1);
    assert_eq!(
        types[0].type_name,
        PhpType::Named("RuntimeException".to_string())
    );
}

#[test]
fn test_skip_modifiers_backward() {
    assert_eq!(skip_modifiers_backward("    public static "), "");
    assert_eq!(
        skip_modifiers_backward("/** @return void */ private "),
        "/** @return void */"
    );
    assert_eq!(
        skip_modifiers_backward("no modifiers here"),
        "no modifiers here"
    );
}

#[test]
fn test_find_method_return_type_with_nested_parens() {
    let content = r#"
public function createException(array $opts = array()): RuntimeException {
}
    "#;
    let ret = find_method_return_type(content, "createException");
    assert_eq!(ret, Some(PhpType::Named("RuntimeException".to_string())));
}

// ── High-level analysis tests ───────────────────────────────────────

#[test]
fn test_extract_function_body_basic() {
    let content = "<?php\n/** @return void */\nfunction foo(): void {\n    echo \"hello\";\n}\n";
    let pos = Position {
        line: 1,
        character: 5,
    };
    let body = extract_function_body(content, pos);
    assert!(body.is_some());
    assert!(body.unwrap().contains("echo"));
}

#[test]
fn test_extract_function_body_abstract() {
    let content = "<?php\n/** @return void */\nabstract function foo(): void;\n";
    let pos = Position {
        line: 1,
        character: 5,
    };
    let body = extract_function_body(content, pos);
    assert!(body.is_none());
}

#[test]
fn test_extract_function_body_with_nested_braces() {
    let content = concat!(
        "<?php\n",
        "/** @return void */\n",
        "function foo(): void {\n",
        "    if (true) {\n",
        "        echo 'inner';\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 1,
        character: 5,
    };
    let body = extract_function_body(content, pos).unwrap();
    assert!(body.contains("if (true)"));
    assert!(body.contains("echo 'inner'"));
}

#[test]
fn test_find_catch_blocks_basic() {
    let body = r#"
        try {
            throw new InvalidArgumentException("bad");
        } catch (InvalidArgumentException $e) {
            // handled
        }
        throw new RuntimeException("oops");
    "#;
    let catches = find_catch_blocks(body);
    assert_eq!(catches.len(), 1);
    assert_eq!(
        catches[0].type_names,
        vec![PhpType::Named("InvalidArgumentException".to_string())]
    );
}

#[test]
fn test_find_catch_blocks_multi_catch() {
    let body = r#"
        try {
            doSomething();
        } catch (InvalidArgumentException | RuntimeException $e) {
            // handled
        }
    "#;
    let catches = find_catch_blocks(body);
    assert_eq!(catches.len(), 1);
    assert_eq!(
        catches[0].type_names,
        vec![
            PhpType::Named("InvalidArgumentException".to_string()),
            PhpType::Named("RuntimeException".to_string())
        ]
    );
}

#[test]
fn test_parse_catch_types_basic() {
    let (types, var) = parse_catch_types("InvalidArgumentException $e");
    assert_eq!(
        types,
        vec![PhpType::Named("InvalidArgumentException".to_string())]
    );
    assert_eq!(var.as_deref(), Some("$e"));
}

#[test]
fn test_parse_catch_types_multi() {
    let (types, var) = parse_catch_types("\\InvalidArgumentException | \\RuntimeException $e");
    assert_eq!(
        types,
        vec![
            PhpType::Named("InvalidArgumentException".to_string()),
            PhpType::Named("RuntimeException".to_string())
        ]
    );
    assert_eq!(var.as_deref(), Some("$e"));
}

#[test]
fn test_find_uncaught_throw_types_all_caught() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new InvalidArgumentException(\"bad\");\n",
        "        } catch (InvalidArgumentException $e) {\n",
        "            // handled\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert!(uncaught.is_empty());
}

#[test]
fn test_find_uncaught_throw_types_uncaught() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        throw new RuntimeException(\"oops\");\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert_eq!(
        uncaught,
        vec![PhpType::Named("RuntimeException".to_string())]
    );
}

#[test]
fn test_find_uncaught_throw_types_mixed() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new InvalidArgumentException(\"bad\");\n",
        "        } catch (InvalidArgumentException $e) {\n",
        "            // handled\n",
        "        }\n",
        "        throw new RuntimeException(\"oops\");\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert_eq!(
        uncaught,
        vec![PhpType::Named("RuntimeException".to_string())]
    );
}

#[test]
fn test_find_uncaught_throw_types_inline_annotation_caught() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            /** @throws NotFoundException */\n",
        "            findOrFail();\n",
        "        } catch (NotFoundException $e) {\n",
        "            // handled\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert!(
        uncaught.is_empty(),
        "inline @throws inside try/catch should be excluded, got: {:?}",
        uncaught
    );
}

#[test]
fn test_find_uncaught_throw_types_inline_annotation_partially_caught() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            /** @throws NotFoundException */\n",
        "            findOrFail();\n",
        "        } catch (NotFoundException $e) {\n",
        "            // handled\n",
        "        }\n",
        "        /** @throws RuntimeException */\n",
        "        riskyCall();\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert_eq!(
        uncaught,
        vec![PhpType::Named("RuntimeException".to_string())],
        "only the uncaught inline @throws should remain"
    );
}

// ── throw $variable tests ───────────────────────────────────────────

#[test]
fn test_find_uncaught_throw_variable_from_catch() {
    // `throw $e` inside a catch block re-throws the caught type.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new ValidationException('bad');\n",
        "        } catch (ValidationException $e) {\n",
        "            throw $e;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert_eq!(
        uncaught,
        vec![PhpType::Named("ValidationException".to_string())],
        "re-thrown catch variable should appear in uncaught list"
    );
}

#[test]
fn test_find_uncaught_throw_variable_not_rethrown() {
    // The caught exception is NOT re-thrown — should be empty.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new ValidationException('bad');\n",
        "        } catch (ValidationException $e) {\n",
        "            // swallowed\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert!(
        uncaught.is_empty(),
        "caught and not re-thrown should be empty, got: {:?}",
        uncaught
    );
}

#[test]
fn test_find_uncaught_throw_variable_multiple_catches() {
    // Two catch blocks, each re-throwing — both types should appear.
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            doSomething();\n",
        "        } catch (ValidationException $e) {\n",
        "            throw $e;\n",
        "        } catch (NotFoundException $e) {\n",
        "            throw $e;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 3,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert!(
        uncaught.contains(&PhpType::Named("ValidationException".to_string())),
        "should contain ValidationException, got: {:?}",
        uncaught
    );
    assert!(
        uncaught.contains(&PhpType::Named("NotFoundException".to_string())),
        "should contain NotFoundException, got: {:?}",
        uncaught
    );
}

// ── throw functionCall() tests ──────────────────────────────────────

#[test]
fn test_find_throw_expression_bare_function_call() {
    let file_content = r#"
function makeException(): RuntimeException {
    return new RuntimeException("oops");
}

public function caller(): void {
    throw makeException();
}
    "#;
    let body = "throw makeException();";
    let types = find_throw_expression_types(body, file_content);
    assert_eq!(types.len(), 1, "should resolve bare function call");
    assert_eq!(
        types[0].type_name,
        PhpType::Named("RuntimeException".to_string())
    );
}

#[test]
fn test_find_uncaught_throw_bare_function_call() {
    let content = concat!(
        "<?php\n",
        "function makeException(): RuntimeException {\n",
        "    return new RuntimeException('oops');\n",
        "}\n",
        "class Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        throw makeException();\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 6,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert_eq!(
        uncaught,
        vec![PhpType::Named("RuntimeException".to_string())],
        "bare function call return type should appear in uncaught"
    );
}

#[test]
fn test_find_uncaught_throw_bare_function_caught() {
    // throw functionCall() inside a try/catch that catches it.
    let content = concat!(
        "<?php\n",
        "function makeException(): RuntimeException {\n",
        "    return new RuntimeException('oops');\n",
        "}\n",
        "class Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw makeException();\n",
        "        } catch (RuntimeException $e) {\n",
        "            // handled\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 6,
        character: 5,
    };
    let uncaught = find_uncaught_throw_types(content, pos);
    assert!(
        uncaught.is_empty(),
        "caught bare function throw should be empty, got: {:?}",
        uncaught
    );
}

// ── Import helper tests ─────────────────────────────────────────────

#[test]
fn test_resolve_exception_fqn_from_use_map() {
    let mut use_map = HashMap::new();
    use_map.insert(
        "RuntimeException".to_string(),
        "App\\Exceptions\\RuntimeException".to_string(),
    );
    let result = crate::util::resolve_to_fqn("RuntimeException", &use_map, &None);
    assert_eq!(result, "App\\Exceptions\\RuntimeException".to_string());
}

#[test]
fn test_resolve_exception_fqn_from_namespace() {
    let use_map = HashMap::new();
    let ns = Some("App\\Services".to_string());
    let result = crate::util::resolve_to_fqn("CustomException", &use_map, &ns);
    assert_eq!(result, "App\\Services\\CustomException".to_string());
}

#[test]
fn test_resolve_exception_fqn_global() {
    let use_map = HashMap::new();
    let result = crate::util::resolve_to_fqn("RuntimeException", &use_map, &None);
    assert_eq!(result, "RuntimeException".to_string());
}

#[test]
fn test_has_use_import_direct() {
    let content = "<?php\nuse App\\Exceptions\\RuntimeException;\n";
    assert!(has_use_import(content, "App\\Exceptions\\RuntimeException"));
    assert!(!has_use_import(content, "App\\Exceptions\\LogicException"));
}

#[test]
fn test_has_use_import_group() {
    let content = "<?php\nuse App\\Exceptions\\{RuntimeException, LogicException};\n";
    assert!(has_use_import(content, "App\\Exceptions\\RuntimeException"));
    assert!(has_use_import(content, "App\\Exceptions\\LogicException"));
    assert!(!has_use_import(content, "App\\Exceptions\\CustomException"));
}

#[test]
fn test_has_use_import_alias() {
    let content = "<?php\nuse App\\Exceptions\\RuntimeException as RE;\n";
    assert!(has_use_import(content, "App\\Exceptions\\RuntimeException"));
}

// ── parse_param_type_map tests ──────────────────────────────────────

#[test]
fn test_parse_param_type_map_basic() {
    let sig = "handle(BusinessCentralService $service): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![(
            "$service".to_string(),
            PhpType::Named("BusinessCentralService".to_string())
        )]
    );
}

#[test]
fn test_parse_param_type_map_multiple_params() {
    let sig = "handle(BusinessCentralService $service, int $count, string $name): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![
            (
                "$service".to_string(),
                PhpType::Named("BusinessCentralService".to_string())
            ),
            ("$count".to_string(), PhpType::Named("int".to_string())),
            ("$name".to_string(), PhpType::Named("string".to_string())),
        ]
    );
}

#[test]
fn test_parse_param_type_map_nullable() {
    let sig = "handle(?Model $model): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![("$model".to_string(), PhpType::Named("Model".to_string()))]
    );
}

#[test]
fn test_parse_param_type_map_fqn() {
    let sig = r"handle(\App\Services\BusinessCentralService $service): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![(
            "$service".to_string(),
            PhpType::Named("App\\Services\\BusinessCentralService".to_string())
        ),]
    );
}

#[test]
fn test_parse_param_type_map_no_type() {
    let sig = "handle($service): void";
    let map = parse_param_type_map(sig);
    assert!(map.is_empty());
}

#[test]
fn test_parse_param_type_map_with_defaults() {
    let sig = "handle(string $name = 'foo', int $count = 0): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![
            ("$name".to_string(), PhpType::Named("string".to_string())),
            ("$count".to_string(), PhpType::Named("int".to_string())),
        ]
    );
}

#[test]
fn test_parse_param_type_map_variadic() {
    let sig = "handle(string ...$names): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![("$names".to_string(), PhpType::Named("string".to_string()))]
    );
}

#[test]
fn test_parse_param_type_map_reference() {
    let sig = "handle(array &$items): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![("$items".to_string(), PhpType::Named("array".to_string()))]
    );
}

#[test]
fn test_parse_param_type_map_promoted_property() {
    let sig = "__construct(public readonly string $name): void";
    let map = parse_param_type_map(sig);
    assert_eq!(
        map,
        vec![("$name".to_string(), PhpType::Named("string".to_string()))]
    );
}

// ── extract_function_signature tests ────────────────────────────────

#[test]
fn test_extract_function_signature_basic() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function handle(BusinessCentralService $service): void {\n",
        "        $service->doStuff();\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 2,
        character: 5,
    };
    let sig = extract_function_signature(content, pos);
    assert!(sig.contains("BusinessCentralService $service"));
    assert!(sig.contains("handle"));
}

// ── find_typed_variable_propagated_throws tests ─────────────────────

fn make_class_with_throws(name: &str, methods: Vec<(&str, Vec<&str>)>) -> Arc<ClassInfo> {
    let method_infos: Vec<MethodInfo> = methods
        .into_iter()
        .map(|(method_name, throws)| MethodInfo {
            name: crate::atom::atom(method_name),
            name_offset: 0,
            parameters: Vec::new(),
            return_type: None,
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static: false,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: Vec::new(),
            template_param_bounds: Default::default(),
            template_bindings: Vec::new(),
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: false,
            type_assertions: Vec::new(),
            throws: throws.into_iter().map(PhpType::parse).collect(),
            if_this_is: None,
        })
        .collect();

    Arc::new(ClassInfo {
        kind: ClassLikeKind::Class,
        name: crate::atom::atom(name),
        methods: method_infos
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>()
            .into(),
        method_index: Default::default(),
        indexed_method_count: 0,
        properties: Default::default(),
        constants: Default::default(),
        start_offset: 0,
        end_offset: 0,
        keyword_offset: 0,
        decl_start_offset: 0,
        parent_class: None,
        interfaces: Vec::new(),
        used_traits: Vec::new(),
        mixins: Vec::new(),
        mixin_generics: Vec::new(),
        is_final: false,
        is_abstract: false,
        deprecation_message: None,
        deprecated_replacement: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        template_params: Vec::new(),
        template_param_bounds: Default::default(),
        template_param_defaults: Default::default(),
        extends_generics: Vec::new(),
        implements_generics: Vec::new(),
        use_generics: Vec::new(),
        type_aliases: Default::default(),
        trait_precedences: Vec::new(),
        trait_aliases: Vec::new(),
        class_docblock: None,
        file_namespace: None,
        backed_type: None,
        attribute_targets: 0,
        laravel: None,
    })
}

#[test]
fn test_find_cross_file_propagated_throws_basic() {
    let body = "$service->sendDataToBusinessCentral($this->customer, BusinessCentralEventTypeEnum::CREATE_CUSTOMER);";
    let signature = "handle(BusinessCentralService $service): void";
    let file_content = "";

    let bc_class = make_class_with_throws(
        "BusinessCentralService",
        vec![(
            "sendDataToBusinessCentral",
            vec![
                "BusinessCentralException",
                "ConvertException",
                "RuntimeException",
                "RandomException",
            ],
        )],
    );

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "BusinessCentralService" {
            Some(bc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(
        names,
        vec![
            "BusinessCentralException",
            "ConvertException",
            "RuntimeException",
            "RandomException",
        ]
    );
}

#[test]
fn test_find_cross_file_propagated_throws_skips_this() {
    let body = "$this->riskyMethod();";
    let signature = "handle(): void";
    let file_content = "";

    let class_loader = |_name: &str| -> Option<Arc<ClassInfo>> { None };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    assert!(
        results.is_empty(),
        "$this-> calls should be handled by find_propagated_throws, not here"
    );
}

#[test]
fn test_find_cross_file_propagated_throws_unknown_variable() {
    let body = "$unknown->doStuff();";
    let signature = "handle(SomeService $service): void";
    let file_content = "";

    let class_loader = |_name: &str| -> Option<Arc<ClassInfo>> { None };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    assert!(results.is_empty());
}

#[test]
fn test_find_cross_file_propagated_throws_property_access_ignored() {
    let body = "$service->someProperty;";
    let signature = "handle(SomeService $service): void";
    let file_content = "";

    let class_loader = |_name: &str| -> Option<Arc<ClassInfo>> { None };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    assert!(
        results.is_empty(),
        "Property access should not be treated as a method call"
    );
}

#[test]
fn test_find_cross_file_propagated_throws_multiple_calls() {
    let body = concat!("$service->methodA();\n", "$service->methodB();\n",);
    let signature = "handle(MyService $service): void";
    let file_content = "";

    let svc_class = make_class_with_throws(
        "MyService",
        vec![
            ("methodA", vec!["IOException"]),
            ("methodB", vec!["NetworkException"]),
        ],
    );

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "MyService" {
            Some(svc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(names, vec!["IOException", "NetworkException"]);
}

#[test]
fn test_find_cross_file_propagated_throws_deduplicates_calls() {
    let body = concat!("$service->doStuff();\n", "$service->doStuff();\n",);
    let signature = "handle(MyService $service): void";
    let file_content = "";

    let svc_class = make_class_with_throws("MyService", vec![("doStuff", vec!["SomeException"])]);

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "MyService" {
            Some(svc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(
        names,
        vec!["SomeException"],
        "Duplicate calls should only produce throws once"
    );
}

#[test]
fn test_find_cross_file_propagated_throws_method_without_throws() {
    let body = "$service->safeMethod();";
    let signature = "handle(MyService $service): void";
    let file_content = "";

    let svc_class = make_class_with_throws("MyService", vec![("safeMethod", vec![])]);

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "MyService" {
            Some(svc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    assert!(results.is_empty());
}

#[test]
fn test_find_cross_file_propagated_throws_static_method_call() {
    let body = "BusinessCentralService::validate();";
    let signature = "handle(): void";
    let file_content = "";

    let bc_class = make_class_with_throws(
        "BusinessCentralService",
        vec![("validate", vec!["ValidationException"])],
    );

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "BusinessCentralService" {
            Some(bc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(names, vec!["ValidationException"]);
}

#[test]
fn test_find_cross_file_propagated_throws_static_skips_self() {
    let body = "self::validate();";
    let signature = "handle(): void";
    let file_content = "";

    let class_loader = |_name: &str| -> Option<Arc<ClassInfo>> { None };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    assert!(
        results.is_empty(),
        "self:: should be handled by same-file propagation"
    );
}

#[test]
fn test_find_cross_file_propagated_throws_function_call() {
    let body = "riskyFunction();";
    let signature = "handle(): void";
    let file_content = "";

    let func_info = FunctionInfo {
        name: crate::atom::atom("riskyFunction"),
        name_offset: 0,
        parameters: Vec::new(),
        return_type: None,
        native_return_type: None,
        description: None,
        return_description: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        namespace: None,
        conditional_return: None,
        type_assertions: Vec::new(),
        deprecation_message: None,
        deprecated_replacement: None,
        template_params: Vec::new(),
        template_param_bounds: Default::default(),
        template_bindings: Vec::new(),
        throws: vec![PhpType::parse("DatabaseException")],
        is_polyfill: false,
        overloads: vec![],
    };

    let class_loader = |_name: &str| -> Option<Arc<ClassInfo>> { None };
    let function_loader = move |name: &str| -> Option<FunctionInfo> {
        if name == "riskyFunction" {
            Some(func_info.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: Some(&function_loader),
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(names, vec!["DatabaseException"]);
}

#[test]
fn test_find_cross_file_propagated_throws_new_constructor() {
    let body = "new BusinessCentralClient($config);";
    let signature = "handle(): void";
    let file_content = "";

    let bc_class = make_class_with_throws(
        "BusinessCentralClient",
        vec![("__construct", vec!["ConnectionException"])],
    );

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "BusinessCentralClient" {
            Some(bc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert_eq!(names, vec!["ConnectionException"]);
}

#[test]
fn test_find_cross_file_propagated_throws_skips_php_keywords() {
    let body = concat!(
        "if ($x) {}\n",
        "foreach ($items as $item) {}\n",
        "return $result;\n",
    );
    let signature = "handle(): void";
    let file_content = "";

    let class_loader = |_name: &str| -> Option<Arc<ClassInfo>> { None };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: None,
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    assert!(
        results.is_empty(),
        "PHP keywords should not be treated as function calls"
    );
}

#[test]
fn test_find_cross_file_propagated_throws_mixed_patterns() {
    let body = concat!(
        "$service->sendData();\n",
        "BusinessCentralService::validate();\n",
        "new HttpClient();\n",
        "helperFunction();\n",
    );
    let signature = "handle(MyService $service): void";
    let file_content = "";

    let svc_class = make_class_with_throws("MyService", vec![("sendData", vec!["SendException"])]);
    let bc_class = make_class_with_throws(
        "BusinessCentralService",
        vec![("validate", vec!["ValidationException"])],
    );
    let http_class = make_class_with_throws(
        "HttpClient",
        vec![("__construct", vec!["ConnectionException"])],
    );

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "MyService" => Some(svc_class.clone()),
            "BusinessCentralService" => Some(bc_class.clone()),
            "HttpClient" => Some(http_class.clone()),
            _ => None,
        }
    };

    let func_info = FunctionInfo {
        name: crate::atom::atom("helperFunction"),
        name_offset: 0,
        parameters: Vec::new(),
        return_type: None,
        native_return_type: None,
        description: None,
        return_description: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        namespace: None,
        conditional_return: None,
        type_assertions: Vec::new(),
        deprecation_message: None,
        deprecated_replacement: None,
        template_params: Vec::new(),
        template_param_bounds: Default::default(),
        template_bindings: Vec::new(),
        throws: vec![PhpType::parse("HelperException")],
        is_polyfill: false,
        overloads: vec![],
    };

    let function_loader = move |name: &str| -> Option<FunctionInfo> {
        if name == "helperFunction" {
            Some(func_info.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let ctx = ThrowsContext {
        class_loader: &class_loader,
        function_loader: Some(&function_loader),
        use_map: &use_map,
        file_namespace: &file_namespace,
    };
    let results = find_cross_file_propagated_throws(body, signature, file_content, &ctx);
    let names: Vec<String> = results.iter().map(|t| t.type_name.to_string()).collect();
    assert!(
        names.contains(&"SendException".to_string()),
        "Should propagate from $service->sendData(), got: {:?}",
        names
    );
    assert!(
        names.contains(&"ValidationException".to_string()),
        "Should propagate from BusinessCentralService::validate(), got: {:?}",
        names
    );
    assert!(
        names.contains(&"ConnectionException".to_string()),
        "Should propagate from new HttpClient(), got: {:?}",
        names
    );
    assert!(
        names.contains(&"HelperException".to_string()),
        "Should propagate from helperFunction(), got: {:?}",
        names
    );
}

#[test]
fn test_find_uncaught_with_class_loader_catches_cross_file() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function handle(BusinessCentralService $service): void {\n",
        "        try {\n",
        "            $service->sendData();\n",
        "        } catch (RuntimeException $e) {\n",
        "            // handled\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let pos = Position {
        line: 2,
        character: 5,
    };

    let bc_class = make_class_with_throws(
        "BusinessCentralService",
        vec![("sendData", vec!["RuntimeException", "ConvertException"])],
    );

    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "BusinessCentralService" {
            Some(bc_class.clone())
        } else {
            None
        }
    };

    let use_map = HashMap::new();
    let file_namespace = None;
    let uncaught = find_uncaught_throw_types_with_context(
        content,
        pos,
        Some(&ThrowsContext {
            class_loader: &class_loader,
            function_loader: None,
            use_map: &use_map,
            file_namespace: &file_namespace,
        }),
    );
    // RuntimeException is caught, but ConvertException is not.
    assert!(
        !uncaught
            .iter()
            .any(|t| t == &PhpType::Named("RuntimeException".to_string())),
        "RuntimeException should be caught"
    );
    assert!(
        uncaught
            .iter()
            .any(|t| t == &PhpType::Named("ConvertException".to_string())),
        "ConvertException should be uncaught, got: {:?}",
        uncaught
    );
}

// ── extract_throws_tags tests ───────────────────────────────────────

#[test]
fn test_extract_throws_tags_basic() {
    let docblock = "/**\n * @throws BusinessCentralException\n * @throws ConvertException\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert_eq!(
        tags,
        vec![
            PhpType::parse("BusinessCentralException"),
            PhpType::parse("ConvertException"),
        ]
    );
}

#[test]
fn test_extract_throws_tags_with_fqn() {
    let docblock = "/**\n * @throws \\App\\Exceptions\\CustomException\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert_eq!(
        tags,
        vec![PhpType::parse("App\\Exceptions\\CustomException")]
    );
}

#[test]
fn test_extract_throws_tags_with_description() {
    let docblock = "/**\n * @throws RuntimeException When something goes wrong\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert_eq!(tags, vec![PhpType::parse("RuntimeException")]);
}

#[test]
fn test_extract_throws_tags_empty_docblock() {
    let docblock = "/**\n * Some description\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert!(tags.is_empty());
}
